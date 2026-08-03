pub mod mutators;
pub mod reporter;
pub mod runner;

use crate::mutators::search_param::generate_fuzzed_query_param;
use crate::mutators::Mutator;
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
        if report.total > 0 && report.total % self.progress_interval == 0 {
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
            let base_resource =
                match fhir_autotest::generate::resource_generator::generate_resource(
                    profile,
                    profiles,
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
            let base_resource =
                match fhir_autotest::generate::resource_generator::generate_resource(
                    profile,
                    profiles,
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
                        .send_fuzzed_put(
                            &profile.base_type,
                            &fake_id,
                            &fuzzed,
                            &mutator_name,
                            i,
                        )
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
