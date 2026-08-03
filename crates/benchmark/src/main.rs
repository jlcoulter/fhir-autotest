use clap::Parser;
use fhir_autotest_bench::BenchConfig;
use fhir_autotest_bench::BenchRunner;

#[derive(Parser)]
#[command(name = "fhir-autotest-bench")]
#[command(about = "Performance and load testing for FHIR servers using fhir-autotest test plans")]
#[command(version)]
struct Cli {
    /// Path to the benchmark config TOML file.
    #[arg(short, long, default_value = "./bench-config.toml")]
    config: String,

    /// Override: path to the project's config.toml.
    #[arg(long)]
    project_config: Option<String>,

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

    // Load bench config from file, then apply CLI overrides
    let mut bench_config = if std::path::Path::new(&cli.config).exists() {
        BenchConfig::load(&cli.config)?
    } else {
        tracing::info!(
            "Bench config '{}' not found, using defaults",
            cli.config
        );
        BenchConfig::default()
    };

    // CLI overrides
    if let Some(path) = cli.project_config {
        bench_config.config_path = path;
    }
    if let Some(c) = cli.concurrency {
        bench_config.concurrency = c;
    }
    if let Some(d) = cli.duration {
        bench_config.duration_secs = d;
    }
    if let Some(r) = cli.ramp_up {
        bench_config.ramp_up_secs = r;
    }
    if let Some(o) = cli.output {
        bench_config.output = o;
    }
    if cli.skip_data_ensure {
        bench_config.skip_data_ensure = true;
    }
    if cli.skip_cleanup {
        bench_config.skip_cleanup = true;
    }
    if !cli.filter_groups.is_empty() {
        bench_config.filter_groups = cli.filter_groups;
    }
    if let Some(tp) = cli.test_plan {
        bench_config.test_plan = Some(tp);
    }
    if let Some(w) = cli.warmup {
        bench_config.warmup_requests = w;
    }

    tracing::info!(
        "Benchmark config: concurrency={}, duration={}s, ramp_up={}s, output={}",
        bench_config.concurrency,
        bench_config.duration_secs,
        bench_config.ramp_up_secs,
        bench_config.output,
    );

    let runner = BenchRunner::new(bench_config).await?;
    runner.run().await?;

    Ok(())
}
