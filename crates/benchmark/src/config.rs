pub use fhir_autotest::config::models::BenchConfig;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bench_config_defaults() {
        let cfg = BenchConfig::default();
        assert_eq!(cfg.concurrency, 10);
        assert_eq!(cfg.duration_secs, 30);
        assert_eq!(cfg.ramp_up_secs, 5);
        assert_eq!(cfg.request_timeout_secs, 30);
        assert_eq!(cfg.output, "./bench-results");
        assert!(cfg.test_plan.is_none());
        assert!(!cfg.skip_data_ensure);
        assert!(!cfg.skip_cleanup);
        assert!(cfg.filter_groups.is_empty());
        assert_eq!(cfg.warmup_requests, 10);
    }

    #[test]
    fn bench_config_duration_methods() {
        let cfg = BenchConfig::default();
        assert_eq!(cfg.duration(), Duration::from_secs(30));
        assert_eq!(cfg.ramp_up(), Duration::from_secs(5));
        assert_eq!(cfg.request_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn bench_config_deserialize_from_bench_section() {
        let toml = r#"
[server]
base_url = "http://localhost/fhir"

[bench]
concurrency = 20
duration_secs = 60
ramp_up_secs = 10
output = "./custom-bench"
filter_groups = ["Patient", "Observation"]
"#;
        let config: fhir_autotest::config::models::TestConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.bench.concurrency, 20);
        assert_eq!(config.bench.duration_secs, 60);
        assert_eq!(config.bench.ramp_up_secs, 10);
        assert_eq!(config.bench.output, "./custom-bench");
        assert_eq!(config.bench.filter_groups, vec!["Patient", "Observation"]);
    }

    #[test]
    fn bench_config_defaults_when_section_missing() {
        let toml = r#"
[server]
base_url = "http://localhost/fhir"
"#;
        let config: fhir_autotest::config::models::TestConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.bench.concurrency, 10);
        assert_eq!(config.bench.duration_secs, 30);
    }
}
