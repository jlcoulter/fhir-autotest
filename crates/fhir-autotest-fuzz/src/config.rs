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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_iterations_value() {
        assert_eq!(default_iterations(), 100);
    }

    #[test]
    fn default_mutations_value() {
        assert_eq!(default_mutations(), "all");
    }

    #[test]
    fn default_concurrency_value() {
        assert_eq!(default_concurrency(), 4);
    }

    #[test]
    fn fuzz_config_default() {
        let config = FuzzConfig::default();
        assert_eq!(config.iterations, 100);
        assert_eq!(config.mutations, "all");
        assert_eq!(config.seed, 0);
        assert_eq!(config.delay_ms, 0);
        assert_eq!(config.concurrency, 4);
    }

    #[test]
    fn fuzz_config_deserialize_empty() {
        let config: FuzzConfig = toml::from_str("").unwrap();
        assert_eq!(config.iterations, 100);
        assert_eq!(config.mutations, "all");
        assert_eq!(config.seed, 0);
        assert_eq!(config.delay_ms, 0);
        assert_eq!(config.concurrency, 4);
    }

    #[test]
    fn fuzz_config_deserialize_partial() {
        let toml_str = r#"
iterations = 50
seed = 12345
"#;
        let config: FuzzConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.iterations, 50);
        assert_eq!(config.mutations, "all");
        assert_eq!(config.seed, 12345);
        assert_eq!(config.delay_ms, 0);
        assert_eq!(config.concurrency, 4);
    }

    #[test]
    fn fuzz_config_deserialize_all_fields() {
        let toml_str = r#"
iterations = 200
mutations = "boundary,encoding"
seed = 42
delay_ms = 100
concurrency = 8
"#;
        let config: FuzzConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.iterations, 200);
        assert_eq!(config.mutations, "boundary,encoding");
        assert_eq!(config.seed, 42);
        assert_eq!(config.delay_ms, 100);
        assert_eq!(config.concurrency, 8);
    }

    #[test]
    fn fuzz_config_deserialize_under_fuzz_section() {
        let toml_str = r#"
iterations = 75
mutations = "cardinality"
seed = 999
delay_ms = 500
concurrency = 2
"#;
        let config: FuzzConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.iterations, 75);
        assert_eq!(config.mutations, "cardinality");
        assert_eq!(config.seed, 999);
        assert_eq!(config.delay_ms, 500);
        assert_eq!(config.concurrency, 2);
    }

    #[test]
    fn fuzz_config_debug_and_clone() {
        let config = FuzzConfig::default();
        let _debug = format!("{:?}", config);
        let cloned = config.clone();
        assert_eq!(config.iterations, cloned.iterations);
        assert_eq!(config.seed, cloned.seed);
    }
}
