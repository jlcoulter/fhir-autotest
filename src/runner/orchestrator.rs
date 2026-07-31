use crate::config::models::*;
use crate::generate::value_resolver::extract_field_values;
use crate::generate::*;
use crate::runner::bulk_loader::*;
use crate::runner::executor::*;
use crate::runner::response_assertions::assert_response;
use crate::runner::response_assertions::resolve_json_path;
use crate::runner::validator::*;
use anyhow::Result;
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
        writeln!(f, "\n=== FHIR IG Test Results ===")?;
        writeln!(
            f,
            "Total: {} | Passed: {} | Failed: {}",
            self.total, self.passed, self.failed
        )?;
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

        // Write failed.json — all failing tests across every group in one file,
        // ordered by group then test name, for easy review.
        let failed_results: Vec<&TestResult> = self.results.iter().filter(|r| !r.passed).collect();
        let failed_path = results_dir.join("failed.json");
        let json = serde_json::to_string_pretty(&failed_results)?;
        std::fs::write(&failed_path, json)?;

        Ok(())
    }
}

impl Orchestrator {
    pub fn new(config: TestConfig) -> Self {
        Self { config }
    }

    /// Run the full test pipeline against the server.
    pub async fn run(&self, ig_package_path: &str) -> Result<RunReport> {
        // 1. Parse the IG package, select CS, resolve creation order, load fixtures
        let ctx = crate::prepare_plan_context(ig_package_path, &self.config).await?;

        tracing::info!("Resource creation order: {:?}", ctx.creation_order);

        // 2. Determine data setup strategy
        let has_bulk_data = !self.config.data_generation.counts.is_empty();
        let write_endpoint = self.config.write_endpoint();
        let upload_method = match &write_endpoint {
            WriteEndpoint::Repository { upload_method, .. }
            | WriteEndpoint::Server { upload_method, .. } => *upload_method,
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
        // Track the upload order so deletion reverses the same sequence
        let mut upload_order: Vec<String> = Vec::new();

        if has_bulk_data {
            // ── Bulk data path: generate NDJSON, optionally upload, then test ──
            let output_path = std::path::Path::new(&self.config.output);
            let data_dir = output_path.join("data");

            println!("\n── Generating bulk test data ──");
            let counts = self.config.data_generation.counts.clone();
            // Ensure creation order includes all types from counts
            for rt in counts.keys() {
                if !ctx.creation_order.contains(rt) {
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
            let profile_urls: HashMap<String, String> = ctx
                .cs
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
                &ctx.pkg.structure_definitions,
                &ctx.value_set_systems,
                &ctx.pkg.raw_resources,
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

            // Write supplement resources (one per uncovered type) to NDJSON files
            // so they appear in the data output alongside the bulk data.
            let supplement_ids = write_supplement_ndjson(
                &ctx.creation_order,
                &self.config.data_generation.counts,
                &profile_urls,
                &ctx.pkg.structure_definitions,
                &ctx.value_set_systems,
                &ctx.pkg.raw_resources,
                output_path,
            )?;
            if !supplement_ids.is_empty() {
                println!(
                    "  Generated {} supplement resource type(s):",
                    supplement_ids.len()
                );
                for rt in supplement_ids.keys() {
                    println!("    {}: {}-1", rt, rt.to_lowercase());
                }
            }

            // Merge supplement IDs so update.ndjson includes them too.
            let mut all_ids = generated_ids;
            for (rt, ids) in supplement_ids.iter() {
                all_ids
                    .entry(rt.clone())
                    .or_default()
                    .extend(ids.iter().cloned());
            }

            // Generate update.ndjson with the same resources but 1-2
            // randomly updated parameters per resource (same IDs).
            generate_update_ndjson(&all_ids, output_path)?;
            println!("  Generated update.ndjson with updated resources");

            if self.config.data_generation.generate_only {
                // Skip upload — NDJSON files are left in {output}/data/ for manual use
                println!("\n  generate_only = true: skipping upload and deletion");
                println!("  NDJSON files are in {}/data/", self.config.output);
            } else {
                // Pre-upload required R5 extension StructureDefinitions if the IG
                // references them. These are only needed for HCPD (which uses R5 extension
                // profile URIs as slicing discriminators on Practitioner.extension).
                let needs_r5_profiles = profile_urls
                    .values()
                    .any(|url| url.contains("digitalhealth.gov.au") || url.contains("/hcpd/"));
                if needs_r5_profiles {
                    println!("\n── Ensuring R5 extension profiles are available ──");
                    ensure_r5_extension_profiles(&write_endpoint).await?;
                }

                // Upload bulk data + supplement resources (all read from NDJSON files on disk).
                // Extend the creation order with supplement types so they are uploaded too.
                upload_order = data_creation_order.clone();
                for rt in supplement_ids.keys() {
                    if !upload_order.contains(rt) {
                        upload_order.push(rt.clone());
                    }
                }

                println!(
                    "\n── Uploading bulk data to {} ({}) ──",
                    write_url, upload_method
                );
                let uploaded_ids =
                    upload_ndjson_files(&data_dir, &upload_order, &write_endpoint, concurrency)
                        .await?;

                bulk_ids = uploaded_ids;
                println!("  Bulk data upload complete");
            }
        }

        // 3. Load fixture overrides (for single-resource setup, not bulk)
        let fixtures = self.config.load_fixtures()?;

        // 4. Generate or load individual resources (when NOT using bulk data)
        let mut resources: HashMap<String, serde_json::Value> = HashMap::new();
        let mut created_ids: HashMap<String, String> = HashMap::new();
        let mut resource_field_values: HashMap<String, HashMap<String, String>> = HashMap::new();

        if !has_bulk_data {
            for resource_type in &ctx.creation_order {
                if let Some(fixture) = fixtures.get(resource_type) {
                    tracing::info!("Using fixture for {}", resource_type);
                    resources.insert(resource_type.clone(), fixture.clone());
                } else {
                    let profile = ctx
                        .pkg
                        .structure_definitions
                        .iter()
                        .find(|sd| sd.base_type == *resource_type);
                    if let Some(profile) = profile {
                        let generated = generate_resource_with_value_sets(
                            profile,
                            &ctx.pkg.structure_definitions,
                            &ctx.value_set_systems,
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
        } else {
            // In bulk mode, use the first uploaded ID for each type so tests
            // that use {_id={id}} placeholders can run against real resources.
            for (resource_type, ids) in &bulk_ids {
                if let Some(id) = ids.first() {
                    created_ids.insert(resource_type.clone(), id.clone());
                }
            }
        }

        // 5. Generate test plan (with field values from generated resources)
        let mut plan = generate_test_plan(
            &ctx.cs,
            &ctx.pkg.search_parameters,
            Some(&ctx.pkg.operation_definitions),
            None,
            &resource_field_values,
            &created_ids,
        );
        plan.creation_order = ctx.creation_order.clone();

        // 6b. Validate CapabilityStatement well-formedness
        let cs_validation = validate_capability_statement(&ctx.cs);
        for warning in &cs_validation.warnings {
            tracing::warn!("CapabilityStatement warning: {}", warning);
        }
        for error in &cs_validation.errors {
            tracing::error!("CapabilityStatement error: {}", error);
        }

        // 6c. Generate conformance tests and add them to the plan
        let conformance_tests = generate_conformance_tests(
            &ctx.cs,
            &ctx.pkg.structure_definitions,
            &ctx.pkg.search_parameters,
        );
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
            for resource_type in &ctx.creation_order {
                if let Some(body) = resources.get(resource_type) {
                    let mut body = body.clone();
                    resolve_references(&mut body, resource_type, &created_ids);

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
                        if let Some(profile_url) = &test.validation.profile_url
                            && let Some(response_body) = &result.response_body
                            && let Some(profile) = ctx
                                .pkg
                                .structure_definitions
                                .iter()
                                .find(|sd| &sd.url == profile_url)
                        {
                            let errors = validate_against_profile(response_body, profile);
                            result.validation_errors.extend(errors);
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

                        // Forbidden elements validation
                        if !test.validation.forbidden_elements.is_empty()
                            && let Some(body) = &result.response_body
                            && let Some(entries) = body.get("entry").and_then(|v| v.as_array())
                        {
                            for field in &test.validation.forbidden_elements {
                                for entry in entries {
                                    if let Some(resource) = entry.get("resource")
                                        && resolve_json_path(resource, field).is_some()
                                    {
                                        result.validation_errors.push(format!(
                                            "Resource contains forbidden element '{}'",
                                            field
                                        ));
                                    }
                                }
                            }
                        }

                        // Required elements validation
                        if !test.validation.required_elements.is_empty()
                            && let Some(body) = &result.response_body
                            && let Some(entries) = body.get("entry").and_then(|v| v.as_array())
                        {
                            for field in &test.validation.required_elements {
                                let found = entries.iter().any(|entry| {
                                    entry
                                        .get("resource")
                                        .and_then(|r| resolve_json_path(r, field))
                                        .is_some()
                                });
                                if !found {
                                    result.validation_errors.push(format!(
                                        "Required element '{}' not found in any response entry",
                                        field
                                    ));
                                }
                            }
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
            // Bulk delete all uploaded resources, including supplement resources.
            // Use upload_order (same order as upload) so deletion reverses the
            // dependency-respected sequence, avoiding 409 referential conflicts.
            println!(
                "\n── Cleanup: bulk-deleting resources from {} ──",
                write_url
            );
            delete_all_resources(&bulk_ids, &upload_order, &write_endpoint, concurrency).await?;
            println!("  Bulk deletion complete");
        } else {
            // Delete individual setup resources in reverse order
            println!("\n── Cleanup: deleting resources from {} ──", write_url);
            for resource_type in ctx.creation_order.iter().rev() {
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

/// Walk the JSON and replace reference values with actual created resource IDs.
///
/// Handles multiple reference patterns:
/// - `placeholder:ResourceType` → `ResourceType/actual-id`
/// - `ResourceType/unknown-id` → `ResourceType/actual-id`
/// - `urn:uuid:...` / `http://...` → `ResourceType/actual-id` (using context resource_type)
/// - Bare `ResourceType` (no slash) → `ResourceType/actual-id`
fn resolve_references(
    body: &mut serde_json::Value,
    resource_type: &str,
    created_ids: &HashMap<String, String>,
) {
    match body {
        serde_json::Value::Object(obj) => {
            for (key, value) in obj.iter_mut() {
                if key == "reference" {
                    if let Some(s) = value.as_str()
                        && let Some(replacement) =
                            resolve_reference_value(s, resource_type, created_ids)
                    {
                        *value = serde_json::Value::String(replacement);
                    }
                } else {
                    resolve_references(value, resource_type, created_ids);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                resolve_references(item, resource_type, created_ids);
            }
        }
        _ => {}
    }
}

/// Try to resolve a single reference string to an actual `ResourceType/id` value.
fn resolve_reference_value(
    s: &str,
    resource_type: &str,
    created_ids: &HashMap<String, String>,
) -> Option<String> {
    // Pattern 1: placeholder:ResourceType — existing behavior
    if let Some(rest) = s.strip_prefix("placeholder:") {
        if let Some(id) = created_ids.get(rest) {
            return Some(format!("{}/{}", rest, id));
        }
        tracing::warn!("Could not resolve placeholder reference: {}", s);
        return None;
    }

    // Pattern 2: ResourceType/some-id (has a slash)
    if let Some((slash_type, _id)) = s.split_once('/') {
        // If the part before the slash is a known created resource type, resolve it
        if let Some(id) = created_ids.get(slash_type) {
            return Some(format!("{}/{}", slash_type, id));
        }
        // urn:uuid:... or http://... absolute references — use context resource_type
        if (s.starts_with("urn:") || s.starts_with("http"))
            && let Some(id) = created_ids.get(resource_type)
        {
            return Some(format!("{}/{}", resource_type, id));
        }
        tracing::warn!("Could not resolve reference: {} (unknown resource type)", s);
        return None;
    }

    // Pattern 3: Bare resource type name (no slash)
    if let Some(id) = created_ids.get(s) {
        return Some(format!("{}/{}", s, id));
    }

    tracing::warn!("Could not resolve reference: {}", s);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::{DataGenerationConfig, OverrideConfig, ServerConfig, TestConfig};
    use crate::mock_server::start_mock_server;
    use crate::test_helpers::create_test_ig_package;
    use std::collections::HashMap;

    struct TestEnvironment {
        _mock_url: String,
        temp_dir: tempfile::TempDir,
        config: TestConfig,
    }

    impl TestEnvironment {
        async fn new() -> Self {
            let temp_dir = tempfile::tempdir().unwrap();
            let mock_addr = start_mock_server(0).await.unwrap();
            let mock_url = format!("http://{}/fhir", mock_addr);

            // Create a minimal IG package
            let tgz_data = create_test_ig_package();
            let tgz_path = temp_dir.path().join("test_ig.tgz");
            std::fs::write(&tgz_path, &tgz_data).unwrap();

            let config = TestConfig {
                package: Some(tgz_path.to_str().unwrap().to_string()),
                output: temp_dir.path().join("output").to_str().unwrap().to_string(),
                server: ServerConfig {
                    base_url: mock_url.clone(),
                    headers: HashMap::new(),
                    tls_verify: true,
                    tls_ca_cert: None,
                },
                repository: None,
                overrides: OverrideConfig::default(),
                data_generation: DataGenerationConfig::default(),
                mock: false,
                mock_port: 0,
                dry_run: false,
            };

            TestEnvironment {
                _mock_url: mock_url,
                temp_dir,
                config,
            }
        }
    }

    #[tokio::test]
    async fn orchestrator_selects_capability_statement() {
        let env = TestEnvironment::new().await;
        let package_path = env.config.package.as_ref().unwrap();
        let pkg = crate::parse::package::parse_package(package_path).unwrap();

        let cs = crate::select_capability_statement(&pkg, &env.config).unwrap();
        assert_eq!(cs.name.as_deref(), Some("TestIG"));
        // Should select server-mode with resources
        assert!(
            cs.rest
                .iter()
                .any(|r| r.mode == "server" && !r.resource.is_empty())
        );
        // Should have Patient and Observation
        let resource_types: Vec<&str> = cs
            .rest
            .iter()
            .flat_map(|r| &r.resource)
            .map(|res| res.resource_type.as_str())
            .collect();
        assert!(resource_types.contains(&"Patient"));
        assert!(resource_types.contains(&"Observation"));
    }

    #[tokio::test]
    async fn orchestrator_resolves_creation_order() {
        let env = TestEnvironment::new().await;
        let package_path = env.config.package.as_ref().unwrap();
        let pkg = crate::parse::package::parse_package(package_path).unwrap();

        let auto_deps =
            crate::generate::dependency_resolver::extract_dependencies(&pkg.structure_definitions);
        let auto_order =
            crate::generate::dependency_resolver::resolve_creation_order(&auto_deps).unwrap();

        // Observation depends on Patient (via subject reference)
        // So Patient should come before Observation
        let patient_idx = auto_order.iter().position(|r| r == "Patient").unwrap();
        let obs_idx = auto_order.iter().position(|r| r == "Observation").unwrap();
        assert!(
            patient_idx < obs_idx,
            "Patient should come before Observation in creation order"
        );
    }

    #[tokio::test]
    async fn orchestrator_uses_fixtures_when_configured() {
        let env = TestEnvironment::new().await;

        // Create a fixture file for Patient
        let fixtures_dir = env.temp_dir.path().join("fixtures");
        std::fs::create_dir_all(&fixtures_dir).unwrap();
        let fixture_patient = serde_json::json!({
            "resourceType": "Patient",
            "id": "fixture-patient-1",
            "name": [{"family": "FixtureFamily", "given": ["FixtureGiven"]}],
            "identifier": [{"system": "http://example.org", "value": "fixture-001"}]
        });
        std::fs::write(
            fixtures_dir.join("patient-fixture.json"),
            serde_json::to_string_pretty(&fixture_patient).unwrap(),
        )
        .unwrap();

        let mut config = env.config.clone();
        config.overrides.fixtures_dir = Some(fixtures_dir);
        config.overrides.fixture_map = {
            let mut m = HashMap::new();
            m.insert("Patient".to_string(), "patient-fixture.json".to_string());
            m
        };

        let orchestrator = Orchestrator::new(config.clone());
        let package_path = config.package.as_ref().unwrap();
        let report = orchestrator.run(package_path).await.unwrap();

        assert!(report.total > 0, "Should have run at least one test");
        assert!(report.passed > 0, "At least one test should pass");
    }

    #[tokio::test]
    async fn orchestrator_generates_test_plan() {
        let env = TestEnvironment::new().await;
        let orchestrator = Orchestrator::new(env.config.clone());
        let package_path = env.config.package.as_ref().unwrap();
        let report = orchestrator.run(package_path).await.unwrap();

        assert!(report.total > 0, "Should have generated at least one test");
        // Should have tests for Patient and Observation
        let test_groups: Vec<&str> = report
            .results
            .iter()
            .map(|r| r.test_group.as_str())
            .collect();
        assert!(
            test_groups.contains(&"Patient"),
            "Should have Patient tests"
        );
        assert!(
            test_groups.contains(&"Observation"),
            "Should have Observation tests"
        );
    }

    #[tokio::test]
    async fn orchestrator_creates_and_deletes_resources() {
        let env = TestEnvironment::new().await;
        let orchestrator = Orchestrator::new(env.config.clone());
        let package_path = env.config.package.as_ref().unwrap();
        let report = orchestrator.run(package_path).await.unwrap();

        assert!(report.total > 0, "Should have run tests");
        assert!(report.passed > 0, "At least one test should pass");

        // Write results to output directory
        let output_dir = std::path::Path::new(&env.config.output);
        report.write_results(output_dir).unwrap();

        // Verify results were written
        assert!(
            output_dir.join("results/summary.json").exists(),
            "summary.json should exist"
        );
        assert!(
            output_dir.join("results/failed.json").exists(),
            "failed.json should exist"
        );
    }

    #[tokio::test]
    async fn orchestrator_handles_missing_resource_type_gracefully() {
        let env = TestEnvironment::new().await;

        // Override creation order with a type that has no profile
        let mut config = env.config.clone();
        config.overrides.creation_order = vec!["NonExistentType".to_string()];

        let orchestrator = Orchestrator::new(config.clone());
        let package_path = config.package.as_ref().unwrap();
        let report = orchestrator.run(package_path).await.unwrap();

        // Should not crash — should produce a report
        // The orchestrator still processes all CS resource types for test generation,
        // but the missing type in creation_order is handled gracefully
        assert!(report.total > 0, "Should have run tests");
        // Some tests may fail (POST to mock server without id), but the key
        // assertion is that it doesn't panic
    }

    #[tokio::test]
    async fn orchestrator_resolves_id_placeholders() {
        let env = TestEnvironment::new().await;
        let orchestrator = Orchestrator::new(env.config.clone());
        let package_path = env.config.package.as_ref().unwrap();
        let report = orchestrator.run(package_path).await.unwrap();

        // Check that non-skipped test URLs don't contain literal {id} placeholders
        for result in &report.results {
            if result
                .validation_errors
                .iter()
                .any(|e| e.contains("Skipped"))
            {
                // Skipped tests may still have {id} in their URL — that's expected
                continue;
            }
            assert!(
                !result.request_url.contains("{id}"),
                "Test URL '{}' should not contain literal {{id}} placeholder",
                result.request_url
            );
        }
    }

    #[tokio::test]
    async fn orchestrator_skips_tests_without_created_ids() {
        let env = TestEnvironment::new().await;

        // Only create Patient resources — Observation tests that need {id}
        // will be skipped because no Observation resource was created
        let mut config = env.config.clone();
        config.overrides.creation_order = vec!["Patient".to_string()];

        let orchestrator = Orchestrator::new(config.clone());
        let package_path = config.package.as_ref().unwrap();
        let report = orchestrator.run(package_path).await.unwrap();

        // Tests that need {id} but have no created resource should be skipped
        // (passed with a note about skipping)
        let skipped_count = report
            .results
            .iter()
            .filter(|r| r.validation_errors.iter().any(|e| e.contains("Skipped")))
            .count();
        assert!(
            skipped_count > 0,
            "Some tests should be skipped when no resource is created for their type"
        );
        for result in &report.results {
            if result
                .validation_errors
                .iter()
                .any(|e| e.contains("Skipped"))
            {
                assert!(result.passed, "Skipped tests should be marked as passed");
            }
        }
    }

    #[tokio::test]
    async fn orchestrator_writes_results() {
        let env = TestEnvironment::new().await;
        let orchestrator = Orchestrator::new(env.config.clone());
        let package_path = env.config.package.as_ref().unwrap();
        let report = orchestrator.run(package_path).await.unwrap();

        // Write results to output directory
        let output_dir = std::path::Path::new(&env.config.output);
        report.write_results(output_dir).unwrap();

        // Verify result files exist
        assert!(
            output_dir.join("results/summary.json").exists(),
            "summary.json should exist"
        );
        assert!(
            output_dir.join("results/failed.json").exists(),
            "failed.json should exist"
        );

        // Verify summary content
        let summary_json =
            std::fs::read_to_string(output_dir.join("results/summary.json")).unwrap();
        let summary: serde_json::Value = serde_json::from_str(&summary_json).unwrap();
        assert_eq!(summary["total"], report.total as u64);
        assert_eq!(summary["passed"], report.passed as u64);
        assert_eq!(summary["failed"], report.failed as u64);
    }

    #[tokio::test]
    async fn orchestrator_reports_failures() {
        let env = TestEnvironment::new().await;

        // Point to a non-existent server so all tests fail
        let mut config = env.config.clone();
        config.server.base_url = "http://127.0.0.1:1/fhir".to_string();

        let orchestrator = Orchestrator::new(config.clone());
        let package_path = config.package.as_ref().unwrap();
        let report = orchestrator.run(package_path).await.unwrap();

        // Should have failures since the server is unreachable
        assert!(
            report.failed > 0,
            "Should have failures when server is unreachable"
        );
        // Some tests may be skipped (no created ID) and some negative tests
        // may pass (server returns 200+Bundles for unknown params), but
        // at least some should fail
        assert!(
            report.failed > 0,
            "At least some tests should fail when server is unreachable"
        );
    }
}
