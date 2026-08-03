use clap::Parser;

#[derive(Parser)]
#[command(name = "fhir-autotest-fuzz")]
#[command(about = "FHIR REST API fuzzer — context-aware mutation testing for FHIR servers")]
#[command(version)]
struct Cli {
    /// Path to the IG package (.tgz) whose profiles drive mutation generation.
    #[arg(short, long)]
    package: String,

    /// Base URL of the FHIR server to fuzz.
    #[arg(short, long)]
    target: String,

    /// Path to a config file (TOML). Overrides individual flags when set.
    #[arg(short, long)]
    config: Option<String>,

    /// Output directory for fuzz results (default: ./fuzz-output).
    #[arg(short, long, default_value = "./fuzz-output")]
    output: String,

    /// Number of fuzz iterations per resource type (default: 100).
    #[arg(short, long, default_value_t = 100)]
    iterations: usize,

    /// Comma-separated list of mutation categories to apply.
    /// Options: boundary, type_mismatch, cardinality, encoding, all
    #[arg(long, default_value = "all")]
    mutations: String,

    /// Seed for deterministic fuzzing (0 = random).
    #[arg(long, default_value_t = 0)]
    seed: u64,

    /// Concurrency level for parallel fuzzing (default: 4).
    #[arg(long, default_value_t = 4)]
    concurrency: usize,

    /// Dry-run: print what would be sent without executing.
    #[arg(long)]
    dry_run: bool,

    /// Use the built-in mock FHIR server instead of --target.
    #[arg(long)]
    mock: bool,

    /// Port for the mock server (default: 0 = random).
    #[arg(long, default_value_t = 0)]
    mock_port: u16,
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

    // Resolve target URL
    let target_url = if cli.mock {
        let addr = fhir_autotest::mock_server::start_mock_server(cli.mock_port).await?;
        let url = format!("http://{}/fhir", addr);
        println!("Mock FHIR server running at {}", url);
        url
    } else {
        cli.target.clone()
    };

    // Parse mutation categories
    let categories: Vec<&str> = if cli.mutations == "all" {
        vec!["boundary", "type_mismatch", "cardinality", "encoding"]
    } else {
        cli.mutations.split(',').map(|s| s.trim()).collect()
    };

    // Parse the IG package to get profiles and structure definitions
    println!("Parsing IG package: {}", cli.package);
    let pkg = fhir_autotest::parse_package(&cli.package)?;
    let profiles = &pkg.structure_definitions;

    println!(
        "Loaded {} StructureDefinitions from package",
        profiles.len()
    );

    // Build the fuzzer
    let mut fuzzer = fhir_autotest_fuzz::Fuzzer::new(
        &target_url,
        &cli.output,
        cli.iterations,
        cli.seed,
        cli.concurrency,
        cli.dry_run,
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

    let report = fuzzer.run(profiles).await?;

    // Print summary
    println!("\n=== Fuzz Results ===");
    println!("Total requests: {}", report.total);
    println!("Anomalies found: {}", report.anomalies);
    println!("Categories: {}", report.categories_used.join(", "));
    println!("Output: {}", cli.output);

    Ok(())
}
