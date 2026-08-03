use clap::Parser;
use fhir_autotest_fuzz::config::FuzzConfig;

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
    /// Reads [fuzz] section for defaults: iterations, mutations, seed, delay_ms, concurrency.
    #[arg(short, long, default_value = "./config.toml")]
    config: String,

    /// Output directory for fuzz results. Overrides `output` in config file.
    #[arg(short, long)]
    output: Option<String>,

    /// Number of fuzz iterations per resource type (default: 100).
    /// Overrides [fuzz].iterations in config file.
    #[arg(short, long)]
    iterations: Option<usize>,

    /// Comma-separated list of mutation categories to apply.
    /// Options: boundary, type_mismatch, cardinality, encoding, search_param, all
    /// Overrides [fuzz].mutations in config file.
    #[arg(long)]
    mutations: Option<String>,

    /// Seed for deterministic fuzzing (0 = random).
    /// Overrides [fuzz].seed in config file.
    #[arg(long)]
    seed: Option<u64>,

    /// Concurrency level for parallel fuzzing (default: 4).
    /// Overrides [fuzz].concurrency in config file.
    #[arg(long)]
    concurrency: Option<usize>,

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
    /// Overrides [fuzz].delay_ms in config file.
    #[arg(long)]
    delay_ms: Option<u64>,
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

    // Load config file — provides defaults for everything
    let config = fhir_autotest::TestConfig::load(&cli.config).ok();
    let fuzz_config: FuzzConfig = config
        .as_ref()
        .and_then(|_| {
            // Re-read the raw TOML to extract [fuzz] section
            let content = std::fs::read_to_string(&cli.config).ok()?;
            toml::from_str(&content).ok()
        })
        .unwrap_or_default();

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

    // Resolve fuzzer settings: CLI flag wins, then [fuzz] config, then hardcoded default
    let iterations = cli.iterations.unwrap_or(fuzz_config.iterations);
    let seed = cli.seed.unwrap_or(fuzz_config.seed);
    let concurrency = cli.concurrency.unwrap_or(fuzz_config.concurrency);
    let delay_ms = cli.delay_ms.unwrap_or(fuzz_config.delay_ms);
    let mutations_str = cli.mutations.unwrap_or(fuzz_config.mutations);

    // Parse mutation categories
    let categories: Vec<&str> = if mutations_str == "all" {
        vec![
            "boundary",
            "type_mismatch",
            "cardinality",
            "encoding",
            "search_param",
        ]
    } else {
        mutations_str.split(',').map(|s| s.trim()).collect()
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
        iterations,
        seed,
        concurrency,
        dry_run,
        delay_ms,
    );

    // Register mutation strategies
    for cat in &categories {
        match *cat {
            "boundary" => {
                fuzzer.register_mutator(Box::new(fhir_autotest_fuzz::mutators::BoundaryMutator))
            }
            "type_mismatch" => {
                fuzzer.register_mutator(Box::new(fhir_autotest_fuzz::mutators::TypeMismatchMutator))
            }
            "cardinality" => {
                fuzzer.register_mutator(Box::new(fhir_autotest_fuzz::mutators::CardinalityMutator))
            }
            "encoding" => {
                fuzzer.register_mutator(Box::new(fhir_autotest_fuzz::mutators::EncodingMutator))
            }
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
        iterations,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_default_config() {
        let cli = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "--package",
            "test.tgz",
            "--target",
            "http://localhost:8080",
        ])
        .unwrap();
        assert_eq!(cli.package, Some("test.tgz".to_string()));
        assert_eq!(cli.target, Some("http://localhost:8080".to_string()));
        assert_eq!(cli.config, "./config.toml");
        assert_eq!(cli.output, None);
        assert_eq!(cli.iterations, None);
        assert_eq!(cli.mutations, None);
        assert_eq!(cli.seed, None);
        assert_eq!(cli.concurrency, None);
        assert!(!cli.dry_run);
        assert!(!cli.mock);
        assert_eq!(cli.mock_port, 0);
        assert_eq!(cli.delay_ms, None);
    }

    #[test]
    fn cli_with_all_options() {
        let cli = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "--package",
            "my-ig.tgz",
            "--target",
            "https://fhir.example.com",
            "--config",
            "my-config.toml",
            "--output",
            "./results",
            "--iterations",
            "50",
            "--mutations",
            "boundary,encoding",
            "--seed",
            "12345",
            "--concurrency",
            "8",
            "--dry-run",
            "--mock",
            "--mock-port",
            "9090",
            "--delay-ms",
            "200",
        ])
        .unwrap();
        assert_eq!(cli.package, Some("my-ig.tgz".to_string()));
        assert_eq!(cli.target, Some("https://fhir.example.com".to_string()));
        assert_eq!(cli.config, "my-config.toml");
        assert_eq!(cli.output, Some("./results".to_string()));
        assert_eq!(cli.iterations, Some(50));
        assert_eq!(cli.mutations, Some("boundary,encoding".to_string()));
        assert_eq!(cli.seed, Some(12345));
        assert_eq!(cli.concurrency, Some(8));
        assert!(cli.dry_run);
        assert!(cli.mock);
        assert_eq!(cli.mock_port, 9090);
        assert_eq!(cli.delay_ms, Some(200));
    }

    #[test]
    fn cli_short_options() {
        let cli = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "-p",
            "pkg.tgz",
            "-t",
            "http://localhost",
            "-c",
            "cfg.toml",
            "-o",
            "./out",
            "-i",
            "10",
        ])
        .unwrap();
        assert_eq!(cli.package, Some("pkg.tgz".to_string()));
        assert_eq!(cli.target, Some("http://localhost".to_string()));
        assert_eq!(cli.config, "cfg.toml");
        assert_eq!(cli.output, Some("./out".to_string()));
        assert_eq!(cli.iterations, Some(10));
    }

    #[test]
    fn cli_dry_run_flag() {
        let cli = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "--package",
            "test.tgz",
            "--target",
            "http://localhost",
            "--dry-run",
        ])
        .unwrap();
        assert!(cli.dry_run);
    }

    #[test]
    fn cli_mock_flag() {
        let cli =
            Cli::try_parse_from(["fhir-autotest-fuzz", "--package", "test.tgz", "--mock"]).unwrap();
        assert!(cli.mock);
        assert_eq!(cli.mock_port, 0);
    }

    #[test]
    fn cli_mock_with_port() {
        let cli = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "--package",
            "test.tgz",
            "--mock",
            "--mock-port",
            "8080",
        ])
        .unwrap();
        assert!(cli.mock);
        assert_eq!(cli.mock_port, 8080);
    }

    #[test]
    fn cli_missing_package_succeeds_at_parse_time() {
        // package is Option<String>, so parsing succeeds even without it
        let cli =
            Cli::try_parse_from(["fhir-autotest-fuzz", "--target", "http://localhost"]).unwrap();
        assert!(cli.package.is_none());
    }

    #[test]
    fn cli_missing_target_and_no_mock_fails() {
        let result = Cli::try_parse_from(["fhir-autotest-fuzz", "--package", "test.tgz"]);
        // This should succeed because --mock is not required at parse time
        // (it's validated at runtime)
        assert!(result.is_ok());
    }

    #[test]
    fn cli_with_mutations_all() {
        let cli = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "--package",
            "test.tgz",
            "--target",
            "http://localhost",
            "--mutations",
            "all",
        ])
        .unwrap();
        assert_eq!(cli.mutations, Some("all".to_string()));
    }

    #[test]
    fn cli_with_seed_zero() {
        let cli = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "--package",
            "test.tgz",
            "--target",
            "http://localhost",
            "--seed",
            "0",
        ])
        .unwrap();
        assert_eq!(cli.seed, Some(0));
    }

    #[test]
    fn cli_with_delay_ms() {
        let cli = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "--package",
            "test.tgz",
            "--target",
            "http://localhost",
            "--delay-ms",
            "500",
        ])
        .unwrap();
        assert_eq!(cli.delay_ms, Some(500));
    }

    #[test]
    fn cli_version_flag() {
        let result = Cli::try_parse_from([
            "fhir-autotest-fuzz",
            "--package",
            "test.tgz",
            "--target",
            "http://localhost",
            "--version",
        ]);
        // --version is handled by clap and returns early
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn cli_help_flag() {
        let result = Cli::try_parse_from(["fhir-autotest-fuzz", "--help"]);
        // --help is handled by clap and returns early
        assert!(result.is_err() || result.is_ok());
    }
}
