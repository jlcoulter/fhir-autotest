use clap::Parser;

#[derive(Parser)]
#[command(name = "fhir-ig-testgen")]
#[command(about = "FHIR R4 IG test generator — parse Implementation Guide packages and generate/run conformance tests")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Generate test plan and synthetic resources from an IG package
    Generate {
        /// Path to the IG package (.tgz)
        #[arg(short, long)]
        package: String,

        /// Path to config file (TOML)
        #[arg(short, long)]
        config: Option<String>,

        /// Output directory for generated test plan and resources
        #[arg(short, long, default_value = "./output")]
        output: String,
    },
    /// Run tests against a FHIR server
    Run {
        /// Path to the IG package (.tgz)
        #[arg(short, long)]
        package: String,

        /// Path to config file (TOML) with server URL
        #[arg(short, long)]
        config: String,

        /// Print all test URLs without executing them
        #[arg(long)]
        dry_run: bool,

        /// Write detailed JSON results to this file
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Validate a JSON resource against a profile from the IG package
    Validate {
        /// Path to the IG package (.tgz)
        #[arg(short, long)]
        package: String,

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

    match cli.command {
        Commands::Generate { package, config, output } => {
            fhir_ig_testgen::run_generate(&package, config.as_deref(), &output)?;
        }
        Commands::Run { package, config, dry_run, output } => {
            if dry_run {
                fhir_ig_testgen::run_dry_run(&package, Some(&config))?;
            } else {
                fhir_ig_testgen::run_generate(&package, Some(&config), "./output")?;
                fhir_ig_testgen::run_tests(&package, &config, output.as_deref()).await?;
            }
        }
        Commands::Validate { package, resource, profile } => {
            fhir_ig_testgen::run_validate(&package, &resource, profile.as_deref())?;
        }
    }

    Ok(())
}