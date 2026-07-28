use crate::config::models::*;
use crate::generate::*;
use crate::parse::*;
use crate::runner::bulk_loader::*;
use crate::runner::executor::*;
use crate::runner::response_assertions::assert_response;
use crate::runner::validator::*;
use crate::runner::value_resolver::extract_field_values;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Orchestrates the full test pipeline:
/// 1. Parse IG package
/// 2. Resolve dependencies → creation order
/// 3. Generate or load fixture resources / bulk data
/// 4. Generate test plan from CapabilityStatement
/// 5. Create setup resources on the server (or bulk-upload NDJSON)
/// 6. Execute test cases
/// 7. Validate responses against profiles
/// 8. Cleanup (delete created resources / bulk-delete)
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
            // Show the request that was made
            writeln!(f, "  → {} {}", result.request_method, result.request_url)?;
            for err in &result.validation_errors {
                writeln!(f, "  ✗ {}", err)?;
            }
            // For failed tests, show the request and response for debugging
            if !result.passed {
                if let Some(body) = &result.request_body {
                    let body_str = serde_json::to_string(body).unwrap_or_else(|_| body.to_string());
                    let max_body = 500;
                    if body_str.len() > max_body {
                        writeln!(f, "  Request body (truncated):\n{}", &body_str[..max_body])?;
                    } else {
                        writeln!(f, "  Request body:\n{}", body_str)?;
                    }
                }
                if let Some(body) = &result.response_body {
                    let body_str =
                        serde_json::to_string_pretty(body).unwrap_or_else(|_| body.to_string());
                    // Truncate very long responses to keep output readable
                    let max_len = 2000;
                    if body_str.len() > max_len {
                        writeln!(f, "  Response (truncated):\n{}", &body_str[..max_len])?;
                        writeln!(f, "  ... ({} bytes truncated)", body_str.len() - max_len)?;
                    } else {
                        writeln!(f, "  Response:\n{}", body_str)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Per-group summary for the results directory.
#[derive(Debug, Serialize)]
struct GroupSummary {
    group: String,
    total: usize,
    passed: usize,
    failed: usize,
}

/// Overall summary written to `summary.json`.
#[derive(Debug, Serialize)]
struct ResultsSummary {
    total: usize,
    passed: usize,
    failed: usize,
    groups: Vec<GroupSummary>,
}

impl RunReport {
    /// Write per-group result files and a summary into `output_dir/results/`.
    ///
    /// Creates:
    /// - `{output_dir}/results/{group}.json` — full test results for one group
    /// - `{output_dir}/results/summary.json` — totals and per-group pass/fail counts
    pub fn write_results(&self, output_dir: &std::path::Path) -> anyhow::Result<()> {
        let results_dir = output_dir.join("results");
        std::fs::create_dir_all(&results_dir)?;

        // Group results by test_group
        let mut groups: BTreeMap<String, Vec<&TestResult>> = BTreeMap::new();
        for result in &self.results {
            groups
                .entry(result.test_group.clone())
                .or_default()
                .push(result);
        }

        // Write per-group files
        for (group_name, group_results) in &groups {
            let path = results_dir.join(format!("{}.json", group_name));
            let json = serde_json::to_string_pretty(&group_results)?;
            std::fs::write(&path, json)?;
        }

        // Build and write summary
        let group_summaries: Vec<GroupSummary> = groups
            .iter()
            .map(|(name, results)| {
                let passed = results.iter().filter(|r| r.passed).count();
                GroupSummary {
                    group: name.clone(),
                    total: results.len(),
                    passed,
                    failed: results.len() - passed,
                }
            })
            .collect();

        let summary = ResultsSummary {
            total: self.total,
            passed: self.passed,
            failed: self.failed,
            groups: group_summaries,
        };

        let summary_path = results_dir.join("summary.json");
        let json = serde_json::to_string_pretty(&summary)?;
        std::fs::write(&summary_path, json)?;

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
        let value_set_systems = build_value_set_system_map(&pkg.raw_resources);

        // Prefer a server-mode CapabilityStatement; fall back to first if none found
        let cs = pkg
            .capability_statements
            .iter()
            .find(|cs| {
                cs.rest
                    .iter()
                    .any(|r| r.mode == "server" && !r.resource.is_empty())
            })
            .or_else(|| {
                pkg.capability_statements
                    .iter()
                    .find(|cs| cs.rest.iter().any(|r| !r.resource.is_empty()))
            })
            .or(pkg.capability_statements.first())
            .context("No CapabilityStatement found in IG package")?;

        // 2. Extract dependencies and determine creation order
        let auto_deps = extract_dependencies(&pkg.structure_definitions);
        let auto_order = resolve_creation_order(&auto_deps)?;
        let creation_order =
            merge_creation_order(&auto_order, &self.config.overrides.creation_order);

        tracing::info!("Resource creation order: {:?}", creation_order);

        // 3. Determine data setup strategy
        let has_bulk_data = !self.config.data_generation.counts.is_empty();
        let write_endpoint = self.config.write_endpoint();
        let upload_method = match &write_endpoint {
            WriteEndpoint::Repository { upload_method, .. }
            | WriteEndpoint::Server { upload_method, .. } => upload_method.to_uppercase(),
        };
        let concurrency = match &write_endpoint {
            WriteEndpoint::Repository { concurrency, .. }
            | WriteEndpoint::Server { concurrency, .. } => *concurrency,
        };
        let write_url = match &self.config.repository {
            Some(repo) => &repo.base_url,
            None => &self.config.server.base_url,
        };

        // Bulk data IDs for cleanup (used when data_generation is configured)
        let mut bulk_ids: HashMap<String, Vec<String>> = HashMap::new();

        if has_bulk_data {
            // ── Bulk data path: generate NDJSON, optionally upload, then test ──
            let output_path = std::path::Path::new(&self.config.output);
            let data_dir = output_path.join("data");

            println!("\n── Generating bulk test data ──");
            let counts = self.config.data_generation.counts.clone();
            // Ensure creation order includes all types from counts
            for rt in counts.keys() {
                if !creation_order.contains(rt) {
                    tracing::warn!(
                        "Data generation type '{}' not in creation order, appending",
                        rt
                    );
                }
            }

            // Build a map of resource_type → profile URL from the
            // CapabilityStatement so bulk data generation stamps the correct
            // meta.profile instead of hardcoded Plan-Net URLs.
            // Strip FHIR version suffixes (e.g. "|26.0.0") — meta.profile
            // should contain the plain canonical URL for server compatibility.
            let profile_urls: HashMap<String, String> = cs
                .rest
                .iter()
                .flat_map(|r| &r.resource)
                .filter_map(|res| {
                    res.profile.as_ref().map(|p| {
                        let url = p.split('|').next().unwrap_or(p);
                        (res.resource_type.clone(), url.to_string())
                    })
                })
                .collect();

            let generated_ids = generate_bulk_data(
                &counts,
                &profile_urls,
                &pkg.structure_definitions,
                &value_set_systems,
                output_path,
            )?;
            let data_creation_order = bulk_data_creation_order(&counts);
            let total_resources: u64 = counts.values().sum();
            println!(
                "  Generated {} total resources across {} types",
                total_resources,
                counts.len()
            );
            for (rt, ids) in &generated_ids {
                println!("    {}: {} resources", rt, ids.len());
            }

            // Generate update.ndjson with the same resources but 1-2
            // randomly updated parameters per resource (same IDs).
            generate_update_ndjson(&generated_ids, output_path)?;
            println!("  Generated update.ndjson with updated resources");

            if self.config.data_generation.generate_only {
                // Skip upload — NDJSON files are left in {output}/data/ for manual use
                println!("\n  generate_only = true: skipping upload and deletion");
                println!("  NDJSON files are in {}/data/", self.config.output);
            } else {
                // Upload NDJSON files to the repository
                println!(
                    "\n── Uploading bulk data to {} ({}) ──",
                    write_url, upload_method
                );
                let uploaded_ids = upload_ndjson_files(
                    &data_dir,
                    &data_creation_order,
                    &write_endpoint,
                    concurrency,
                )
                .await?;

                bulk_ids = uploaded_ids;
                println!("  Bulk data upload complete");
            }
        }

        // 4. Load fixture overrides (for single-resource setup, not bulk)
        let fixtures = self.config.load_fixtures()?;

        // 5. Generate or load individual resources (when NOT using bulk data)
        let mut resources: HashMap<String, serde_json::Value> = HashMap::new();
        let mut created_ids: HashMap<String, String> = HashMap::new();
        let mut resource_field_values: HashMap<String, HashMap<String, String>> = HashMap::new();

        if !has_bulk_data {
            for resource_type in &creation_order {
                if let Some(fixture) = fixtures.get(resource_type) {
                    tracing::info!("Using fixture for {}", resource_type);
                    resources.insert(resource_type.clone(), fixture.clone());
                } else {
                    let profile = pkg
                        .structure_definitions
                        .iter()
                        .find(|sd| sd.base_type == *resource_type);
                    if let Some(profile) = profile {
                        let generated = generate_resource_with_value_sets(
                            profile,
                            &pkg.structure_definitions,
                            &value_set_systems,
                        )?;
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
        }

        // 6. Generate test plan
        let mut plan = generate_test_plan(
            cs,
            &pkg.structure_definitions,
            &pkg.search_parameters,
            Some(&pkg.operation_definitions),
            None,
        );
        plan.creation_order = creation_order.clone();

        // 6b. Validate CapabilityStatement well-formedness
        let cs_validation = validate_capability_statement(cs);
        for warning in &cs_validation.warnings {
            tracing::warn!("CapabilityStatement warning: {}", warning);
        }
        for error in &cs_validation.errors {
            tracing::error!("CapabilityStatement error: {}", error);
        }

        // 6c. Generate conformance tests and add them to the plan
        let conformance_tests = generate_conformance_tests(cs, &pkg.structure_definitions);
        if !conformance_tests.is_empty() {
            // Convert conformance tests into regular test cases and add to plan
            let mut conformance_group = TestGroup {
                resource_type: "_conformance".to_string(),
                profile_url: None,
                tests: conformance_tests
                    .iter()
                    .map(conformance_test_to_test_case)
                    .collect(),
            };
            // Stamp response assertions
            for test in &mut conformance_group.tests {
                if test.validation.response_assertion.is_none() {
                    test.validation.response_assertion =
                        assertion_for_kind(&test.kind, &test.resource_type);
                }
            }
            plan.test_groups.push(conformance_group);
        }

        tracing::info!(
            "Generated test plan with {} test groups, {} total tests",
            plan.test_groups.len(),
            plan.total_tests()
        );

        // 7. Create setup resources (single-resource path only)
        let executor = TestExecutor::new(
            self.config.server.base_url.clone(),
            self.config.server.headers.clone(),
            write_endpoint.clone(),
        )?;

        if !has_bulk_data {
            println!("\n── Setup: creating resources on {} ──", write_url);
            for resource_type in &creation_order {
                if let Some(body) = resources.get(resource_type) {
                    let mut body = body.clone();
                    resolve_references(&mut body, &created_ids);

                    let field_values = extract_field_values(resource_type, &body);
                    resource_field_values.insert(resource_type.clone(), field_values);

                    print!("  {} {}/{} ... ", upload_method, write_url, resource_type);
                    match executor.create_resource(resource_type, &body).await {
                        Ok((id, _)) => {
                            println!("→ {}/{}", resource_type, id);
                            created_ids.insert(resource_type.clone(), id);
                        }
                        Err(e) => {
                            println!("✗ {}", e);
                        }
                    }
                }
            }
        }

        // 8. Run test cases (GET/search goes to the public FHIR server)
        println!(
            "\n── Running {} test cases against {} ──",
            plan.total_tests(),
            self.config.server.base_url
        );
        let mut results = Vec::new();

        for group in &plan.test_groups {
            println!("\n── {} ──", group.resource_type);
            for test in &group.tests {
                let mut test = test.clone();

                // Skip tests that need an {id} placeholder but no resource was created
                // for this type (e.g. no setup data available). These would send literal
                // {id} in the URL and always fail with 400.
                if test.request.url.contains("{id}")
                    && !created_ids.contains_key(&test.resource_type)
                {
                    println!(
                        "  ⊘ {} — skipped (no created ID for {})",
                        test.name, test.resource_type
                    );
                    results.push(TestResult {
                        test_name: test.name,
                        passed: true, // skipped, not a failure
                        status_code: 0,
                        response_body: None,
                        validation_errors: vec![format!(
                            "Skipped: no resource created for {} — {{id}} placeholder unresolved",
                            test.resource_type
                        )],
                        request_url: format!("{}{}", self.config.server.base_url, test.request.url),
                        request_method: test.request.method.clone(),
                        request_body: test.request.body.clone(),
                        test_group: group.resource_type.clone(),
                    });
                    continue;
                }

                // Replace {id} placeholders in URLs with actual created IDs
                if let Some(id) = created_ids.get(&test.resource_type) {
                    test.request.url = test.request.url.replace("{id}", id);
                }
                if test.request.url.contains("{id}") {
                    println!(
                        "  ⊘ {} {} — skipped: no resource created for {}",
                        test.request.method, test.request.url, test.resource_type
                    );
                    results.push(TestResult {
                        test_name: test.name,
                        passed: false,
                        status_code: 0,
                        response_body: None,
                        validation_errors: vec![format!(
                            "Skipped: {{id}} placeholder unresolved — no resource created for {}",
                            test.resource_type
                        )],
                        request_url: format!("{}{}", self.config.server.base_url, test.request.url),
                        request_method: test.request.method.clone(),
                        request_body: test.request.body.clone(),
                        test_group: group.resource_type.clone(),
                    });
                    continue;
                }

                // Resolve search parameter values from created resources
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
                        let mut type_fields: HashMap<String, serde_json::Value> = HashMap::new();
                        for (path, value) in fields.iter() {
                            if path.matches('.').count() <= 2 {
                                type_fields
                                    .insert(path.clone(), serde_json::Value::String(value.clone()));
                            }
                        }
                        assertion
                            .field_values
                            .insert(test.resource_type.clone(), type_fields);
                    }
                }

                match executor.execute_test(&test).await {
                    Ok(mut result) => {
                        let status_icon = if result.status_code >= 200 && result.status_code < 300 {
                            "→"
                        } else {
                            "✗"
                        };
                        println!(
                            "  {} {} {} [{}]",
                            status_icon, test.request.method, test.request.url, result.status_code
                        );
                        // Profile validation
                        if let Some(profile_url) = &test.validation.profile_url {
                            if let Some(response_body) = &result.response_body {
                                if let Some(profile) = pkg
                                    .structure_definitions
                                    .iter()
                                    .find(|sd| &sd.url == profile_url)
                                {
                                    let errors = validate_against_profile(response_body, profile);
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
                        result.test_group = group.resource_type.clone();
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
                            request_url: format!(
                                "{}{}",
                                self.config.server.base_url, test.request.url
                            ),
                            request_method: test.request.method.clone(),
                            request_body: test.request.body.clone(),
                            test_group: group.resource_type.clone(),
                        });
                    }
                }
            }
        }

        // 9. Cleanup
        if has_bulk_data && !self.config.data_generation.generate_only {
            // Bulk delete all uploaded resources
            let data_creation_order = bulk_data_creation_order(&self.config.data_generation.counts);
            println!(
                "\n── Cleanup: bulk-deleting resources from {} ──",
                write_url
            );
            delete_all_resources(
                &bulk_ids,
                &data_creation_order,
                &write_endpoint,
                concurrency,
            )
            .await?;
            println!("  Bulk deletion complete");
        } else {
            // Delete individual setup resources in reverse order
            println!("\n── Cleanup: deleting resources from {} ──", write_url);
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
