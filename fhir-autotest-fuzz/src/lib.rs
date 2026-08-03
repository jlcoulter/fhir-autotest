pub mod mutators;
pub mod reporter;
pub mod runner;

use crate::mutators::Mutator;
use crate::reporter::FuzzReport;
use crate::runner::FuzzRunner;
use fhir_autotest::model::profile::StructureDefinition;
use std::path::Path;

/// The main fuzzer orchestrator.
///
/// 1. Generates a valid FHIR resource from each profile
/// 2. Applies registered mutation strategies to produce fuzzed variants
/// 3. Sends each variant to the target server
/// 4. Records and classifies the server's response
pub struct Fuzzer {
    target_url: String,
    output_dir: String,
    iterations: usize,
    seed: u64,
    concurrency: usize,
    dry_run: bool,
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
    ) -> Self {
        Self {
            target_url: target_url.to_string(),
            output_dir: output_dir.to_string(),
            iterations,
            seed,
            concurrency,
            dry_run,
            mutators: Vec::new(),
        }
    }

    pub fn register_mutator(&mut self, mutator: Box<dyn Mutator>) {
        self.mutators.push(mutator);
    }

    pub async fn run(&self, profiles: &[StructureDefinition]) -> anyhow::Result<FuzzReport> {
        let output_path = Path::new(&self.output_dir);
        std::fs::create_dir_all(output_path)?;

        let runner = FuzzRunner::new(
            &self.target_url,
            self.concurrency,
            self.dry_run,
        );

        let mut report = FuzzReport::new();
        let mut rng: u64 = if self.seed == 0 {
            rand::random()
        } else {
            self.seed
        };

        for profile in profiles {
            // Generate a valid base resource from the profile
            let base_resource = match fhir_autotest::generate::resource_generator::generate_resource(
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
                "Fuzzing profile {} (type: {})",
                profile.name,
                profile.base_type
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
