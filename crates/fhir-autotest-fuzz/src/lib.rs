pub mod config;
pub mod mutators;
pub mod reporter;
pub mod runner;

use crate::mutators::Mutator;
use crate::mutators::search_param::generate_fuzzed_query_param;
use crate::reporter::FuzzReport;
use crate::runner::FuzzRunner;
use fhir_autotest::model::capability::CapabilityStatement;
use fhir_autotest::model::profile::StructureDefinition;
use std::path::Path;
use tokio::time::sleep;

/// The main fuzzer orchestrator.
///
/// 1. Generates a valid FHIR resource from each profile
/// 2. Applies registered mutation strategies to produce fuzzed variants
/// 3. Sends each variant to the target server via POST, PUT, and GET (search)
/// 4. Records and classifies the server's response
pub struct Fuzzer {
    target_url: String,
    output_dir: String,
    iterations: usize,
    seed: u64,
    concurrency: usize,
    dry_run: bool,
    delay_ms: u64,
    progress_interval: usize,
    mutators: Vec<Box<dyn Mutator>>,
}

impl Fuzzer {
    pub fn new(
        target_url: &str,
        output_dir: &str,
        iterations: usize,
        seed: u64,
        concurrency: usize,
        dry_run: bool,
        delay_ms: u64,
    ) -> Self {
        Self {
            target_url: target_url.to_string(),
            output_dir: output_dir.to_string(),
            iterations,
            seed,
            concurrency,
            dry_run,
            delay_ms,
            progress_interval: 100,
            mutators: Vec::new(),
        }
    }

    pub fn set_progress_interval(&mut self, interval: usize) {
        self.progress_interval = interval;
    }

    pub fn register_mutator(&mut self, mutator: Box<dyn Mutator>) {
        self.mutators.push(mutator);
    }

    /// Print a progress line if the request count has crossed a multiple of the interval.
    fn report_progress(&self, report: &FuzzReport, phase: &str, detail: &str) {
        if report.total > 0 && report.total.is_multiple_of(self.progress_interval) {
            let pct_anomalies = if report.total > 0 {
                (report.anomalies as f64 / report.total as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "  [{:>8}] {} — {} requests, {} anomalies ({:.1}%) — {}",
                phase, detail, report.total, report.anomalies, pct_anomalies, detail
            );
        }
    }

    /// Enforce inter-request delay if configured.
    async fn enforce_delay(&self) {
        if self.delay_ms > 0 {
            sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        }
    }

    pub async fn run(
        &self,
        profiles: &[StructureDefinition],
        capability_statement: Option<&CapabilityStatement>,
    ) -> anyhow::Result<FuzzReport> {
        let output_path = Path::new(&self.output_dir);
        std::fs::create_dir_all(output_path)?;

        let runner = FuzzRunner::new(&self.target_url, self.concurrency, self.dry_run);

        let mut report = FuzzReport::new();
        let mut rng: u64 = if self.seed == 0 {
            rand::random()
        } else {
            self.seed
        };

        // ── Phase 1: POST fuzzing (create with mutated body) ──────────────
        for profile in profiles {
            let base_resource = match fhir_autotest::generate::resource_generator::generate_resource(
                profile, profiles,
            ) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "Skipping profile {}: cannot generate base resource: {}",
                        profile.name,
                        e
                    );
                    continue;
                }
            };

            tracing::info!(
                "Fuzzing POST {} (profile: {})",
                profile.base_type,
                profile.name
            );

            for mutator in &self.mutators {
                let mutator_name = mutator.name();
                report.categories_used.push(mutator_name.to_string());

                for i in 0..self.iterations {
                    rng = rng.wrapping_add(1);
                    let fuzzed = mutator.mutate(&base_resource, profile, rng);

                    let result = runner
                        .send_fuzzed(&profile.base_type, &fuzzed, mutator_name, i)
                        .await;

                    report.total += 1;
                    if result.is_anomaly() {
                        report.anomalies += 1;
                        report.anomaly_details.push(result);
                    }

                    self.report_progress(
                        &report,
                        "POST",
                        &format!("{}/{}", profile.base_type, mutator_name),
                    );
                    self.enforce_delay().await;
                }
            }
        }

        // ── Phase 2: PUT fuzzing (update with mutated body) ───────────────
        for profile in profiles {
            let base_resource = match fhir_autotest::generate::resource_generator::generate_resource(
                profile, profiles,
            ) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "Skipping PUT fuzz for profile {}: cannot generate base: {}",
                        profile.name,
                        e
                    );
                    continue;
                }
            };

            let fake_id = format!("fuzz-put-{}", uuid::Uuid::new_v4());

            tracing::info!(
                "Fuzzing PUT {}/{} (profile: {})",
                profile.base_type,
                fake_id,
                profile.name
            );

            for mutator in &self.mutators {
                let mutator_name = format!("put_{}", mutator.name());
                report.categories_used.push(mutator_name.clone());

                for i in 0..self.iterations {
                    rng = rng.wrapping_add(1);
                    let fuzzed = mutator.mutate(&base_resource, profile, rng);

                    let result = runner
                        .send_fuzzed_put(&profile.base_type, &fake_id, &fuzzed, &mutator_name, i)
                        .await;

                    report.total += 1;
                    if result.is_anomaly() {
                        report.anomalies += 1;
                        report.anomaly_details.push(result);
                    }

                    self.report_progress(
                        &report,
                        "PUT",
                        &format!("{}/{}", profile.base_type, mutator_name),
                    );
                    self.enforce_delay().await;
                }
            }
        }

        // ── Phase 3: Search param fuzzing (GET with fuzzed query params) ──
        if let Some(cs) = capability_statement {
            for rest in &cs.rest {
                for resource in &rest.resource {
                    let rtype = &resource.resource_type;

                    tracing::info!(
                        "Fuzzing search params for {} ({} params)",
                        rtype,
                        resource.search_param.len()
                    );

                    for sp in &resource.search_param {
                        let param_name = &sp.name;
                        let param_type = &sp.param_type;

                        for i in 0..self.iterations {
                            rng = rng.wrapping_add(1);
                            let query = generate_fuzzed_query_param(param_name, param_type, rng);

                            let result = runner
                                .send_fuzzed_search(rtype, &query, "search_param", i)
                                .await;

                            report.total += 1;
                            if result.is_anomaly() {
                                report.anomalies += 1;
                                report.anomaly_details.push(result);
                            }

                            self.report_progress(
                                &report,
                                "SEARCH",
                                &format!("{}?{}", rtype, param_name),
                            );
                            self.enforce_delay().await;
                        }
                    }
                }
            }
        }

        report.categories_used.sort();
        report.categories_used.dedup();

        // Write report
        let report_json = serde_json::to_string_pretty(&report)?;
        std::fs::write(output_path.join("fuzz_report.json"), &report_json)?;

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutators::BoundaryMutator;
    use fhir_autotest::test_helpers::create_test_ig_package;

    /// Start the mock server on a random port and return the base URL.
    async fn setup_mock_server() -> String {
        let app = fhir_autotest::mock_server::create_mock_app();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{}/fhir", addr)
    }

    #[tokio::test]
    async fn fuzzer_run_produces_report_with_all_phases() {
        let base_url = setup_mock_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let output_dir = temp_dir.path().join("fuzz-output");

        // Create the IG package (has Patient + Observation with search params)
        let tgz_data = create_test_ig_package();
        let tgz_path = temp_dir.path().join("test_ig.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        // Parse the package
        let pkg = fhir_autotest::parse_package(tgz_path.to_str().unwrap()).unwrap();
        let profiles = &pkg.structure_definitions;
        let cs = pkg.capability_statements.first();

        // Build the fuzzer with 2 iterations and one mutator
        let mut fuzzer = Fuzzer::new(&base_url, output_dir.to_str().unwrap(), 2, 42, 1, false, 0);
        fuzzer.register_mutator(Box::new(BoundaryMutator));

        // Run
        let report = fuzzer.run(profiles, cs).await.unwrap();

        // Verify report structure
        assert!(report.total > 0, "Should have run at least one request");
        assert!(!report.categories_used.is_empty(), "Should have categories");

        // POST: 2 profiles × 1 mutator × 2 iterations = 4
        // PUT: 2 profiles × 1 mutator (put_boundary) × 2 iterations = 4
        // SEARCH: 2 resource types × 2 params each × 2 iterations = 8
        // Total: 16
        assert_eq!(report.total, 16, "Expected 16 total requests");

        // Categories should include boundary and put_boundary
        assert!(
            report.categories_used.contains(&"boundary".to_string()),
            "Should include boundary category"
        );

        // Report JSON should have been written
        let report_path = output_dir.join("fuzz_report.json");
        assert!(report_path.exists(), "fuzz_report.json should exist");

        // Verify the JSON is valid
        let report_content = std::fs::read_to_string(&report_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&report_content).unwrap();
        assert_eq!(parsed["total"], 16);
    }

    #[tokio::test]
    async fn fuzzer_dry_run_produces_report_with_zero_anomalies() {
        let base_url = setup_mock_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let output_dir = temp_dir.path().join("fuzz-output");

        let tgz_data = create_test_ig_package();
        let tgz_path = temp_dir.path().join("test_ig.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = fhir_autotest::parse_package(tgz_path.to_str().unwrap()).unwrap();
        let profiles = &pkg.structure_definitions;
        let cs = pkg.capability_statements.first();

        // Dry run — no actual requests
        let mut fuzzer = Fuzzer::new(&base_url, output_dir.to_str().unwrap(), 2, 42, 1, true, 0);
        fuzzer.register_mutator(Box::new(BoundaryMutator));

        let report = fuzzer.run(profiles, cs).await.unwrap();

        assert_eq!(report.total, 16, "Dry run should still count requests");
        assert_eq!(report.anomalies, 0, "Dry run should have no anomalies");
        assert!(report.anomaly_details.is_empty());
    }

    #[tokio::test]
    async fn fuzzer_run_without_capability_statement_skips_search_phase() {
        let base_url = setup_mock_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let output_dir = temp_dir.path().join("fuzz-output");

        let tgz_data = create_test_ig_package();
        let tgz_path = temp_dir.path().join("test_ig.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = fhir_autotest::parse_package(tgz_path.to_str().unwrap()).unwrap();
        let profiles = &pkg.structure_definitions;

        // No CapabilityStatement passed — search phase should be skipped
        let mut fuzzer = Fuzzer::new(&base_url, output_dir.to_str().unwrap(), 2, 42, 1, false, 0);
        fuzzer.register_mutator(Box::new(BoundaryMutator));

        let report = fuzzer.run(profiles, None).await.unwrap();

        // POST: 2 profiles × 1 mutator × 2 iterations = 4
        // PUT: 2 profiles × 1 mutator (put_boundary) × 2 iterations = 4
        // SEARCH: skipped (no CS)
        // Total: 8
        assert_eq!(
            report.total, 8,
            "Expected 8 requests without CS (4 POST + 4 PUT)"
        );
    }

    // ── New tests for uncovered paths ────────────────────────────────────

    #[test]
    fn fuzzer_set_progress_interval() {
        let mut fuzzer = Fuzzer::new("http://localhost", "./out", 10, 0, 1, false, 0);
        assert_eq!(fuzzer.progress_interval, 100);
        fuzzer.set_progress_interval(50);
        assert_eq!(fuzzer.progress_interval, 50);
    }

    #[test]
    fn fuzzer_new_defaults() {
        let fuzzer = Fuzzer::new("http://localhost", "./out", 10, 0, 1, false, 0);
        assert_eq!(fuzzer.target_url, "http://localhost");
        assert_eq!(fuzzer.output_dir, "./out");
        assert_eq!(fuzzer.iterations, 10);
        assert_eq!(fuzzer.seed, 0);
        assert_eq!(fuzzer.concurrency, 1);
        assert!(!fuzzer.dry_run);
        assert_eq!(fuzzer.delay_ms, 0);
        assert_eq!(fuzzer.progress_interval, 100);
        assert!(fuzzer.mutators.is_empty());
    }

    #[test]
    fn fuzzer_register_mutator() {
        let mut fuzzer = Fuzzer::new("http://localhost", "./out", 10, 0, 1, false, 0);
        assert!(fuzzer.mutators.is_empty());
        fuzzer.register_mutator(Box::new(BoundaryMutator));
        assert_eq!(fuzzer.mutators.len(), 1);
        assert_eq!(fuzzer.mutators[0].name(), "boundary");
    }

    #[test]
    fn report_progress_triggers_at_interval() {
        let fuzzer = Fuzzer::new("http://localhost", "./out", 10, 0, 1, false, 0);
        let mut report = FuzzReport::new();
        report.total = 100;
        report.anomalies = 5;
        // progress_interval is 100, total=100, so 100 % 100 == 0 -> triggers
        // This just tests it doesn't panic
        fuzzer.report_progress(&report, "POST", "Patient/boundary");
    }

    #[test]
    fn report_progress_does_not_trigger_below_interval() {
        let fuzzer = Fuzzer::new("http://localhost", "./out", 10, 0, 1, false, 0);
        let mut report = FuzzReport::new();
        report.total = 50;
        // 50 % 100 != 0, should not trigger
        fuzzer.report_progress(&report, "POST", "Patient/boundary");
    }

    #[test]
    fn report_progress_zero_total() {
        let fuzzer = Fuzzer::new("http://localhost", "./out", 10, 0, 1, false, 0);
        let report = FuzzReport::new();
        // total=0, should not trigger
        fuzzer.report_progress(&report, "POST", "Patient/boundary");
    }

    #[tokio::test]
    async fn enforce_delay_zero_is_noop() {
        let fuzzer = Fuzzer::new("http://localhost", "./out", 10, 0, 1, false, 0);
        // delay_ms=0, should return immediately
        fuzzer.enforce_delay().await;
    }

    #[tokio::test]
    async fn enforce_delay_positive() {
        let fuzzer = Fuzzer::new("http://localhost", "./out", 10, 0, 1, false, 1);
        // delay_ms=1, should sleep briefly
        let start = std::time::Instant::now();
        fuzzer.enforce_delay().await;
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() >= 1);
    }

    #[tokio::test]
    async fn fuzzer_run_with_seed_zero_uses_random() {
        let base_url = setup_mock_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let output_dir = temp_dir.path().join("fuzz-output");

        let tgz_data = create_test_ig_package();
        let tgz_path = temp_dir.path().join("test_ig.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = fhir_autotest::parse_package(tgz_path.to_str().unwrap()).unwrap();
        let profiles = &pkg.structure_definitions;
        let cs = pkg.capability_statements.first();

        // seed=0 should use random seed
        let mut fuzzer = Fuzzer::new(&base_url, output_dir.to_str().unwrap(), 1, 0, 1, false, 0);
        fuzzer.register_mutator(Box::new(BoundaryMutator));

        let report = fuzzer.run(profiles, cs).await.unwrap();
        assert!(report.total > 0);
    }

    #[tokio::test]
    async fn fuzzer_run_with_delay_ms() {
        let base_url = setup_mock_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let output_dir = temp_dir.path().join("fuzz-output");

        let tgz_data = create_test_ig_package();
        let tgz_path = temp_dir.path().join("test_ig.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = fhir_autotest::parse_package(tgz_path.to_str().unwrap()).unwrap();
        let profiles = &pkg.structure_definitions;
        let cs = pkg.capability_statements.first();

        // With delay_ms=1, should still work
        let mut fuzzer = Fuzzer::new(&base_url, output_dir.to_str().unwrap(), 1, 42, 1, false, 1);
        fuzzer.register_mutator(Box::new(BoundaryMutator));

        let report = fuzzer.run(profiles, cs).await.unwrap();
        assert!(report.total > 0);
    }

    #[tokio::test]
    async fn fuzzer_run_with_progress_interval() {
        let base_url = setup_mock_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let output_dir = temp_dir.path().join("fuzz-output");

        let tgz_data = create_test_ig_package();
        let tgz_path = temp_dir.path().join("test_ig.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = fhir_autotest::parse_package(tgz_path.to_str().unwrap()).unwrap();
        let profiles = &pkg.structure_definitions;
        let cs = pkg.capability_statements.first();

        // Set progress interval to 1 so it triggers on every request
        let mut fuzzer = Fuzzer::new(&base_url, output_dir.to_str().unwrap(), 1, 42, 1, false, 0);
        fuzzer.set_progress_interval(1);
        fuzzer.register_mutator(Box::new(BoundaryMutator));

        let report = fuzzer.run(profiles, cs).await.unwrap();
        assert!(report.total > 0);
    }

    #[tokio::test]
    async fn fuzzer_run_with_multiple_mutators() {
        let base_url = setup_mock_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let output_dir = temp_dir.path().join("fuzz-output");

        let tgz_data = create_test_ig_package();
        let tgz_path = temp_dir.path().join("test_ig.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = fhir_autotest::parse_package(tgz_path.to_str().unwrap()).unwrap();
        let profiles = &pkg.structure_definitions;
        let cs = pkg.capability_statements.first();

        // Register two mutators
        let mut fuzzer = Fuzzer::new(&base_url, output_dir.to_str().unwrap(), 1, 42, 1, false, 0);
        fuzzer.register_mutator(Box::new(BoundaryMutator));
        fuzzer.register_mutator(Box::new(crate::mutators::TypeMismatchMutator));

        let report = fuzzer.run(profiles, cs).await.unwrap();
        // POST: 2 profiles × 2 mutators × 1 iteration = 4
        // PUT: 2 profiles × 2 mutators (put_boundary, put_type_mismatch) × 1 iteration = 4
        // SEARCH: 2 resource types × 2 params × 1 iteration = 4
        // Total: 12
        assert_eq!(report.total, 12);
        assert!(report.categories_used.contains(&"boundary".to_string()));
        assert!(
            report
                .categories_used
                .contains(&"type_mismatch".to_string())
        );
    }

    #[tokio::test]
    async fn fuzzer_run_with_unknown_mutation_category_handled_by_main() {
        // This tests that the fuzzer itself doesn't validate mutation names
        // (that's done in main.rs). The fuzzer just runs whatever mutators
        // are registered.
        let base_url = setup_mock_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let output_dir = temp_dir.path().join("fuzz-output");

        let tgz_data = create_test_ig_package();
        let tgz_path = temp_dir.path().join("test_ig.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = fhir_autotest::parse_package(tgz_path.to_str().unwrap()).unwrap();
        let profiles = &pkg.structure_definitions;

        // No mutators registered — should still run (just no POST/PUT phases)
        let fuzzer = Fuzzer::new(&base_url, output_dir.to_str().unwrap(), 1, 42, 1, false, 0);

        let report = fuzzer.run(profiles, None).await.unwrap();
        // No mutators, so POST and PUT phases produce 0 requests
        // SEARCH skipped (no CS)
        // Total: 0
        assert_eq!(report.total, 0);
    }
}
