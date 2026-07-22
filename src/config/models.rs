use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration for running tests against a FHIR server.
#[derive(Debug, Deserialize)]
pub struct TestConfig {
    pub server: ServerConfig,
    #[serde(default)]
    pub overrides: OverrideConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub base_url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OverrideConfig {
    /// Manual creation order (overrides auto-resolved order).
    #[serde(default)]
    pub creation_order: Vec<String>,
    /// Path to fixture JSON files directory.
    #[serde(default)]
    pub fixtures_dir: Option<PathBuf>,
    /// Map of resource type → fixture filename to use instead of generating.
    #[serde(default)]
    pub fixture_map: HashMap<String, String>,
}

impl TestConfig {
    /// Load a TestConfig from a TOML file.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: TestConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load fixture resources from the configured fixtures directory.
    ///
    /// Reads each file referenced in `fixture_map`, parses it as JSON,
    /// and returns a map of resource_type → JSON value.
    pub fn load_fixtures(&self) -> anyhow::Result<HashMap<String, serde_json::Value>> {
        let mut fixtures = HashMap::new();
        if let Some(dir) = &self.overrides.fixtures_dir {
            for (resource_type, filename) in &self.overrides.fixture_map {
                let path = dir.join(filename);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| anyhow::anyhow!("Failed to load fixture '{}' for {}: {}", filename, resource_type, e))?;
                let value: serde_json::Value = serde_json::from_str(&content)
                    .map_err(|e| anyhow::anyhow!("Failed to parse fixture JSON '{}' for {}: {}", filename, resource_type, e))?;
                fixtures.insert(resource_type.clone(), value);
            }
        }
        Ok(fixtures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_toml() {
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[server.headers]
Authorization = "Bearer test-token"

[overrides]
creation_order = ["Patient", "Encounter", "Observation"]
fixtures_dir = "./fixtures"

[overrides.fixture_map]
Patient = "us-core-patient.json"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.server.base_url, "http://localhost:8080/fhir");
        assert_eq!(config.server.headers.get("Authorization").unwrap(), "Bearer test-token");
        assert_eq!(config.overrides.creation_order, vec!["Patient", "Encounter", "Observation"]);
        assert!(config.overrides.fixtures_dir.is_some());
        assert_eq!(config.overrides.fixture_map.get("Patient").unwrap(), "us-core-patient.json");
    }
}