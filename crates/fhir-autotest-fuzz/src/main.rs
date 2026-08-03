use clap::Parser;

#[derive(Parser)]
#[command(name = "fhir-autotest-fuzz")]
#[command(about = "FHIR REST API fuzzer — context-aware mutation testing for FHIR servers")]
#[command(version)]
struct Cli {
    /// Path to the IG package (.tgz). Overrides `package` in config file.
    #[arg(short, long)]
    package: Option<String>,

    /// Base URL of the FHIR server to fuzz. Overrides `server.base_url` in config file.
    /// Required unless --mock is used or config has a server URL.
    #[arg(short, long)]
    target: Option<String>,

    /// Path to config file (TOML). Defaults to ./config.toml.
    /// Fields: package, server.base_url, mock, mock_port, output, dry_run.
    #[arg(short, long, default_value = "./config.toml")]
    config: String,

    /// Output directory for fuzz results. Overrides `output` in config file.
    #[arg(short, long)]
    output: Option<String>,

    /// Number of fuzz iterations per resource type (default: 100).
    #[arg(short, long, default_value_t = 100)]
    iterations: usize,

    /// Comma-separated list of mutation categories to apply.
    /// Options: boundary, type_mismatch, cardinality, encoding, search_param, all
    #[arg(long, default_value = "all")]
    mutations: String,

    /// Seed for deterministic fuzzing (0 = random).
    #[arg(long, default_value_t = 0)]
    seed: u64,

    /// Concurrency level for parallel fuzzing (default: 4).
    #[arg(long, default_value_t = 4)]
    concurrency: usize,

    /// Dry-run: print what would be sent without executing.
    /// Overrides `dry_run` in config file.
    #[arg(long)]
    dry_run: bool,

    /// Use the built-in mock FHIR server instead of --target or config's server URL.
    /// Overrides `mock` in config file.
    #[arg(long)]
    mock: bool,

    /// Port for the mock server (default: 0 = random).
    /// Overrides `mock_port` in config file.
    #[arg(long, default_value_t = 0)]
    mock_port: u16,

    /// Delay in milliseconds between requests (default: 0 = no delay).
    /// Use to avoid overwhelming the target server.
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,
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

    // Load config file — provides defaults for package, target, mock, output, dry_run
    let config = fhir_autotest::TestConfig::load(&cli.config).ok();

    // Resolve package path: CLI flag wins, then config file
    let package = cli
        .package
        .or_else(|| config.as_ref().and_then(|c| c.package.clone()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No IG package specified. Set 'package' in config file or use --package."
            )
        })?;

    // Resolve mock mode: CLI --mock wins, then config.mock
    let use_mock = cli.mock || config.as_ref().map(|c| c.mock).unwrap_or(false);

    // Resolve mock port: CLI --mock-port wins, then config.mock_port, then 0
    let mock_port = if cli.mock_port != 0 {
        cli.mock_port
    } else {
        config.as_ref().map(|c| c.mock_port).unwrap_or(0)
    };

    // Resolve target URL
    let target_url: String = if use_mock {
        let addr = fhir_autotest::mock_server::start_mock_server(mock_port).await?;
        let url = format!("http://{}/fhir", addr);
        println!("Mock FHIR server running at {}", url);
        url
    } else if let Some(target) = cli.target {
        target
    } else if let Some(cfg) = &config {
        cfg.server.base_url.clone()
    } else {
        anyhow::bail!(
            "No target server. Provide --target <URL>, --mock, or set server.base_url in config."
        );
    };

    // Resolve output directory: CLI flag wins, then config.output, then default
    let output = cli
        .output
        .or_else(|| config.as_ref().map(|c| c.output.clone()))
        .unwrap_or_else(|| "./fuzz-output".to_string());

    // Resolve dry-run: CLI --dry-run wins, then config.dry_run
    let dry_run = cli.dry_run || config.as_ref().map(|c| c.dry_run).unwrap_or(false);

    // Parse mutation categories
    let categories: Vec<&str> = if cli.mutations == "all" {
        vec![
            "boundary",
            "type_mismatch",
            "cardinality",
            "encoding",
            "search_param",
        ]
    } else {
        cli.mutations.split(',').map(|s| s.trim()).collect()
    };

    // Parse the IG package to get profiles, structure definitions, and capability statement
    println!("Parsing IG package: {}", package);
    let pkg = fhir_autotest::parse_package(&package)?;
    let profiles = &pkg.structure_definitions;
    let capability_statement = pkg.capability_statements.first();

    println!(
        "Loaded {} StructureDefinitions, {} CapabilityStatements from package",
        profiles.len(),
        pkg.capability_statements.len()
    );

    // Build the fuzzer
    let mut fuzzer = fhir_autotest_fuzz::Fuzzer::new(
        &target_url,
        &output,
        cli.iterations,
        cli.seed,
        cli.concurrency,
        dry_run,
        cli.delay_ms,
    );

    // Register mutation strategies
    for cat in &categories {
        match *cat {
            "boundary" => fuzzer.register_mutator(Box::new(
                fhir_autotest_fuzz::mutators::BoundaryMutator,
            )),
            "type_mismatch" => fuzzer.register_mutator(Box::new(
                fhir_autotest_fuzz::mutators::TypeMismatchMutator,
            )),
            "cardinality" => fuzzer.register_mutator(Box::new(
                fhir_autotest_fuzz::mutators::CardinalityMutator,
            )),
            "encoding" => fuzzer.register_mutator(Box::new(
                fhir_autotest_fuzz::mutators::EncodingMutator,
            )),
            "search_param" => {
                // search_param is handled directly by the orchestrator, not as a body mutator
            }
            other => {
                anyhow::bail!("Unknown mutation category: {}", other);
            }
        }
    }

    // Run the fuzzer
    println!(
        "Starting fuzz run: {} iterations, {} categories, target: {}",
        cli.iterations,
        categories.len(),
        target_url
    );

    let report = fuzzer.run(profiles, capability_statement).await?;

    // Print summary
    println!("\n=== Fuzz Results ===");
    println!("Total requests: {}", report.total);
    println!("Anomalies found: {}", report.anomalies);
    println!("Categories: {}", report.categories_used.join(", "));
    println!("Output: {}", output);

    Ok(())
}
