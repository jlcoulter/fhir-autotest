use clap::Parser;
use fhir_autotest::config::models::TestConfig;
use fhir_autotest_bench::BenchRunner;

#[derive(Parser)]
#[command(name = "fhir-autotest-bench")]
#[command(about = "Performance and load testing for FHIR servers using fhir-autotest test plans")]
#[command(version)]
struct Cli {
    /// Path to the project's config.toml (default: ./config.toml).
    #[arg(long, default_value = "./config.toml")]
    config: String,

    /// Override: number of concurrent virtual users.
    #[arg(short = 'c', long)]
    concurrency: Option<usize>,

    /// Override: duration of the benchmark in seconds.
    #[arg(short = 'd', long)]
    duration: Option<u64>,

    /// Override: ramp-up time in seconds.
    #[arg(long)]
    ramp_up: Option<u64>,

    /// Override: output directory for reports.
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// Skip the data-ensure step (assume data already exists).
    #[arg(long)]
    skip_data_ensure: bool,

    /// Skip cleanup after the benchmark.
    #[arg(long)]
    skip_cleanup: bool,

    /// Filter test groups by resource type (can be specified multiple times).
    #[arg(long = "filter", value_name = "RESOURCE_TYPE")]
    filter_groups: Vec<String>,

    /// Path to an existing test_plan.json (skip generation).
    #[arg(long)]
    test_plan: Option<String>,

    /// Number of warm-up requests before recording.
    #[arg(long)]
    warmup: Option<usize>,

    /// Use a built-in mock FHIR server instead of the configured server.
    #[arg(long)]
    mock: bool,

    /// Port for the mock server (default: 0 = random available port).
    #[arg(long)]
    mock_port: Option<u16>,

    /// Benchmark mode: "steady" (default), "max_throughput", or "soak".
    #[arg(long)]
    mode: Option<String>,

    /// Starting concurrency for max-throughput ramp.
    #[arg(long)]
    min_concurrency: Option<usize>,

    /// Maximum concurrency to try before giving up.
    #[arg(long)]
    max_concurrency: Option<usize>,

    /// Concurrency increment per step (max-throughput mode).
    #[arg(long)]
    step_size: Option<usize>,

    /// Seconds to stabilize at each concurrency level (max-throughput mode).
    #[arg(long)]
    stabilization_secs: Option<u64>,

    /// Stop when error rate exceeds this fraction (max-throughput mode).
    #[arg(long)]
    max_error_rate: Option<f64>,

    /// Stop when p95 latency exceeds this value in ms (max-throughput mode).
    #[arg(long)]
    max_latency_p95_ms: Option<u64>,

    /// Duration in hours for soak mode.
    #[arg(long)]
    soak_hours: Option<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load the project's TestConfig (includes [bench] section)
    let mut config = TestConfig::load(&cli.config)?;

    // CLI overrides for mock mode
    if cli.mock {
        config.mock = true;
    }
    if let Some(p) = cli.mock_port {
        config.mock_port = p;
    }

    // CLI overrides for bench settings
    if let Some(c) = cli.concurrency {
        config.bench.concurrency = c;
    }
    if let Some(d) = cli.duration {
        config.bench.duration_secs = d;
    }
    if let Some(r) = cli.ramp_up {
        config.bench.ramp_up_secs = r;
    }
    if let Some(o) = cli.output {
        config.bench.output = o;
    }
    if cli.skip_data_ensure {
        config.bench.skip_data_ensure = true;
    }
    if cli.skip_cleanup {
        config.bench.skip_cleanup = true;
    }
    if !cli.filter_groups.is_empty() {
        config.bench.filter_groups = cli.filter_groups;
    }
    if let Some(tp) = cli.test_plan {
        config.bench.test_plan = Some(tp);
    }
    if let Some(w) = cli.warmup {
        config.bench.warmup_requests = w;
    }

    // CLI overrides for mode
    if let Some(m) = cli.mode {
        config.bench.mode = match m.as_str() {
            "steady" => fhir_autotest::config::models::BenchMode::Steady,
            "max_throughput" => fhir_autotest::config::models::BenchMode::MaxThroughput,
            "soak" => fhir_autotest::config::models::BenchMode::Soak,
            _ => anyhow::bail!(
                "Unknown mode '{}'. Use 'steady', 'max_throughput', or 'soak'.",
                m
            ),
        };
    }
    if let Some(v) = cli.min_concurrency {
        config.bench.min_concurrency = v;
    }
    if let Some(v) = cli.max_concurrency {
        config.bench.max_concurrency = v;
    }
    if let Some(v) = cli.step_size {
        config.bench.step_size = v;
    }
    if let Some(v) = cli.stabilization_secs {
        config.bench.stabilization_secs = v;
    }
    if let Some(v) = cli.max_error_rate {
        config.bench.max_error_rate = v;
    }
    if let Some(v) = cli.max_latency_p95_ms {
        config.bench.max_latency_p95_ms = v;
    }
    if let Some(v) = cli.soak_hours {
        config.bench.soak_hours = v;
    }

    tracing::info!(
        "Benchmark config: mode={:?}, concurrency={}, duration={}s, ramp_up={}s, output={}",
        config.bench.mode,
        config.bench.concurrency,
        config.bench.duration_secs,
        config.bench.ramp_up_secs,
        config.bench.output,
    );

    let runner = BenchRunner::new(config).await?;
    runner.run().await?;

    Ok(())
}
