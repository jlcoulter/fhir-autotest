use crate::parse::*;
use crate::generate::*;
use crate::runner::executor::*;
use crate::runner::validator::*;
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
#[derive(Debug)]
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

        let cs = pkg
            .capability_statements
            .first()
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
        let mut results = Vec::new();

        // Create resources in dependency order
        for resource_type in &creation_order {
            if let Some(body) = resources.get(resource_type) {
                let mut body = body.clone();
                resolve_references(&mut body, &created_ids);

                match executor.create_resource(resource_type, &body).await {
                    Ok((id, _)) => {
                        tracing::info!("Created {}/{}", resource_type, id);
                        created_ids.insert(resource_type.clone(), id);
                    }
                    Err(e) => {
                        tracing::error!("Failed to create {}: {}", resource_type, e);
                        // Continue with other resources
                    }
                }
            }
        }

        // Run test cases
        for group in &plan.test_groups {
            for test in &group.tests {
                let mut test = test.clone();

                // Replace {id} placeholders in URLs with actual created IDs
                if let Some(id) = created_ids.get(&test.resource_type) {
                    test.request.url = test.request.url.replace("{id}", id);
                }

                match executor.execute_test(&test).await {
                    Ok(mut result) => {
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

                        result.passed = result.passed && result.validation_errors.is_empty();
                        results.push(result);
                    }
                    Err(e) => {
                        results.push(TestResult {
                            test_name: test.name,
                            passed: false,
                            status_code: 0,
                            response_body: None,
                            validation_errors: vec![format!("Request failed: {}", e)],
                        });
                    }
                }
            }
        }

        // 7. Cleanup: delete created resources in reverse order
        for resource_type in creation_order.iter().rev() {
            if let Some(id) = created_ids.get(resource_type) {
                if let Err(e) = executor.delete_resource(resource_type, id).await {
                    tracing::warn!("Failed to delete {}/{}: {}", resource_type, id, e);
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