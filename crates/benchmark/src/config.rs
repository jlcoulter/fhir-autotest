use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

/// Benchmark-specific configuration, layered on top of the project's TestConfig.
#[derive(Debug, Clone, Deserialize)]
pub struct BenchConfig {
    /// Path to the project's config.toml (default: ./config.toml).
    #[serde(default = "default_config_path")]
    pub config_path: String,

    /// Number of concurrent virtual users (connections).
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,

    /// Duration of the benchmark in seconds (0 = run all tests once).
    #[serde(default = "default_duration_secs")]
    pub duration_secs: u64,

    /// Ramp-up time in seconds — gradually increase concurrency over this period.
    #[serde(default = "default_ramp_up_secs")]
    pub ramp_up_secs: u64,

    /// Timeout per individual request in seconds.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Path to an existing test_plan.json. If absent, one is generated.
    #[serde(default)]
    pub test_plan: Option<String>,

    /// Output directory for benchmark reports (default: ./bench-results).
    #[serde(default = "default_output")]
    pub output: String,

    /// Whether to skip the data-ensure step (assume data already exists).
    #[serde(default)]
    pub skip_data_ensure: bool,

    /// Whether to skip cleanup after the benchmark.
    #[serde(default)]
    pub skip_cleanup: bool,

    /// Filter test groups by resource type (e.g. ["Patient", "Observation"]).
    /// Empty means all groups.
    #[serde(default)]
    pub filter_groups: Vec<String>,

    /// Warm-up requests before recording measurements (number of requests).
    #[serde(default = "default_warmup")]
    pub warmup_requests: usize,

    /// Use a built-in mock FHIR server instead of the configured server.
    #[serde(default)]
    pub mock: bool,

    /// Port for the mock server (default: 0 = random available port).
    #[serde(default = "default_mock_port")]
    pub mock_port: u16,
}

fn default_config_path() -> String {
    "./config.toml".to_string()
}

fn default_concurrency() -> usize {
    10
}

fn default_duration_secs() -> u64 {
    30
}

fn default_ramp_up_secs() -> u64 {
    5
}

fn default_request_timeout_secs() -> u64 {
    30
}

fn default_output() -> String {
    "./bench-results".to_string()
}

fn default_warmup() -> usize {
    10
}

fn default_mock_port() -> u16 {
    0
}

impl BenchConfig {
    /// Load from a TOML file. Fields not present use defaults.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read bench config '{}': {}", path, e))?;
        let config: BenchConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load from CLI args (overrides config file values).
    pub fn from_cli() -> anyhow::Result<Self> {
        // Start with defaults, then try to load from file if it exists
        let mut config = Self::default();

        let default_path = "./bench-config.toml";
        if Path::new(default_path).exists() {
            config = Self::load(default_path)?;
        }

        Ok(config)
    }

    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs(self.duration_secs)
    }

    pub fn ramp_up(&self) -> Duration {
        Duration::from_secs(self.ramp_up_secs)
    }
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            config_path: default_config_path(),
            concurrency: default_concurrency(),
            duration_secs: default_duration_secs(),
            ramp_up_secs: default_ramp_up_secs(),
            request_timeout_secs: default_request_timeout_secs(),
            test_plan: None,
            output: default_output(),
            skip_data_ensure: false,
            skip_cleanup: false,
            filter_groups: Vec::new(),
            warmup_requests: default_warmup(),
            mock: false,
            mock_port: 0,
        }
    }
}
