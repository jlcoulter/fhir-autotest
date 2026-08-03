use serde::Deserialize;

/// Fuzzer-specific configuration, read from the `[fuzz]` section of config.toml.
///
/// All fields have CLI flag equivalents that override these values.
#[derive(Debug, Clone, Deserialize)]
pub struct FuzzConfig {
    /// Number of fuzz iterations per resource type per mutator (default: 100).
    #[serde(default = "default_iterations")]
    pub iterations: usize,

    /// Comma-separated mutation categories (default: "all").
    #[serde(default = "default_mutations")]
    pub mutations: String,

    /// Seed for deterministic fuzzing (0 = random, default: 0).
    #[serde(default)]
    pub seed: u64,

    /// Delay in milliseconds between requests (default: 0).
    #[serde(default)]
    pub delay_ms: u64,

    /// Concurrency level (default: 4).
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
}

fn default_iterations() -> usize {
    100
}

fn default_mutations() -> String {
    "all".to_string()
}

fn default_concurrency() -> usize {
    4
}

impl Default for FuzzConfig {
    fn default() -> Self {
        Self {
            iterations: default_iterations(),
            mutations: default_mutations(),
            seed: 0,
            delay_ms: 0,
            concurrency: default_concurrency(),
        }
    }
}
