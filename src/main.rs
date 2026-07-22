use clap::Parser;

#[derive(Parser)]
#[command(name = "fhir-ig-testgen")]
#[command(
    about = "FHIR R4 IG test generator — parse Implementation Guide packages and generate/run conformance tests"
)]
#[command(version)]
struct Cli {
    /// Path to config file (TOML).
    ///
    /// The config file is the source of truth: it defines the IG package path,
    /// server URL, output directory, and all overrides. CLI flags override
    /// specific fields when provided.
    ///
    /// Defaults to "config.toml" in the current directory.
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Override the IG package path from the config file.
    #[arg(short, long)]
    package: Option<String>,

    /// Override the output directory from the config file.
    #[arg(short, long)]
    output: Option<String>,

    /// Override: path to write detailed JSON test results.
    #[arg(long)]
    results: Option<String>,

    /// Override: run in dry-run mode (print URLs without executing).
    #[arg(long)]
    dry_run: bool,

    /// Generate only: produce test plan and resources without running against a server.
    #[arg(long)]
    generate: bool,

    /// Use a built-in mock FHIR server instead of the configured server.
    ///
    /// Starts an in-process mock server and points the config at it.
    /// The mock server supports CRUD operations and basic search filtering.
    /// Useful for development and CI where no real FHIR server is available.
    #[arg(long)]
    mock: bool,

    /// Port for the mock server (default: 0 = random available port).
    /// Only used with --mock.
    #[arg(long, default_value = "0")]
    mock_port: u16,

    /// Validate a JSON resource against a profile from the IG package.
    /// Requires --resource (and optionally --profile).
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Validate a JSON resource against a profile from the IG package
    Validate {
        /// Path to the resource JSON file to validate
        #[arg(short, long)]
        resource: String,

        /// Profile canonical URL to validate against (optional — auto-detect by resource type)
        #[arg(long)]
        profile: Option<String>,
    },
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

    // Handle the validate subcommand separately — it doesn't need a full config
    if let Some(Commands::Validate { resource, profile }) = cli.command {
        // Load config just to get the package path
        let config = fhir_ig_testgen::TestConfig::load(&cli.config)?;
        let package = cli.package.or(config.package).ok_or_else(|| {
            anyhow::anyhow!(
                "No IG package path specified. Set 'package' in the config file or use --package."
            )
        })?;
        fhir_ig_testgen::run_validate(&package, &resource, profile.as_deref())?;
        return Ok(());
    }

    // Load config — the single source of truth
    let mut config = fhir_ig_testgen::TestConfig::load(&cli.config)?;

    // If --mock is set or config.mock is true, start the mock server and redirect
    let use_mock = cli.mock || config.mock;
    let mock_port = if cli.mock_port != 0 {
        cli.mock_port
    } else {
        config.mock_port
    };
    if use_mock {
        let addr = fhir_ig_testgen::mock_server::start_mock_server(mock_port).await?;
        let mock_url = format!("http://{}", addr);
        println!("Mock FHIR server running at {}", mock_url);
        config.server.base_url = mock_url.clone();
        // Clear repository — mock server handles both read and write
        config.repository = None;
    }

    // CLI flags override config values
    if let Some(pkg) = cli.package {
        config.package = Some(pkg);
    }
    if let Some(output) = cli.output {
        config.output = output;
    }
    if let Some(results) = cli.results {
        config.results = Some(results);
    }
    if cli.dry_run {
        config.dry_run = true;
    }

    // Resolve the package path (required)
    let package = config.package.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "No IG package path specified. Set 'package' in the config file or use --package."
        )
    })?;

    // Determine mode: --generate, --dry_run, or full run
    if cli.generate {
        // Generate-only mode: no server needed, just produce test plan + resources
        fhir_ig_testgen::run_generate(package, &config)?;
    } else if config.dry_run {
        fhir_ig_testgen::run_dry_run(package, &config)?;
    } else {
        fhir_ig_testgen::run_generate(package, &config)?;
        fhir_ig_testgen::run_tests(package, &config).await?;
    }

    Ok(())
}
