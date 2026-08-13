use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Benchmark mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchMode {
    /// Fixed concurrency for a fixed duration (default).
    #[default]
    Steady,
    /// Ramp concurrency upward until error rate or latency threshold is breached.
    MaxThroughput,
    /// Sustained load at fixed concurrency for an extended period.
    Soak,
}

use anyhow::Context;

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

/// Validate that a path does not escape the expected base directory.
///
/// Canonicalizes both the path and the base directory, then checks that
/// the resolved path starts with the resolved base. This prevents path
/// traversal attacks via `../` components.
///
/// For paths that don't exist yet (e.g. output directories), use
/// [`validate_output_path`] instead.
pub fn validate_path(path: &Path, base: &Path) -> anyhow::Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("Path does not exist or is inaccessible: {}", path.display()))?;
    let base_canonical = base.canonicalize().with_context(|| {
        format!(
            "Base directory does not exist or is inaccessible: {}",
            base.display()
        )
    })?;
    if !canonical.starts_with(&base_canonical) {
        anyhow::bail!(
            "Path traversal detected: {} resolves outside {}",
            path.display(),
            base.display()
        );
    }
    Ok(canonical)
}

/// Validate that an output path does not escape the current working directory
/// via `..` components.
///
/// Unlike [`validate_path`], this does not require the path to already exist.
/// Absolute paths are allowed (the user explicitly chose them). Relative paths
/// are resolved against the CWD and checked for traversal.
fn validate_output_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = std::env::current_dir().context("Failed to get current working directory")?;
    let resolved = cwd.join(path);
    // The parent directory must exist for canonicalization
    let parent = resolved.parent().unwrap_or(&resolved);
    let parent_canonical = parent.canonicalize().with_context(|| {
        format!(
            "Output parent directory does not exist: {}",
            parent.display()
        )
    })?;
    let cwd_canonical = cwd.canonicalize().context("Failed to canonicalize CWD")?;
    if !parent_canonical.starts_with(&cwd_canonical) {
        anyhow::bail!(
            "Path traversal detected: output path {} resolves outside the current working directory",
            path.display()
        );
    }
    Ok(resolved)
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

    /// Benchmark configuration for load/performance testing.
    ///
    /// These settings are read from the `[bench]` section of the config file
    /// and used by `fhir-autotest-bench` for concurrency, duration, ramp-up,
    /// and other benchmark parameters.
    #[serde(default)]
    pub bench: BenchConfig,

    /// Custom test definitions.
    ///
    /// Users can define their own tests in the `[custom_tests]` section of the
    /// config file. These are merged into the generated test plan and executed
    /// alongside the auto-generated tests.
    ///
    /// See [`CustomTestsConfig`] for the full format.
    #[serde(default)]
    pub custom_tests: CustomTestsConfig,
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

    /// Whether to verify TLS certificates (default: true).
    ///
    /// Set to `false` to accept self-signed or otherwise invalid certificates
    /// (useful for development/testing with internal CAs).
    #[serde(default = "default_tls_verify")]
    pub tls_verify: bool,

    /// Optional path to a PEM-encoded CA certificate bundle.
    ///
    /// When set, this certificate is added to the root certificate store,
    /// allowing connections to servers using custom/internal CAs.
    #[serde(default)]
    pub tls_ca_cert: Option<PathBuf>,
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

    /// Whether to verify TLS certificates (default: true).
    ///
    /// Set to `false` to accept self-signed or otherwise invalid certificates
    /// (useful for development/testing with internal CAs).
    #[serde(default = "default_tls_verify")]
    pub tls_verify: bool,

    /// Optional path to a PEM-encoded CA certificate bundle.
    ///
    /// When set, this certificate is added to the root certificate store,
    /// allowing connections to servers using custom/internal CAs.
    #[serde(default)]
    pub tls_ca_cert: Option<PathBuf>,
}

pub fn default_concurrency() -> usize {
    1
}

pub fn default_tls_verify() -> bool {
    true
}

/// TLS configuration for HTTP clients.
///
/// Controls certificate verification and custom CA certificates.
/// These settings apply to all HTTP clients created by the application.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Whether to verify TLS certificates (default: true).
    pub verify: bool,
    /// Optional path to a PEM-encoded CA certificate bundle.
    pub ca_cert: Option<PathBuf>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            verify: true,
            ca_cert: None,
        }
    }
}

impl TlsConfig {
    /// Extract TLS config from a `ServerConfig`.
    pub fn from_server(config: &ServerConfig) -> Self {
        Self {
            verify: config.tls_verify,
            ca_cert: config.tls_ca_cert.clone(),
        }
    }

    /// Extract TLS config from a `RepositoryConfig`.
    pub fn from_repository(config: &RepositoryConfig) -> Self {
        Self {
            verify: config.tls_verify,
            ca_cert: config.tls_ca_cert.clone(),
        }
    }

    /// Build a `reqwest::Client` with this TLS configuration applied.
    pub fn build_client(&self) -> anyhow::Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("fhir-autotest/0.1");
        if !self.verify {
            builder = builder.danger_accept_invalid_certs(true);
        }
        if let Some(ca_path) = &self.ca_cert {
            let cert = std::fs::read(ca_path)
                .with_context(|| format!("Failed to read CA certificate: {}", ca_path.display()))?;
            builder = builder.add_root_certificate(
                reqwest::Certificate::from_pem(&cert).with_context(|| {
                    format!(
                        "Failed to parse CA certificate from PEM: {}",
                        ca_path.display()
                    )
                })?,
            );
        }
        Ok(builder.build()?)
    }
}

#[derive(Debug, Clone, Deserialize)]
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
    /// Fetch the CapabilityStatement from the live server's `/metadata` endpoint.
    ///
    /// When true, the responder CapabilityStatement is fetched from
    /// `{server.base_url}/metadata` (using the configured server headers/TLS)
    /// instead of the one bundled in the IG package. Ignored when
    /// `capability_statement_file` is set (the file takes precedence).
    #[serde(default)]
    pub capability_statement_from_server: bool,
    /// Path to fixture JSON files directory.
    #[serde(default)]
    pub fixtures_dir: Option<PathBuf>,
    /// Map of resource type → fixture filename to use instead of generating.
    #[serde(default)]
    pub fixture_map: HashMap<String, String>,
    /// Maximum number of search parameters combined in a single combinatorial
    /// search test. All combination sizes from 2 up to this value are generated
    /// per resource (e.g. `3` produces pairs and triples). Default `2`.
    ///
    /// Higher values increase coverage but grow combinatorially, so raise it
    /// deliberately. Values below 2 disable combinatorial search tests.
    #[serde(default = "default_max_search_combo_params")]
    pub max_search_combo_params: usize,
}

impl Default for OverrideConfig {
    fn default() -> Self {
        Self {
            creation_order: Vec::new(),
            capability_statement_file: None,
            fixtures_dir: None,
            fixture_map: HashMap::new(),
            max_search_combo_params: default_max_search_combo_params(),
        }
    }
}

fn default_max_search_combo_params() -> usize {
    2
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

/// Benchmark configuration for load/performance testing.
///
/// These settings are read from the `[bench]` section of the config file
/// and used by `fhir-autotest-bench` for concurrency, duration, ramp-up,
/// and other benchmark parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct BenchConfig {
    /// Number of concurrent virtual users (connections).
    #[serde(default = "default_bench_concurrency")]
    pub concurrency: usize,

    /// Duration of the benchmark in seconds (0 = run all tests once).
    #[serde(default = "default_bench_duration_secs")]
    pub duration_secs: u64,

    /// Ramp-up time in seconds — gradually increase concurrency over this period.
    #[serde(default = "default_bench_ramp_up_secs")]
    pub ramp_up_secs: u64,

    /// Timeout per individual request in seconds.
    #[serde(default = "default_bench_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Path to an existing test_plan.json. If absent, one is generated.
    #[serde(default)]
    pub test_plan: Option<String>,

    /// Output directory for benchmark reports (default: ./bench-results).
    #[serde(default = "default_bench_output")]
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
    #[serde(default = "default_bench_warmup")]
    pub warmup_requests: usize,

    /// Benchmark mode: "steady" (default), "max_throughput", or "soak".
    #[serde(default)]
    pub mode: BenchMode,

    // ── Max-throughput mode fields ──────────────────────────────────────
    /// Starting concurrency for max-throughput ramp.
    #[serde(default = "default_bench_min_concurrency")]
    pub min_concurrency: usize,

    /// Maximum concurrency to try before giving up.
    #[serde(default = "default_bench_max_concurrency")]
    pub max_concurrency: usize,

    /// Concurrency increment per step.
    #[serde(default = "default_bench_step_size")]
    pub step_size: usize,

    /// Seconds to stabilize at each concurrency level.
    #[serde(default = "default_bench_stabilization_secs")]
    pub stabilization_secs: u64,

    /// Stop when error rate exceeds this fraction (0.0–1.0).
    #[serde(default = "default_bench_max_error_rate")]
    pub max_error_rate: f64,

    /// Stop when p95 latency exceeds this value in milliseconds.
    #[serde(default = "default_bench_max_latency_p95_ms")]
    pub max_latency_p95_ms: u64,

    // ── Soak mode fields ──────────────────────────────────────────────
    /// Duration in hours for soak mode (overrides duration_secs when mode=soak).
    #[serde(default = "default_bench_soak_hours")]
    pub soak_hours: u64,
}

fn default_bench_concurrency() -> usize {
    10
}
fn default_bench_duration_secs() -> u64 {
    30
}
fn default_bench_ramp_up_secs() -> u64 {
    5
}
fn default_bench_request_timeout_secs() -> u64 {
    30
}
fn default_bench_output() -> String {
    "./bench-results".to_string()
}
fn default_bench_warmup() -> usize {
    10
}
fn default_bench_min_concurrency() -> usize {
    1
}
fn default_bench_max_concurrency() -> usize {
    500
}
fn default_bench_step_size() -> usize {
    10
}
fn default_bench_stabilization_secs() -> u64 {
    10
}
fn default_bench_max_error_rate() -> f64 {
    0.05
}
fn default_bench_max_latency_p95_ms() -> u64 {
    1000
}
fn default_bench_soak_hours() -> u64 {
    4
}

impl BenchConfig {
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
            concurrency: default_bench_concurrency(),
            duration_secs: default_bench_duration_secs(),
            ramp_up_secs: default_bench_ramp_up_secs(),
            request_timeout_secs: default_bench_request_timeout_secs(),
            test_plan: None,
            output: default_bench_output(),
            skip_data_ensure: false,
            skip_cleanup: false,
            filter_groups: Vec::new(),
            warmup_requests: default_bench_warmup(),
            mode: BenchMode::default(),
            min_concurrency: default_bench_min_concurrency(),
            max_concurrency: default_bench_max_concurrency(),
            step_size: default_bench_step_size(),
            stabilization_secs: default_bench_stabilization_secs(),
            max_error_rate: default_bench_max_error_rate(),
            max_latency_p95_ms: default_bench_max_latency_p95_ms(),
            soak_hours: default_bench_soak_hours(),
        }
    }
}

/// Custom test definitions from the `[custom_tests]` section of config.toml.
///
/// Users can define their own tests alongside the auto-generated ones. The
/// simplest form is a single-request test where the tool generates a resource,
/// creates it, substitutes real IDs/values into the URL, runs the test, and
/// cleans up.
///
/// For multi-step sequences (e.g., create → update → verify propagation),
/// use `[[custom_tests.sequence]]` with named steps that pass state between
/// each other via `{steps.<name>.id}` and `{steps.<name>.<field>}` templates.
///
/// # Example
///
/// ```toml
/// [custom_tests]
///
/// # Override values used in auto-generated tests
/// overrides = [
///   { param = "identifier", value = "http://example.com/mrn|KNOWN-001" },
/// ]
///
/// # Skip auto-generated tests that don't apply
/// skip = ["SearchModifier:name:contains"]
///
/// # Simple single-request tests
/// [[custom_tests.test]]
/// name = "Read a Patient"
/// method = "GET"
/// url = "/Patient/{Patient.id}"
///
/// [[custom_tests.test]]
/// name = "Search by name"
/// method = "GET"
/// url = "/Patient?name={Patient.name.family}"
/// assert = { expected_status = 200, bundle_type = "searchset" }
///
/// # Multi-step sequence
/// [[custom_tests.sequence]]
/// name = "Propagation test"
///
///   [[custom_tests.sequence.step]]
///   action = "create"
///   resource = "Practitioner"
///   body_overrides = { name = [{ family = "Test" }] }
///   save_as = "prac"
///
///   [[custom_tests.sequence.step]]
///   action = "read"
///   resource = "Practitioner"
///   id = "{steps.prac.id}"
///   assert = { expected_status = 200 }
/// ```
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct CustomTestsConfig {
    /// Override values for auto-generated test parameters.
    /// Keyed by search parameter name, value is the replacement.
    pub overrides: Vec<CustomTestOverride>,

    /// Names of auto-generated tests to skip.
    /// Matched against the test's `name` field (substring match).
    pub skip: Vec<String>,

    /// Simple single-request custom tests.
    pub test: Vec<CustomTestDef>,

    /// Multi-step sequence tests.
    pub sequence: Vec<CustomSequenceDef>,
}

/// A single parameter override for auto-generated tests.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomTestOverride {
    pub param: String,
    pub value: String,
}

/// A single custom test definition.
///
/// The tool generates a resource for the specified `resource_type`, creates it
/// on the server, substitutes `{ResourceType.id}` and `{ResourceType.field}`
/// templates in the URL, runs the request, and asserts the response.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomTestDef {
    /// Test name (displayed in output).
    pub name: String,

    /// HTTP method: GET, POST, PUT, DELETE, PATCH.
    pub method: String,

    /// URL path (e.g. `/Patient/{Patient.id}`).
    /// `{base_url}` is replaced with the server base URL.
    /// `{ResourceType.id}` is replaced with the created resource's ID.
    /// `{ResourceType.field.path}` is replaced with extracted field values.
    pub url: String,

    /// Resource type for auto-generated data (e.g. "Patient").
    /// When set, the tool generates a resource from the IG profile, creates it,
    /// and substitutes its values into the URL.
    #[serde(default)]
    pub resource_type: String,

    /// Request body. Use `"auto"` to auto-generate from the profile.
    /// Omit or set to `null` for GET/DELETE requests.
    #[serde(default)]
    pub body: Option<serde_json::Value>,

    /// Request headers.
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Response assertions.
    #[serde(default)]
    pub assert: CustomAssertDef,
}

/// Response assertions for a custom test.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CustomAssertDef {
    /// Expected HTTP status code (default: 200).
    #[serde(default = "default_expected_status")]
    pub expected_status: u16,

    /// Expected Bundle type (e.g. "searchset", "history").
    #[serde(default)]
    pub bundle_type: Option<String>,

    /// Minimum number of Bundle entries.
    #[serde(default)]
    pub min_entries: Option<usize>,

    /// Maximum number of Bundle entries.
    #[serde(default)]
    pub max_entries: Option<usize>,

    /// Whether the Bundle must have a `total` field.
    #[serde(default)]
    pub bundle_total_present: Option<bool>,

    /// Expected summary mode.
    #[serde(default)]
    pub summary_mode: Option<String>,

    /// Expected OperationOutcome severity for error tests.
    #[serde(default)]
    pub outcome_severity: Option<String>,

    /// Alternative acceptable status codes.
    #[serde(default)]
    pub accept_statuses: Vec<u16>,

    /// Shorthand field value assertions.
    /// Key is a dot-path into the response (e.g. "name_family" → name[0].family).
    /// Value is the expected string value.
    #[serde(default)]
    pub field_values: HashMap<String, String>,

    /// Fields that must be absent from response resources.
    #[serde(default)]
    pub absent_fields: Vec<String>,

    /// Fields that must be present in response resources.
    #[serde(default)]
    pub required_fields: HashMap<String, Vec<String>>,

    /// Sort assertion.
    #[serde(default)]
    pub sort_by: Option<SortAssertionDef>,

    /// Expected resource types in the Bundle.
    #[serde(default)]
    pub resource_types: Vec<String>,

    /// Top-level response key that must exist.
    #[serde(default)]
    pub response_contains_key: Option<String>,

    /// Allowed response resourceType values.
    #[serde(default)]
    pub response_resource_types: Vec<String>,
}

fn default_expected_status() -> u16 {
    200
}

/// Sort assertion for custom tests.
#[derive(Debug, Clone, Deserialize)]
pub struct SortAssertionDef {
    pub field: String,
    pub direction: String,
    #[serde(default)]
    pub additional_fields: Vec<String>,
}

/// A multi-step sequence test.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomSequenceDef {
    /// Sequence name.
    pub name: String,
    /// Ordered list of steps.
    pub step: Vec<CustomSequenceStep>,
}

/// A single step in a multi-step sequence.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomSequenceStep {
    /// Action: "create", "read", "update", "delete", "search", "history", "patch".
    pub action: String,

    /// FHIR resource type (e.g. "Patient", "Practitioner").
    pub resource: String,

    /// Resource ID (for read/update/delete/history actions).
    /// Supports `{steps.<name>.id}` and `{steps.<name>.<field>}` templates.
    #[serde(default)]
    pub id: String,

    /// Body field overrides for create/update actions.
    /// Merged into the auto-generated resource body.
    #[serde(default)]
    pub body_overrides: Option<serde_json::Value>,

    /// Search query parameters (for search action).
    #[serde(default)]
    pub params: HashMap<String, String>,

    /// Name to save the response as (for `{steps.<name>.*}` references).
    #[serde(default)]
    pub save_as: String,

    /// Response assertions for this step.
    #[serde(default)]
    pub assert: CustomAssertDef,
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

        // Resolve the config file's parent directory for relative path validation
        let config_path = Path::new(path);
        let config_dir = config_path
            .parent()
            .context("Config file has no parent directory")?
            .canonicalize()
            .context("Failed to canonicalize config file directory")?;

        // Validate output path — must not escape the current working directory
        let output_path = Path::new(&config.output);
        validate_output_path(output_path)?;

        // Validate read-only paths that must exist
        if let Some(cs_path) = &config.overrides.capability_statement_file {
            validate_path(cs_path, &config_dir)?;
        }
        if let Some(fixtures_path) = &config.overrides.fixtures_dir {
            validate_path(fixtures_path, &config_dir)?;
        }

        // Resolve env vars in repository credentials
        if let Some(repo) = &mut config.repository {
            repo.username = resolve_env_vars(&repo.username);
            repo.password = resolve_env_vars(&repo.password);

            // Load credential file if configured
            if let Some(cred_path) = &repo.credential_file {
                let cred_path_str = resolve_env_vars(&cred_path.to_string_lossy());
                let cred_path_buf = PathBuf::from(&cred_path_str);
                // Validate credential file path against config file directory
                validate_path(&cred_path_buf, &config_dir)?;
                let cred_content = std::fs::read_to_string(&cred_path_str).map_err(|e| {
                    anyhow::anyhow!("Failed to read credential file '{}': {}", cred_path_str, e)
                })?;
                #[derive(Deserialize)]
                struct CredentialFile {
                    username: Option<String>,
                    password: Option<String>,
                }
                let creds: CredentialFile = toml::from_str(&cred_content).map_err(|e| {
                    anyhow::anyhow!("Failed to parse credential file '{}': {}", cred_path_str, e)
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
                tls_config: TlsConfig::from_repository(repo),
            },
            None => WriteEndpoint::Server {
                base_url: self.server.base_url.clone(),
                headers: self.server.headers.clone(),
                upload_method: UploadMethod::default(),
                concurrency: default_concurrency(),
                tls_config: TlsConfig::from_server(&self.server),
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
        /// TLS configuration for this endpoint.
        tls_config: TlsConfig,
    },
    Server {
        base_url: String,
        headers: HashMap<String, String>,
        /// "PUT" (default) or "POST" — the HTTP method for resource upload.
        upload_method: UploadMethod,
        /// Number of parallel requests for upload and delete.
        concurrency: usize,
        /// TLS configuration for this endpoint.
        tls_config: TlsConfig,
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

    #[test]
    fn validate_path_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let legit_path = dir.path().join("legit.txt");
        std::fs::write(&legit_path, "ok").unwrap();

        // Legitimate path should pass
        assert!(validate_path(&legit_path, dir.path()).is_ok());

        // Path traversal outside the base should fail
        let traversal = PathBuf::from("../../../etc/passwd");
        let result = validate_path(&traversal, dir.path());
        assert!(result.is_err(), "Expected path traversal to be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Path traversal detected") || err.contains("does not exist"),
            "Unexpected error: {err}"
        );
    }

    #[test]
    fn validate_output_path_rejects_traversal() {
        // Path traversal outside CWD should fail
        let traversal = Path::new("../../../etc");
        let result = validate_output_path(traversal);
        assert!(
            result.is_err(),
            "Expected output path traversal to be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Path traversal detected"),
            "Unexpected error: {err}"
        );
    }

    #[test]
    fn load_config_rejects_output_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let toml = r#"
output = "../../../etc"

[server]
base_url = "http://localhost:8080/fhir"
"#
        .to_string();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, &toml).unwrap();
        let result = TestConfig::load(config_path.to_str().unwrap());
        assert!(
            result.is_err(),
            "Expected output path traversal to be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Path traversal detected"),
            "Unexpected error: {err}"
        );
    }

    #[test]
    fn load_config_rejects_capability_statement_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[overrides]
capability_statement_file = "../../../etc/passwd"
"#
        .to_string();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, &toml).unwrap();
        let result = TestConfig::load(config_path.to_str().unwrap());
        assert!(result.is_err(), "Expected CS file traversal to be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Path traversal detected") || err.contains("does not exist"),
            "Unexpected error: {err}"
        );
    }

    #[test]
    fn load_config_rejects_fixtures_dir_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let toml = r#"
[server]
base_url = "http://localhost:8080/fhir"

[overrides]
fixtures_dir = "../../../etc"
"#
        .to_string();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, &toml).unwrap();
        let result = TestConfig::load(config_path.to_str().unwrap());
        assert!(
            result.is_err(),
            "Expected fixtures dir traversal to be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Path traversal detected") || err.contains("does not exist"),
            "Unexpected error: {err}"
        );
    }
}
