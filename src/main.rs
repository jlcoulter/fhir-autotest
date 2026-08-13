use clap::Parser;

#[derive(Parser)]
#[command(name = "fhir-autotest")]
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
    #[arg(short, long, default_value = "./config.toml")]
    config: String,

    /// Override the IG package path from the config file.
    #[arg(short, long)]
    package: Option<String>,

    /// Override the output directory from the config file.
    #[arg(short, long)]
    output: Option<String>,

    /// Override: run in dry-run mode (print URLs without executing).
    #[arg(long)]
    dry_run: bool,

    /// Generate only: produce test plan and resources without running against a server.
    #[arg(long)]
    generate: bool,

    /// Generate an OpenAPI 3.0 spec from the CapabilityStatement to
    /// {output}/openapi.json, then exit (no resources or tests).
    #[arg(long)]
    openapi: bool,

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
        let config = fhir_autotest::TestConfig::load(&cli.config)?;
        let package = cli.package.or(config.package).ok_or_else(|| {
            anyhow::anyhow!(
                "No IG package path specified. Set 'package' in the config file or use --package."
            )
        })?;
        fhir_autotest::run_validate(&package, &resource, profile.as_deref()).await?;
        return Ok(());
    }

    // Load config — the single source of truth
    let mut config = fhir_autotest::TestConfig::load(&cli.config)?;

    // If --mock is set or config.mock is true, start the mock server and redirect
    let use_mock = cli.mock || config.mock;
    let mock_port = if cli.mock_port != 0 {
        cli.mock_port
    } else {
        config.mock_port
    };
    if use_mock {
        let addr = fhir_autotest::mock_server::start_mock_server(mock_port).await?;
        let mock_url = format!("http://{}/fhir", addr);
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
    if cli.dry_run {
        config.dry_run = true;
    }

    // OpenAPI mode: emit the spec and exit. The IG package is optional when the
    // CapabilityStatement is sourced from the server /metadata endpoint.
    if cli.openapi {
        let uses_server = config.overrides.capability_statement_from_server
            && config.overrides.capability_statement_file.is_none();
        let package = if uses_server {
            config.package.clone().unwrap_or_default()
        } else {
            config.package.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "No IG package path specified. Set 'package', use --package, or enable \
                     capability_statement_from_server to source the CapabilityStatement from \
                     the server."
                )
            })?
        };
        fhir_autotest::run_openapi(&package, &config).await?;
        return Ok(());
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
        fhir_autotest::run_generate(package, &config).await?;
    } else if config.dry_run {
        fhir_autotest::run_dry_run(package, &config).await?;
    } else {
        fhir_autotest::run_generate(package, &config).await?;
        fhir_autotest::run_tests(package, &config).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn cli_defaults() {
        let cli = Cli::parse_from(["fhir-autotest"]);
        assert_eq!(cli.config, "./config.toml");
        assert!(cli.package.is_none());
        assert!(cli.output.is_none());
        assert!(!cli.dry_run);
        assert!(!cli.generate);
        assert!(!cli.openapi);
        assert!(!cli.mock);
        assert_eq!(cli.mock_port, 0);
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_with_package() {
        let cli = Cli::parse_from(["fhir-autotest", "--package", "test-ig.tgz"]);
        assert_eq!(cli.package.as_deref(), Some("test-ig.tgz"));
    }

    #[test]
    fn cli_with_output() {
        let cli = Cli::parse_from(["fhir-autotest", "--output", "/tmp/output"]);
        assert_eq!(cli.output.as_deref(), Some("/tmp/output"));
    }

    #[test]
    fn cli_with_dry_run() {
        let cli = Cli::parse_from(["fhir-autotest", "--dry-run"]);
        assert!(cli.dry_run);
    }

    #[test]
    fn cli_with_generate() {
        let cli = Cli::parse_from(["fhir-autotest", "--generate"]);
        assert!(cli.generate);
    }

    #[test]
    fn cli_with_mock() {
        let cli = Cli::parse_from(["fhir-autotest", "--mock"]);
        assert!(cli.mock);
    }

    #[test]
    fn cli_with_mock_port() {
        let cli = Cli::parse_from(["fhir-autotest", "--mock", "--mock-port", "8080"]);
        assert!(cli.mock);
        assert_eq!(cli.mock_port, 8080);
    }

    #[test]
    fn cli_with_config() {
        let cli = Cli::parse_from(["fhir-autotest", "--config", "/path/to/config.toml"]);
        assert_eq!(cli.config, "/path/to/config.toml");
    }

    #[test]
    fn cli_with_validate_subcommand() {
        let cli = Cli::parse_from([
            "fhir-autotest",
            "validate",
            "--resource",
            "test-resource.json",
        ]);
        assert!(cli.command.is_some());
        match cli.command.unwrap() {
            Commands::Validate { resource, profile } => {
                assert_eq!(resource, "test-resource.json");
                assert!(profile.is_none());
            }
        }
    }

    #[test]
    fn cli_with_validate_subcommand_and_profile() {
        let cli = Cli::parse_from([
            "fhir-autotest",
            "validate",
            "--resource",
            "test-resource.json",
            "--profile",
            "http://example.org/StructureDefinition/Test",
        ]);
        assert!(cli.command.is_some());
        match cli.command.unwrap() {
            Commands::Validate { resource, profile } => {
                assert_eq!(resource, "test-resource.json");
                assert_eq!(
                    profile.as_deref(),
                    Some("http://example.org/StructureDefinition/Test")
                );
            }
        }
    }

    #[test]
    fn cli_all_flags() {
        let cli = Cli::parse_from([
            "fhir-autotest",
            "--config",
            "my-config.toml",
            "--package",
            "my-ig.tgz",
            "--output",
            "/tmp/results",
            "--dry-run",
            "--generate",
            "--mock",
            "--mock-port",
            "9090",
        ]);
        assert_eq!(cli.config, "my-config.toml");
        assert_eq!(cli.package.as_deref(), Some("my-ig.tgz"));
        assert_eq!(cli.output.as_deref(), Some("/tmp/results"));
        assert!(cli.dry_run);
        assert!(cli.generate);
        assert!(cli.mock);
        assert_eq!(cli.mock_port, 9090);
    }
}
