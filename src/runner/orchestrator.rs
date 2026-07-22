use crate::parse::*;
use crate::generate::*;
use crate::runner::executor::*;
use crate::runner::validator::*;
use crate::runner::response_assertions::assert_response;
use crate::runner::value_resolver::extract_field_values;
use crate::config::models::*;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Orchestrates the full test pipeline:
/// 1. Parse IG package
/// 2. Resolve dependencies → creation order
/// 3. Generate or load fixture resources
/// 4. Generate test plan from CapabilityStatement
/// 5. Create setup resources on the server
/// 6. Execute test cases
/// 7. Validate responses against profiles
/// 8. Cleanup (delete created resources)
pub struct Orchestrator {
    config: TestConfig,
}

/// Summary report of a test run.
#[derive(Debug, serde::Serialize)]
pub struct RunReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<TestResult>,
}

impl std::fmt::Display for RunReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== FHIR IG Test Results ===")?;
        writeln!(
            f,
            "Total: {} | Passed: {} | Failed: {}",
            self.total, self.passed, self.failed
        )?;
        writeln!(f, "---")?;
        for result in &self.results {
            let status = if result.passed { "PASS" } else { "FAIL" };
            writeln!(
                f,
                "[{}] {} (HTTP {})",
                status, result.test_name, result.status_code
            )?;
            for err in &result.validation_errors {
                writeln!(f, "  - {}", err)?;
            }
        }
        Ok(())
    }
}

impl Orchestrator {
    pub fn new(config: TestConfig) -> Self {
        Self { config }
    }

    /// Run the full test pipeline against the server.
    pub async fn run(&self, ig_package_path: &str) -> Result<RunReport> {
        // 1. Parse the IG package
        let pkg = parse_package(ig_package_path)?;

        // Prefer a server-mode CapabilityStatement; fall back to first if none found
        let cs = pkg
            .capability_statements
            .iter()
            .find(|cs| cs.rest.iter().any(|r| r.mode == "server" && !r.resource.is_empty()))
            .or_else(|| pkg.capability_statements.iter().find(|cs| cs.rest.iter().any(|r| !r.resource.is_empty())))
            .or(pkg.capability_statements.first())
            .context("No CapabilityStatement found in IG package")?;

        // 2. Extract dependencies and determine creation order
        let auto_deps = extract_dependencies(&pkg.structure_definitions);
        let auto_order = resolve_creation_order(&auto_deps)?;
        let creation_order = merge_creation_order(
            &auto_order,
            &self.config.overrides.creation_order,
        );

        tracing::info!("Resource creation order: {:?}", creation_order);

        // 3. Load fixture overrides
        let fixtures = self.config.load_fixtures()?;

        // 4. Generate or load resources for each type
        let mut resources: HashMap<String, serde_json::Value> = HashMap::new();
        for resource_type in &creation_order {
            if let Some(fixture) = fixtures.get(resource_type) {
                tracing::info!("Using fixture for {}", resource_type);
                resources.insert(resource_type.clone(), fixture.clone());
            } else {
                // Find the profile for this resource type
                let profile = pkg
                    .structure_definitions
                    .iter()
                    .find(|sd| sd.base_type == *resource_type);
                if let Some(profile) = profile {
                    let generated = generate_resource(profile)?;
                    tracing::info!("Generated resource for {}", resource_type);
                    resources.insert(resource_type.clone(), generated);
                } else {
                    tracing::warn!(
                        "No profile found for {}, skipping resource generation",
                        resource_type
                    );
                }
            }
        }

        // 5. Generate test plan
        let mut plan = generate_test_plan(
            cs,
            &pkg.structure_definitions,
            &pkg.search_parameters,
            Some(&pkg.operation_definitions),
            None,
        );
        plan.creation_order = creation_order.clone();

        tracing::info!(
            "Generated test plan with {} test groups, {} total tests",
            plan.test_groups.len(),
            plan.total_tests()
        );

        // 6. Execute: create setup resources, run tests
        let executor = TestExecutor::new(&self.config.server)?;
        let mut created_ids: HashMap<String, String> = HashMap::new();
        let mut resource_field_values: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut results = Vec::new();

        // Create resources in dependency order
        println!("\n── Setup: creating resources ──");
        for resource_type in &creation_order {
            if let Some(body) = resources.get(resource_type) {
                let mut body = body.clone();
                resolve_references(&mut body, &created_ids);

                // Extract field values BEFORE creating (so we know what we sent)
                let field_values = extract_field_values(resource_type, &body);
                resource_field_values.insert(resource_type.clone(), field_values);

                print!("  POST {}/{} ... ", self.config.server.base_url, resource_type);
                match executor.create_resource(resource_type, &body).await {
                    Ok((id, _)) => {
                        println!("→ {}/{}", resource_type, id);
                        created_ids.insert(resource_type.clone(), id);
                    }
                    Err(e) => {
                        println!("✗ {}", e);
                        // Continue with other resources
                    }
                }
            }
        }

        // Run test cases
        println!("\n── Running {} test cases ──", plan.total_tests());
        for group in &plan.test_groups {
            println!("\n── {} ──", group.resource_type);
            for test in &group.tests {
                let mut test = test.clone();

                // Replace {id} placeholders in URLs with actual created IDs
                if let Some(id) = created_ids.get(&test.resource_type) {
                    test.request.url = test.request.url.replace("{id}", id);
                }

                // Resolve search parameter values from created resources
                // and replace sentinel values with actual values
                if test.request.url.contains('?') || test.request.url.contains('&') {
                    let fields = resource_field_values.get(&test.resource_type);
                    test.request.url = resolve_url_params(
                        &test.request.url,
                        &test.resource_type,
                        fields,
                        &created_ids,
                    );
                }

                // Populate response assertion field_values from created resources
                if let Some(ref mut assertion) = test.validation.response_assertion {
                    if let Some(fields) = resource_field_values.get(&test.resource_type) {
                        // Add key field values for response validation
                        let mut type_fields: HashMap<String, serde_json::Value> = HashMap::new();
                        for (path, value) in fields.iter() {
                            // Only add top-level fields for response validation
                            if path.matches('.').count() <= 2 {
                                type_fields.insert(path.clone(), serde_json::Value::String(value.clone()));
                            }
                        }
                        assertion.field_values.insert(test.resource_type.clone(), type_fields);
                    }
                }

                match executor.execute_test(&test).await {
                    Ok(mut result) => {
                        let status_icon = if result.status_code >= 200 && result.status_code < 300 { "→" } else { "✗" };
                        println!("  {} {} {} [{}]", status_icon, test.request.method, test.request.url, result.status_code);
                        // Profile validation
                        if let Some(profile_url) = &test.validation.profile_url {
                            if let Some(response_body) = &result.response_body {
                                if let Some(profile) = pkg
                                    .structure_definitions
                                    .iter()
                                    .find(|sd| &sd.url == profile_url)
                                {
                                    let errors =
                                        validate_against_profile(response_body, profile);
                                    result.validation_errors.extend(errors);
                                }
                            }
                        }

                        // Response assertion validation
                        if let Some(assertion) = &test.validation.response_assertion {
                            let errors = assert_response(
                                assertion,
                                result.status_code,
                                &result.response_body,
                            );
                            result.validation_errors.extend(errors);
                        }

                        result.passed = result.passed && result.validation_errors.is_empty();
                        results.push(result);
                    }
                    Err(e) => {
                        println!("  ✗ {} {} — {}", test.request.method, test.request.url, e);
                        results.push(TestResult {
                            test_name: test.name,
                            passed: false,
                            status_code: 0,
                            response_body: None,
                            validation_errors: vec![format!("Request failed: {}", e)],
                            request_url: format!("{}{}", self.config.server.base_url, test.request.url),
                            request_method: test.request.method.clone(),
                            request_body: test.request.body.clone(),
                        });
                    }
                }
            }
        }

        // 7. Cleanup: delete created resources in reverse order
        println!("\n── Cleanup: deleting resources ──");
        for resource_type in creation_order.iter().rev() {
            if let Some(id) = created_ids.get(resource_type) {
                print!("  DELETE {}/{} ... ", resource_type, id);
                if let Err(e) = executor.delete_resource(resource_type, id).await {
                    println!("✗ {}", e);
                } else {
                    println!("→ deleted");
                }
            }
        }

        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = total - passed;

        Ok(RunReport {
            total,
            passed,
            failed,
            results,
        })
    }
}

/// Walk the JSON and replace "reference": "placeholder:ResourceType" with actual IDs.
fn resolve_references(body: &mut serde_json::Value, created_ids: &HashMap<String, String>) {
    match body {
        serde_json::Value::Object(obj) => {
            for (key, value) in obj.iter_mut() {
                if key == "reference" {
                    if let Some(s) = value.as_str() {
                        if let Some(rest) = s.strip_prefix("placeholder:") {
                            if let Some(id) = created_ids.get(rest) {
                                *value = serde_json::Value::String(format!("{}/{}", rest, id));
                            }
                        }
                    }
                } else {
                    resolve_references(value, created_ids);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                resolve_references(item, created_ids);
            }
        }
        _ => {}
    }
}

/// Resolve sentinel search values in URLs with actual values from created resources.
/// Replaces patterns like `?name=test-value` with `?name=GeneratedFamily`
/// and `?subject=Patient/test-id` with `?subject=Patient/actual-id`.
fn resolve_url_params(
    url: &str,
    resource_type: &str,
    field_values: Option<&HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> String {
    let mut result = url.to_string();

    // Replace reference-style sentinel values: ResourceType/test-id → ResourceType/actual-id
    for (rt, id) in created_ids {
        let sentinel = format!("{}/test-id", rt);
        let actual = format!("{}/{}", rt, id);
        result = result.replace(&sentinel, &actual);
    }

    // Replace field-based sentinel values if we have field values
    if let Some(fields) = field_values {
        // Common sentinel values from the planner's sample_value() function
        let replacements = [
            ("test-value", "string"),
            ("test-code", "token"),
            ("1", "number"),
            ("2024-01-01", "date"),
            ("2024-01-01T00:00:00Z", "dateTime"),
            ("5.0||http://unitsofmeasure.org|kg", "quantity"),
            ("http://example.org", "uri"),
        ];

        for (_sentinel, _param_type) in &replacements {
            // Only replace if we can find a matching field value
            // Check common search param → field path mappings
            let param_mappings: Vec<(&str, &str)> = vec![
                ("name", "name[0].family"),
                ("family", "name[0].family"),
                ("given", "name[0].given[0]"),
                ("identifier", "identifier[0].value"),
                ("gender", "gender"),
                ("birthdate", "birthDate"),
                ("active", "active"),
                ("status", "status"),
                ("telecom", "telecom[0].value"),
                ("phone", "telecom[0].value"),
                ("email", "telecom[0].value"),
                ("city", "address[0].city"),
                ("state", "address[0].state"),
                ("postalCode", "address[0].postalCode"),
                ("country", "address[0].country"),
                ("code", "code.coding[0].code"),
                ("type", "type[0].coding[0].code"),
            ];

            for (param, field_suffix) in &param_mappings {
                let path = format!("{}.{}", resource_type, field_suffix);
                if let Some(actual_value) = fields.get(&path) {
                    let sentinel_for_param = match *param {
                        "name" | "family" | "given" => "test-value",
                        "identifier" => "test-value",
                        "gender" | "status" | "active" => "test-value",
                        "telecom" | "phone" | "email" => "test-value",
                        "city" | "state" | "postalCode" | "country" => "test-value",
                        "code" | "type" => "test-code",
                        "birthdate" => "2024-01-01",
                        _ => "test-value",
                    };
                    let pattern = format!("{}={}", param, sentinel_for_param);
                    let replacement = format!("{}={}", param, actual_value);
                    result = result.replace(&pattern, &replacement);
                }
            }
        }
    }

    result
}