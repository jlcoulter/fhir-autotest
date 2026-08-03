use crate::config::BenchConfig;
use crate::report::{BenchReport, BenchSample};
use anyhow::Result;
use chrono::Utc;
use fhir_autotest::config::models::{TestConfig, WriteEndpoint};
use fhir_autotest::generate::model::TestPlan;
use indicatif::{ProgressBar, ProgressStyle};
use rand::Rng;
use rand::SeedableRng;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// The benchmark runner orchestrates data setup, test execution, and reporting.
pub struct BenchRunner {
    bench_config: BenchConfig,
    test_config: TestConfig,
    plan: TestPlan,
    client: reqwest::Client,
    /// IDs uploaded during data ensure, keyed by resource type.
    uploaded_ids: HashMap<String, Vec<String>>,
    /// Order in which resources were uploaded (for reverse-order cleanup).
    upload_order: Vec<String>,
    /// The write endpoint used for upload (reused for cleanup).
    write_endpoint: WriteEndpoint,
}

impl BenchRunner {
    /// Create a new runner, loading configs and ensuring data/plan exist.
    pub async fn new(mut bench_config: BenchConfig) -> Result<Self> {
        // 1. Load the project's TestConfig
        let mut test_config = TestConfig::load(&bench_config.config_path)?;

        // 2. If mock mode is enabled, start the mock server and redirect
        if bench_config.mock || test_config.mock {
            let port = if bench_config.mock_port != 0 {
                bench_config.mock_port
            } else {
                test_config.mock_port
            };
            let addr = fhir_autotest::mock_server::start_mock_server(port).await?;
            let mock_url = format!("http://{}/fhir", addr);
            println!("Mock FHIR server running at {}", mock_url);
            test_config.server.base_url = mock_url.clone();
            test_config.repository = None;
            // Also update the bench config so the report reflects the mock URL
            bench_config.mock = true;
        }

        // 3. Build an HTTP client with the server's TLS settings and headers
        let tls = fhir_autotest::config::models::TlsConfig::from_server(&test_config.server);
        let mut client_builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(bench_config.request_timeout_secs))
            .user_agent("fhir-autotest-bench/0.1");
        if !tls.verify {
            client_builder = client_builder.danger_accept_invalid_certs(true);
        }
        if let Some(ca_path) = &tls.ca_cert {
            let cert = std::fs::read(ca_path)?;
            client_builder = client_builder.add_root_certificate(reqwest::Certificate::from_pem(&cert)?);
        }
        let mut default_headers = reqwest::header::HeaderMap::new();
        for (key, value) in &test_config.server.headers {
            if let (Ok(k), Ok(v)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                default_headers.insert(k, v);
            }
        }
        client_builder = client_builder.default_headers(default_headers);
        let client = client_builder.build()?;

        // 4. Ensure data exists (generate + upload if needed)
        let (uploaded_ids, upload_order, write_endpoint) = if !bench_config.skip_data_ensure {
            ensure_data_exists(&test_config).await?
        } else {
            (HashMap::new(), Vec::new(), test_config.write_endpoint())
        };

        // 5. Load or generate the test plan
        let plan = load_or_generate_plan(&test_config, &bench_config).await?;

        Ok(Self {
            bench_config,
            test_config,
            plan,
            client,
            uploaded_ids,
            upload_order,
            write_endpoint,
        })
    }

    /// Run the benchmark and return a report.
    pub async fn run(&self) -> Result<BenchReport> {
        let bench_cfg = &self.bench_config;
        let base_url = self.test_config.server.base_url.trim_end_matches('/').to_string();

        // Filter test groups if configured
        let groups: Vec<_> = if bench_cfg.filter_groups.is_empty() {
            self.plan.test_groups.iter().collect()
        } else {
            self.plan
                .test_groups
                .iter()
                .filter(|g| bench_cfg.filter_groups.contains(&g.resource_type))
                .collect()
        };

        if groups.is_empty() {
            anyhow::bail!("No test groups match the configured filters");
        }

        tracing::info!(
            "Benchmark: {} groups, {} total tests, concurrency={}, duration={}s",
            groups.len(),
            groups.iter().map(|g| g.tests.len()).sum::<usize>(),
            bench_cfg.concurrency,
            bench_cfg.duration_secs,
        );

        // Collect all test cases into a flat list for random selection
        let all_tests: Vec<_> = groups
            .iter()
            .flat_map(|g| {
                let group_name = g.resource_type.clone();
                g.tests.iter().map(move |t| (group_name.clone(), t.clone()))
            })
            .collect();

        if all_tests.is_empty() {
            anyhow::bail!("No test cases available in the selected groups");
        }

        // Shared state
        let samples = Arc::<std::sync::Mutex<Vec<BenchSample>>>::default();
        let semaphore = Arc::new(Semaphore::new(bench_cfg.concurrency));
        let start = Instant::now();
        let duration = bench_cfg.duration();
        let ramp_up = bench_cfg.ramp_up();
        let warmup = bench_cfg.warmup_requests;
        let warmup_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let warmup_count = Arc::new(std::sync::atomic::AtomicU64::new(0));

        // Warm-up phase: send a few requests without recording
        if warmup > 0 {
            tracing::info!("Warm-up: sending {} requests...", warmup);
            let client = self.client.clone();
            let base_url = base_url.clone();
            let tests = all_tests.clone();
            let warmup_done = warmup_done.clone();
            let warmup_count = warmup_count.clone();

            let warmup_sem = Arc::new(Semaphore::new(bench_cfg.concurrency));
            let mut handles = Vec::new();
            for i in 0..warmup {
                let permit = warmup_sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let base_url = base_url.clone();
                let tests = tests.clone();
                let warmup_done = warmup_done.clone();
                let warmup_count = warmup_count.clone();
                let seed = i as u64;
                let h = tokio::spawn(async move {
                    let _permit = permit;
                    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
                    let idx = rng.random_range(0..tests.len());
                    let (_group, test) = &tests[idx];
                    let url = format!("{}{}", base_url, test.request.url);
                    let _ = client.get(&url).send().await;
                    let prev = warmup_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if prev + 1 >= warmup as u64 {
                        warmup_done.store(true, std::sync::atomic::Ordering::Release);
                    }
                });
                handles.push(h);
            }
            // Wait for warmup to complete
            while !warmup_done.load(std::sync::atomic::Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            tracing::info!("Warm-up complete");
        }

        // Main benchmark loop
        let is_oneshot = bench_cfg.duration_secs == 0;
        if is_oneshot {
            tracing::info!(
                "Benchmark running in one-shot mode: {} test cases across {} workers",
                all_tests.len(),
                bench_cfg.concurrency,
            );
        } else {
            tracing::info!(
                "Benchmark running for {}s with {} workers...",
                bench_cfg.duration_secs,
                bench_cfg.concurrency,
            );
        }
        let mut handles = Vec::new();
        let concurrency = bench_cfg.concurrency;

        // Progress bar for duration mode
        let pb = if !is_oneshot {
            let pb = ProgressBar::new(bench_cfg.duration_secs);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}/{duration_precise}] {bar:40.cyan/blue} {pos}/{len}s {msg}")
                    .unwrap()
                    .progress_chars("#>-"),
            );
            pb.set_message("0 req, 0.0 req/s");
            Some(pb)
        } else {
            None
        };

        // Progress bar updater task (duration mode only)
        if let Some(pb) = &pb {
            let pb = pb.clone();
            let samples = samples.clone();
            let start = start;
            let duration = duration;
            let handle = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let elapsed = start.elapsed();
                    if elapsed >= duration {
                        break;
                    }
                    let count = samples.lock().unwrap().len();
                    let secs = elapsed.as_secs_f64().max(0.1);
                    let rate = count as f64 / secs;
                    pb.set_message(format!("{} req, {:.1} req/s", count, rate));
                    pb.inc(1);
                }
                pb.finish_with_message("done");
            });
            handles.push(handle);
        }

        if is_oneshot {
            // One-shot mode: distribute all test cases across workers, each runs once
            let chunk_size = (all_tests.len() + concurrency - 1) / concurrency;
            for worker_id in 0..concurrency {
                let start_idx = worker_id * chunk_size;
                let end_idx = std::cmp::min(start_idx + chunk_size, all_tests.len());
                if start_idx >= all_tests.len() {
                    break;
                }
                let worker_tests: Vec<_> = all_tests[start_idx..end_idx].to_vec();
                let client = self.client.clone();
                let base_url = base_url.clone();
                let samples = samples.clone();

                let handle = tokio::spawn(async move {
                    for (group_name, test) in &worker_tests {
                        let url = format!("{}{}", base_url, test.request.url);
                        let request_start = Instant::now();

                        let result = match test.request.method.as_str() {
                            "GET" => client.get(&url).send().await,
                            "POST" => {
                                let body = test.request.body.as_ref().map(|b| serde_json::to_vec(b));
                                match body {
                                    Some(Ok(bytes)) => client.post(&url).body(bytes).send().await,
                                    _ => client.post(&url).send().await,
                                }
                            }
                            "PUT" => {
                                let body = test.request.body.as_ref().map(|b| serde_json::to_vec(b));
                                match body {
                                    Some(Ok(bytes)) => client.put(&url).body(bytes).send().await,
                                    _ => client.put(&url).send().await,
                                }
                            }
                            "DELETE" => client.delete(&url).send().await,
                            _ => client.get(&url).send().await,
                        };

                        let latency_us = request_start.elapsed().as_micros() as u64;
                        let (status_code, passed) = match &result {
                            Ok(resp) => {
                                let sc = resp.status().as_u16();
                                let ok = sc >= 200 && sc < 500;
                                (sc, ok)
                            }
                            Err(_) => (0, false),
                        };

                        let sample = BenchSample {
                            test_group: group_name.clone(),
                            test_name: test.name.clone(),
                            request_method: test.request.method.clone(),
                            request_url: url,
                            status_code,
                            latency_us,
                            passed,
                            timestamp: Utc::now(),
                        };
                        samples.lock().unwrap().push(sample);
                    }
                });
                handles.push(handle);
            }
        } else {
            // Duration mode: spawn workers that continuously send requests
            for worker_id in 0..concurrency {
                let permit = semaphore.clone().acquire_owned().await.unwrap();
                let client = self.client.clone();
                let base_url = base_url.clone();
                let tests = all_tests.clone();
                let samples = samples.clone();
                let start = start;
                let duration = duration;
                let ramp_up = ramp_up;
                let worker_id = worker_id;

                let handle = tokio::spawn(async move {
                    let _permit = permit;
                    let mut rng = rand::rngs::StdRng::seed_from_u64(worker_id as u64);

                    loop {
                        let elapsed = start.elapsed();
                        if elapsed >= duration {
                            break;
                        }

                        // Ramp-up: gradually increase effective concurrency
                        if ramp_up > Duration::ZERO {
                            let ramp_progress = elapsed.as_secs_f64() / ramp_up.as_secs_f64();
                            let effective_workers = (worker_id as f64 + 1.0) / concurrency as f64;
                            if effective_workers > ramp_progress.min(1.0) {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                continue;
                            }
                        }

                        // Pick a random test case
                        let idx = rng.random_range(0..tests.len());
                        let (group_name, test) = &tests[idx];

                        let url = format!("{}{}", base_url, test.request.url);
                        let request_start = Instant::now();

                        let result = match test.request.method.as_str() {
                            "GET" => client.get(&url).send().await,
                            "POST" => {
                                let body = test.request.body.as_ref().map(|b| serde_json::to_vec(b));
                                match body {
                                    Some(Ok(bytes)) => client.post(&url).body(bytes).send().await,
                                    _ => client.post(&url).send().await,
                                }
                            }
                            "PUT" => {
                                let body = test.request.body.as_ref().map(|b| serde_json::to_vec(b));
                                match body {
                                    Some(Ok(bytes)) => client.put(&url).body(bytes).send().await,
                                    _ => client.put(&url).send().await,
                                }
                            }
                            "DELETE" => client.delete(&url).send().await,
                            _ => client.get(&url).send().await,
                        };

                        let latency_us = request_start.elapsed().as_micros() as u64;
                        let (status_code, passed) = match &result {
                            Ok(resp) => {
                                let sc = resp.status().as_u16();
                                let ok = sc >= 200 && sc < 500; // 5xx is a failure
                                (sc, ok)
                            }
                            Err(_) => (0, false),
                        };

                        let sample = BenchSample {
                            test_group: group_name.clone(),
                            test_name: test.name.clone(),
                            request_method: test.request.method.clone(),
                            request_url: url,
                            status_code,
                            latency_us,
                            passed,
                            timestamp: Utc::now(),
                        };

                        samples.lock().unwrap().push(sample);
                    }
                });

                handles.push(handle);
            }
        }

        // Wait for all workers to finish
        for h in handles {
            h.await.ok();
        }

        let actual_duration = start.elapsed();
        let all_samples = std::mem::take(&mut *samples.lock().unwrap());

        tracing::info!(
            "Benchmark complete: {} requests in {:.1}s",
            all_samples.len(),
            actual_duration.as_secs_f64()
        );

        let report = BenchReport::from_samples(all_samples, actual_duration, bench_cfg.concurrency);

        // Write report
        let output_dir = Path::new(&bench_cfg.output);
        report.write(output_dir)?;

        // Print summary to stdout
        println!("\n{}", report.title);
        println!("  Duration:   {:.1}s", report.duration_secs);
        println!("  Concurrency: {}", report.concurrency);
        println!("  Requests:   {} total, {} passed, {} failed",
            report.total_requests, report.passed, report.failed);
        println!("  Throughput: {:.1} req/s", report.throughput_req_per_sec);
        println!("  Latency:    p50={}  p95={}  p99={}",
            format_duration(report.latency_p50_us),
            format_duration(report.latency_p95_us),
            format_duration(report.latency_p99_us));
        println!("  Report:     {}/", output_dir.display());

        // 6. Cleanup: delete uploaded resources
        if !self.bench_config.skip_cleanup && !self.uploaded_ids.is_empty() {
            let concurrency = match &self.write_endpoint {
                WriteEndpoint::Repository { concurrency, .. }
                | WriteEndpoint::Server { concurrency, .. } => *concurrency,
            };
            let write_url = match &self.write_endpoint {
                WriteEndpoint::Repository { base_url, .. }
                | WriteEndpoint::Server { base_url, .. } => base_url.as_str(),
            };
            println!(
                "\n── Cleanup: deleting {} resource types from {} ──",
                self.uploaded_ids.len(),
                write_url,
            );
            if let Err(e) = fhir_autotest::runner::bulk_loader::delete_all_resources(
                &self.uploaded_ids,
                &self.upload_order,
                &self.write_endpoint,
                concurrency,
            )
            .await
            {
                tracing::warn!("Cleanup encountered errors: {:#}", e);
                println!("  Cleanup completed with errors (see logs)");
            } else {
                println!("  Cleanup complete");
            }
        }

        Ok(report)
    }
}

/// Ensure data exists: generate resources and upload them if needed.
/// Returns (uploaded_ids, upload_order, write_endpoint) for cleanup.
async fn ensure_data_exists(
    test_config: &TestConfig,
) -> Result<(HashMap<String, Vec<String>>, Vec<String>, WriteEndpoint)> {
    let has_bulk_data = !test_config.data_generation.counts.is_empty();

    if !has_bulk_data {
        tracing::info!("No bulk data configured — skipping data ensure step");
        return Ok((HashMap::new(), Vec::new(), test_config.write_endpoint()));
    }

    // Check if data already exists by looking for NDJSON files
    let output_path = Path::new(&test_config.output);
    let data_dir = output_path.join("data");
    let combined_path = data_dir.join("combined.ndjson");

    if combined_path.exists() {
        tracing::info!("Bulk data already exists at {}", data_dir.display());
    }

    // Run the generate + upload pipeline from fhir-autotest
    tracing::info!("Ensuring bulk data exists...");

    // We need a package path
    let package = test_config.package.as_deref().ok_or_else(|| {
        anyhow::anyhow!("No IG package path configured")
    })?;

    // Generate the test plan and resources first
    fhir_autotest::run_generate(package, test_config).await?;

    // Generate bulk data
    let ctx = fhir_autotest::prepare_plan_context(package, test_config).await?;

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

    let _generated_ids = fhir_autotest::generate::bulk_data::generate_bulk_data(
        &test_config.data_generation.counts,
        &profile_urls,
        &ctx.pkg.structure_definitions,
        &ctx.value_set_systems,
        &ctx.pkg.raw_resources,
        output_path,
    )?;

    if test_config.data_generation.generate_only {
        tracing::info!("generate_only = true: NDJSON files left in {}/data/", test_config.output);
        return Ok((HashMap::new(), Vec::new(), test_config.write_endpoint()));
    }

    // Upload the bulk data
    let write_endpoint = test_config.write_endpoint();
    let upload_order = fhir_autotest::generate::bulk_data::bulk_data_creation_order(
        &test_config.data_generation.counts,
    );

    let concurrency = match &write_endpoint {
        fhir_autotest::config::models::WriteEndpoint::Repository { concurrency, .. }
        | fhir_autotest::config::models::WriteEndpoint::Server { concurrency, .. } => *concurrency,
    };

    tracing::info!("Uploading bulk data...");
    let uploaded_ids = fhir_autotest::runner::bulk_loader::upload_ndjson_files(
        &data_dir,
        &upload_order,
        &write_endpoint,
        concurrency,
    )
    .await?;

    tracing::info!("Bulk data upload complete");
    Ok((uploaded_ids, upload_order, write_endpoint))
}

/// Load an existing test plan or generate a new one.
async fn load_or_generate_plan(
    test_config: &TestConfig,
    bench_config: &BenchConfig,
) -> Result<TestPlan> {
    // Check for explicit test plan path
    if let Some(plan_path) = &bench_config.test_plan {
        let content = std::fs::read_to_string(plan_path)?;
        let plan: TestPlan = serde_json::from_str(&content)?;
        tracing::info!("Loaded test plan from {}", plan_path);
        return Ok(plan);
    }

    // Check default location
    let default_path = Path::new(&test_config.output).join("test_plan.json");
    if default_path.exists() {
        let content = std::fs::read_to_string(&default_path)?;
        let plan: TestPlan = serde_json::from_str(&content)?;
        tracing::info!("Loaded test plan from {}", default_path.display());
        return Ok(plan);
    }

    // Generate a new test plan
    let package = test_config.package.as_deref().ok_or_else(|| {
        anyhow::anyhow!("No IG package path configured")
    })?;

    tracing::info!("Generating test plan...");
    fhir_autotest::run_generate(package, test_config).await?;

    // Load the generated plan
    let content = std::fs::read_to_string(&default_path)?;
    let plan: TestPlan = serde_json::from_str(&content)?;
    tracing::info!("Generated test plan with {} groups", plan.test_groups.len());
    Ok(plan)
}

fn format_duration(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    } else if us >= 1_000 {
        format!("{:.1}ms", us as f64 / 1_000.0)
    } else {
        format!("{}μs", us)
    }
}
