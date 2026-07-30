use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// HTTP method for uploading resources to the repository.
///
/// `Put` uses "update as create" (PUT /{rtype}/{id} with client-assigned IDs).
/// `Post` uses "create" (POST /{rtype} with server-assigned IDs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum UploadMethod {
    #[default]
    Put,
    Post,
}

impl std::fmt::Display for UploadMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadMethod::Put => write!(f, "PUT"),
            UploadMethod::Post => write!(f, "POST"),
        }
    }
}

/// Resolve `${ENV_VAR}` references in a string to their environment variable values.
///
/// If the referenced variable is not set, the placeholder is left as-is so the
/// user gets a clear error downstream rather than silently substituting an
/// empty string.
fn resolve_env_vars(s: &str) -> String {
    let mut result = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        // Push everything before the placeholder
        result.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        if let Some(end) = rest.find('}') {
            let var_name = &rest[..end];
            match std::env::var(var_name) {
                Ok(val) => result.push_str(&val),
                Err(_) => {
                    // Leave unresolved — the original placeholder stays so the
                    // user sees it in error messages rather than a silent empty string.
                    result.push_str(&format!("${{{}}}", var_name));
                }
            }
            rest = &rest[end + 1..];
        } else {
            // No closing brace — push the "${" and continue
            result.push_str("${");
        }
    }
    result.push_str(rest);
    result
}

/// Configuration for running tests against a FHIR server.
///
/// The config file is the single source of truth: it defines the IG package,
/// server URL, output location, and all overrides. CLI flags override specific
/// fields when provided.
#[derive(Debug, Clone, Deserialize)]
pub struct TestConfig {
    /// Path to the IG package (.tgz).
    pub package: Option<String>,

    /// Output directory for generated test plan and resources (default: ./output).
    #[serde(default = "default_output")]
    pub output: String,

    /// Run in dry-run mode: print all test URLs without executing.
    #[serde(default)]
    pub dry_run: bool,

    /// Server connection settings for the public-facing FHIR API.
    ///
    /// This is where test queries (GET/search) are sent. In development,
    /// this can also handle writes if no repository is configured.
    pub server: ServerConfig,

    /// Internal repository endpoint for creating and deleting resources.
    ///
    /// When set, POST/PUT/DELETE requests go here instead of the public server.
    /// This is useful when the public FHIR server only allows search/read,
    /// and data must be loaded via an internal repository service.
    ///
    /// If omitted, all requests go to `server`.
    #[serde(default)]
    pub repository: Option<RepositoryConfig>,

    /// Manual overrides for dependency order, fixtures, etc.
    #[serde(default)]
    pub overrides: OverrideConfig,

    /// Bulk data generation settings.
    ///
    /// When configured, realistic FHIR resources are generated in bulk,
    /// written as NDJSON files, uploaded to the repository before tests,
    /// and deleted after tests complete.
    #[serde(default)]
    pub data_generation: DataGenerationConfig,

    /// Use a built-in mock FHIR server instead of the configured server.
    ///
    /// Starts an in-process mock server and redirects all requests to it.
    /// The mock server supports CRUD operations and basic search filtering.
    /// Useful for development and CI where no real FHIR server is available.
    ///
    /// Can also be set via --mock on the CLI (CLI flag takes precedence).
    #[serde(default)]
    pub mock: bool,

    /// Port for the mock server (default: 0 = random available port).
    ///
    /// Only used when mock is enabled. Set to a specific port for reproducibility
    /// (e.g. for manual testing with curl). Use 0 for a random port.
    #[serde(default = "default_mock_port")]
    pub mock_port: u16,
}

fn default_output() -> String {
    "./output".to_string()
}

fn default_mock_port() -> u16 {
    0
}

/// Public-facing FHIR server configuration.
///
/// Used for test queries (read, search). Also used for writes
/// when no `repository` is configured.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Base URL of the FHIR server (e.g. "http://fhir.example.com/fhir").
    pub base_url: String,

    /// Optional HTTP headers sent with every request (e.g. auth tokens).
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// Internal repository configuration for resource creation/deletion.
///
/// The repository is a FHIR server (or proxy) that accepts POST/PUT/DELETE
/// but may not be publicly accessible. Username/password auth is typical.
///
/// # Security
///
/// Username and password support `${ENV_VAR}` syntax to read from environment
/// variables instead of storing plaintext credentials in the config file:
///
/// ```toml
/// [repository]
/// username = "${FHIR_REPO_USER}"
/// password = "${FHIR_REPO_PASS}"
/// ```
///
/// You can also use `credential_file` to point to a separate file (e.g.
/// `~/.fhir-autotest/credentials.toml`) with restricted permissions that
/// contains the credentials. The file format is:
///
/// ```toml
/// username = "admin"
/// password = "s3cret"
/// ```
///
/// When `credential_file` is set, its values override `username`/`password`
/// from the main config.
#[derive(Debug, Clone, Deserialize)]
pub struct RepositoryConfig {
    /// Base URL of the repository FHIR server (e.g. "http://repo.internal:8080/fhir").
    pub base_url: String,

    /// Username for basic auth. Supports `${ENV_VAR}` syntax.
    pub username: String,

    /// Password for basic auth. Supports `${ENV_VAR}` syntax.
    pub password: String,

    /// Optional path to a separate credentials file (TOML with `username` and `password`).
    ///
    /// When set, values from this file override `username`/`password` from the
    /// main config. The file should have restricted permissions (e.g. 600).
    /// Supports `${ENV_VAR}` syntax in the path itself.
    #[serde(default)]
    pub credential_file: Option<PathBuf>,

    /// HTTP method for resource upload: "PUT" (default) or "POST".
    ///
    /// PUT uses "update as create" (PUT /{rtype}/{id} with client-assigned IDs).
    /// POST uses "create" (POST /{rtype} with server-assigned IDs).
    #[serde(default)]
    pub upload_method: UploadMethod,

    /// Number of parallel requests for upload and delete operations.
    ///
    /// Set to 1 for sequential (safe for most repositories). Increase for
    /// repositories that can handle higher concurrency.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
}

pub fn default_concurrency() -> usize {
    1
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct OverrideConfig {
    /// Manual creation order (overrides auto-resolved order).
    #[serde(default)]
    pub creation_order: Vec<String>,
    /// Optional path to a CapabilityStatement JSON file.
    ///
    /// When set, this file is used as the responder CapabilityStatement
    /// instead of selecting one from the IG package.
    #[serde(default)]
    pub capability_statement_file: Option<PathBuf>,
    /// Path to fixture JSON files directory.
    #[serde(default)]
    pub fixtures_dir: Option<PathBuf>,
    /// Map of resource type → fixture filename to use instead of generating.
    #[serde(default)]
    pub fixture_map: HashMap<String, String>,
}

/// Bulk data generation settings.
///
/// Each key is a FHIR resource type (e.g. "Organization", "Practitioner"),
/// and the value is the number of resources to generate.
///
/// Generated resources are written as NDJSON (one line per resource) to
/// `{output}/data/{ResourceType}.ndjson`.
///
/// By default, generated data is uploaded to the repository before tests
/// and deleted afterward. Set `generate_only = true` to skip upload/delete
/// and keep the NDJSON files for manual use.
///
/// Resources reference each other: PractitionerRole points to Practitioner
/// and Organization, HealthcareService references Location, etc.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct DataGenerationConfig {
    /// Number of resources to generate per type.
    /// Key = FHIR resource type, Value = count.
    #[serde(default)]
    pub counts: HashMap<String, u64>,

    /// When true, generate NDJSON files but skip uploading to the repository
    /// and skip bulk deletion after tests. The files remain in
    /// `{output}/data/` for manual upload.
    #[serde(default)]
    pub generate_only: bool,
}

impl TestConfig {
    /// Load a TestConfig from a TOML file.
    ///
    /// After deserialization, `${ENV_VAR}` references in `username` and
    /// `password` fields are resolved from the environment. If a
    /// `credential_file` is configured, its values override the main config.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: TestConfig = toml::from_str(&content)?;

        // Resolve env vars in repository credentials
        if let Some(repo) = &mut config.repository {
            repo.username = resolve_env_vars(&repo.username);
            repo.password = resolve_env_vars(&repo.password);

            // Load credential file if configured
            if let Some(cred_path) = &repo.credential_file {
                let cred_path = resolve_env_vars(&cred_path.to_string_lossy());
                let cred_content = std::fs::read_to_string(&cred_path).map_err(|e| {
                    anyhow::anyhow!("Failed to read credential file '{}': {}", cred_path, e)
                })?;
                #[derive(Deserialize)]
                struct CredentialFile {
                    username: Option<String>,
                    password: Option<String>,
                }
                let creds: CredentialFile = toml::from_str(&cred_content).map_err(|e| {
                    anyhow::anyhow!("Failed to parse credential file '{}': {}", cred_path, e)
                })?;
                if let Some(u) = creds.username {
                    repo.username = resolve_env_vars(&u);
                }
                if let Some(p) = creds.password {
                    repo.password = resolve_env_vars(&p);
                }
            }
        }

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
                let content = std::fs::read_to_string(&path).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to load fixture '{}' for {}: {}",
                        filename,
                        resource_type,
                        e
                    )
                })?;
                let value: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to parse fixture JSON '{}' for {}: {}",
                        filename,
                        resource_type,
                        e
                    )
                })?;
                fixtures.insert(resource_type.clone(), value);
            }
        }
        Ok(fixtures)
    }

    /// Returns the repository config if set, otherwise falls back to the server config.
    ///
    /// Use this for POST/PUT/DELETE (resource creation and cleanup).
    /// In development mode where there's no separate repository, this returns
    /// a synthetic RepositoryConfig derived from the server config (no basic auth).
    pub fn write_endpoint(&self) -> WriteEndpoint {
        match &self.repository {
            Some(repo) => WriteEndpoint::Repository {
                base_url: repo.base_url.clone(),
                username: repo.username.clone(),
                password: repo.password.clone(),
                upload_method: repo.upload_method,
                concurrency: repo.concurrency,
            },
            None => WriteEndpoint::Server {
                base_url: self.server.base_url.clone(),
                headers: self.server.headers.clone(),
                upload_method: UploadMethod::default(),
                concurrency: default_concurrency(),
            },
        }
    }
}

/// The endpoint to use for write operations (POST/PUT/DELETE).
///
/// Either a repository with basic auth, or fall back to the public server
/// with its configured headers.
#[derive(Debug, Clone)]
pub enum WriteEndpoint {
    Repository {
        base_url: String,
        username: String,
        password: String,
        /// "PUT" (default) or "POST" — the HTTP method for resource upload.
        upload_method: UploadMethod,
        /// Number of parallel requests for upload and delete.
        concurrency: usize,
    },
    Server {
        base_url: String,
        headers: HashMap<String, String>,
        /// "PUT" (default) or "POST" — the HTTP method for resource upload.
        upload_method: UploadMethod,
        /// Number of parallel requests for upload and delete.
        concurrency: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_toml() {
        let toml = r#"
package = "ig-package.tgz"
output = "./test-output"
dry_run = true

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
        assert_eq!(config.package.as_deref(), Some("ig-package.tgz"));
        assert_eq!(config.output, "./test-output");
        assert!(config.dry_run);
        assert_eq!(config.server.base_url, "http://localhost:8080/fhir");
        assert_eq!(
            config.server.headers.get("Authorization").unwrap(),
            "Bearer test-token"
        );
        assert_eq!(
            config.overrides.creation_order,
            vec!["Patient", "Encounter", "Observation"]
        );
        assert!(config.overrides.fixtures_dir.is_some());
        assert_eq!(
            config.overrides.fixture_map.get("Patient").unwrap(),
            "us-core-patient.json"
        );
        assert!(config.repository.is_none());
    }

    #[test]
    fn parse_config_defaults() {
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert!(config.package.is_none());
        assert_eq!(config.output, "./output");
        assert!(!config.dry_run);
        assert!(config.server.headers.is_empty());
        assert!(config.overrides.creation_order.is_empty());
        assert!(config.repository.is_none());
        assert!(!config.mock);
        assert_eq!(config.mock_port, 0);
    }

    #[test]
    fn parse_config_with_mock() {
        let toml = r#"
mock = true
mock_port = 8091

[server]
base_url = "http://localhost:8080/fhir"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert!(config.mock);
        assert_eq!(config.mock_port, 8091);
    }

    #[test]
    fn parse_config_with_generate_only() {
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[data_generation]
generate_only = true
counts.Organization = 100
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert!(config.data_generation.generate_only);
        assert_eq!(
            config.data_generation.counts.get("Organization"),
            Some(&100)
        );
    }

    #[test]
    fn parse_config_with_repository() {
        let toml = r#"
package = "ig-package.tgz"

[server]
base_url = "https://fhir.example.com/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "admin"
password = "s3cret"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.server.base_url, "https://fhir.example.com/fhir");
        let repo = config.repository.unwrap();
        assert_eq!(repo.base_url, "http://repo.internal:8080/fhir");
        assert_eq!(repo.username, "admin");
        assert_eq!(repo.password, "s3cret");
    }

    #[test]
    fn write_endpoint_falls_back_to_server() {
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[server.headers]
Authorization = "Bearer test-token"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        match config.write_endpoint() {
            WriteEndpoint::Server {
                base_url, headers, ..
            } => {
                assert_eq!(base_url, "http://localhost:8080/fhir");
                assert_eq!(headers.get("Authorization").unwrap(), "Bearer test-token");
            }
            WriteEndpoint::Repository { .. } => panic!("Expected Server fallback"),
        }
    }

    #[test]
    fn write_endpoint_uses_repository_when_configured() {
        let toml = r#"
[server]
base_url = "https://fhir.example.com/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "admin"
password = "s3cret"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        match config.write_endpoint() {
            WriteEndpoint::Repository {
                base_url,
                username,
                password,
                upload_method,
                ..
            } => {
                assert_eq!(base_url, "http://repo.internal:8080/fhir");
                assert_eq!(username, "admin");
                assert_eq!(password, "s3cret");
                assert_eq!(upload_method, UploadMethod::Put);
            }
            WriteEndpoint::Server { .. } => panic!("Expected Repository"),
        }
    }

    #[test]
    fn upload_method_defaults_to_put() {
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "admin"
password = "s3cret"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.repository.unwrap().upload_method, UploadMethod::Put);
    }

    #[test]
    fn upload_method_can_be_post() {
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "admin"
password = "s3cret"
upload_method = "POST"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.repository.unwrap().upload_method, UploadMethod::Post);
    }

    #[test]
    fn concurrency_defaults_to_one() {
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "admin"
password = "s3cret"
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.repository.unwrap().concurrency, 1);
    }

    #[test]
    fn concurrency_can_be_configured() {
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "admin"
password = "s3cret"
concurrency = 10
"#;
        let config: TestConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.repository.unwrap().concurrency, 10);
    }

    #[test]
    fn resolve_env_vars_replaces_known_var() {
        unsafe { std::env::set_var("TEST_FHIR_USER", "env-admin") };
        let result = resolve_env_vars("${TEST_FHIR_USER}");
        assert_eq!(result, "env-admin");
    }

    #[test]
    fn resolve_env_vars_leaves_unknown_var() {
        let result = resolve_env_vars("${DOES_NOT_EXIST_XYZ123}");
        assert_eq!(result, "${DOES_NOT_EXIST_XYZ123}");
    }

    #[test]
    fn resolve_env_vars_mixed_plaintext_and_env() {
        unsafe {
            std::env::set_var("TEST_FHIR_USER", "env-admin");
            std::env::set_var("TEST_FHIR_PASS", "s3cret!");
        }
        let result = resolve_env_vars("user_${TEST_FHIR_USER}_pass_${TEST_FHIR_PASS}");
        assert_eq!(result, "user_env-admin_pass_s3cret!");
    }

    #[test]
    fn resolve_env_vars_no_placeholder() {
        let result = resolve_env_vars("plaintext-value");
        assert_eq!(result, "plaintext-value");
    }

    #[test]
    fn resolve_env_vars_empty_var_name() {
        let result = resolve_env_vars("prefix_${}_suffix");
        // Empty var name won't be found in env, so left as-is
        assert_eq!(result, "prefix_${}_suffix");
    }

    #[test]
    fn resolve_env_vars_multiple_vars() {
        unsafe {
            std::env::set_var("TEST_VAR_A", "alpha");
            std::env::set_var("TEST_VAR_B", "beta");
        }
        let result = resolve_env_vars("${TEST_VAR_A}_${TEST_VAR_B}");
        assert_eq!(result, "alpha_beta");
    }

    #[test]
    fn load_config_resolves_env_vars_in_repository() {
        unsafe {
            std::env::set_var("FHIR_REPO_USER_TEST", "env-user");
            std::env::set_var("FHIR_REPO_PASS_TEST", "env-pass");
        }
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "${FHIR_REPO_USER_TEST}"
password = "${FHIR_REPO_PASS_TEST}"
"#;
        // Write to a temp file and load via TestConfig::load
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, toml).unwrap();
        let config = TestConfig::load(config_path.to_str().unwrap()).unwrap();
        let repo = config.repository.unwrap();
        assert_eq!(repo.username, "env-user");
        assert_eq!(repo.password, "env-pass");
    }

    #[test]
    fn load_config_credential_file_overrides() {
        unsafe {
            std::env::set_var("CRED_FILE_USER_TEST", "cred-user");
            std::env::set_var("CRED_FILE_PASS_TEST", "cred-pass");
        }
        let dir = tempfile::tempdir().unwrap();

        // Write credential file
        let cred_path = dir.path().join("credentials.toml");
        std::fs::write(
            &cred_path,
            r#"username = "${CRED_FILE_USER_TEST}"
password = "${CRED_FILE_PASS_TEST}""#,
        )
        .unwrap();

        // Write config that references the credential file
        let config_toml = format!(
            r#"
[server]
base_url = "http://localhost:8080/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "should-be-overridden"
password = "should-be-overridden"
credential_file = "{}"
"#,
            cred_path.to_str().unwrap().replace('\\', "\\\\")
        );
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, &config_toml).unwrap();
        let config = TestConfig::load(config_path.to_str().unwrap()).unwrap();
        let repo = config.repository.unwrap();
        assert_eq!(repo.username, "cred-user");
        assert_eq!(repo.password, "cred-pass");
    }

    #[test]
    fn load_config_credential_file_partial_override() {
        let dir = tempfile::tempdir().unwrap();

        // Write credential file with only password
        let cred_path = dir.path().join("creds.toml");
        std::fs::write(&cred_path, r#"password = "from-file""#).unwrap();

        let config_toml = format!(
            r#"
[server]
base_url = "http://localhost:8080/fhir"

[repository]
base_url = "http://repo.internal:8080/fhir"
username = "main-user"
password = "main-pass"
credential_file = "{}"
"#,
            cred_path.to_str().unwrap().replace('\\', "\\\\")
        );
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, &config_toml).unwrap();
        let config = TestConfig::load(config_path.to_str().unwrap()).unwrap();
        let repo = config.repository.unwrap();
        // username should come from main config (not overridden)
        assert_eq!(repo.username, "main-user");
        // password should be overridden by credential file
        assert_eq!(repo.password, "from-file");
    }
}
