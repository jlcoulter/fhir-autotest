use serde::Serialize;
use std::time::Instant;

/// Patterns that indicate information leakage in response bodies.
/// Each entry is (label, characteristic_substring).
const LEAK_PATTERNS: &[(&str, &str)] = &[
    // Java stack traces
    ("java_stack", ".java:"),
    // .NET stack traces
    ("dotnet_stack", ".cs:"),
    // Python tracebacks
    ("python_tb", "Traceback (most recent call last)"),
    ("python_error", "File \""),
    // Go stack traces
    ("go_stack", "goroutine "),
    ("go_file", ".go:"),
    // Rust panics
    ("rust_panic", "panicked at "),
    // SQL errors
    ("sql_error", "SQL error"),
    ("sql_exception", "SQLException"),
    ("mysql_error", "doesn't exist"),
    ("postgres_error", "does not exist"),
    ("sqlite_error", "no such table"),
    // Path disclosure
    ("path_etc", "/etc/"),
    ("path_var", "/var/www/"),
    ("path_usr", "/usr/local/"),
    ("path_win", "C:\\Users\\"),
    ("path_inetpub", "C:\\inetpub\\"),
    // HTML error pages
    ("html_500", "<title>500"),
    ("html_502", "<title>502"),
    ("html_503", "<title>503"),
    ("html_504", "<title>504"),
    ("html_internal_error", "Internal Server Error"),
    ("html_runtime_error", "Runtime Error"),
    // XML / XXE
    ("xml_error", "XML parse error"),
    ("xml_entity", "XML entity"),
    // JSON parsing errors
    ("json_parse_error", "JSON parse error"),
    ("json_deserialize", "deserialize JSON"),
    // Null pointer / NPE
    ("null_pointer", "NullPointerException"),
    ("null_reference", "NullReferenceException"),
    // Hibernate / ORM
    ("hibernate", "org.hibernate."),
    // Spring Boot
    ("spring", "org.springframework."),
    // Stack trace generic
    ("stack_trace", "Stack trace:"),
    // PHP errors
    ("php_error", "PHP Fatal error"),
    ("php_warning", "PHP Warning"),
    // Node.js / JavaScript
    ("js_error", "TypeError:"),
    ("js_reference", "ReferenceError:"),
    // Ruby
    ("ruby_error", "NoMethodError"),
    ("ruby_backtrace", "from "),
];

/// Result of a single fuzzed request.
#[derive(Debug, Clone, Serialize)]
pub struct FuzzResult {
    /// Name of the mutation category that produced this request.
    pub mutator: String,
    /// Resource type being fuzzed.
    pub resource_type: String,
    /// Iteration index within this mutator.
    pub iteration: usize,
    /// HTTP method used.
    pub method: String,
    /// URL the request was sent to.
    pub url: String,
    /// HTTP status code returned.
    pub status_code: u16,
    /// Whether the response indicates a potential issue.
    pub is_anomaly: bool,
    /// Why this was flagged as an anomaly.
    pub reason: Option<String>,
    /// Round-trip time in milliseconds.
    pub duration_ms: u64,
    /// Size of the request body in bytes (0 for GET).
    pub body_size: usize,
    /// Size of the response body in bytes.
    pub response_size: usize,
    /// First 500 chars of the response body (for debugging leaks).
    pub response_snippet: Option<String>,
}

impl FuzzResult {
    pub fn is_anomaly(&self) -> bool {
        self.is_anomaly
    }
}

/// Sends fuzzed FHIR requests to the target server and records responses.
pub struct FuzzRunner {
    client: reqwest::Client,
    base_url: String,
    _concurrency: usize,
    dry_run: bool,
}

impl FuzzRunner {
    pub fn new(base_url: &str, _concurrency: usize, dry_run: bool) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("fhir-autotest-fuzz/0.1")
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            _concurrency,
            dry_run,
        }
    }

    /// Send a fuzzed POST (create) request with a mutated body.
    pub async fn send_fuzzed(
        &self,
        resource_type: &str,
        body: &serde_json::Value,
        mutator_name: &str,
        iteration: usize,
    ) -> FuzzResult {
        let url = format!("{}/{}", self.base_url, resource_type);
        let body_bytes = serde_json::to_vec(body).unwrap_or_default();
        let body_size = body_bytes.len();

        if self.dry_run {
            return FuzzResult {
                mutator: mutator_name.to_string(),
                resource_type: resource_type.to_string(),
                iteration,
                method: "POST".to_string(),
                url: url.clone(),
                status_code: 0,
                is_anomaly: false,
                reason: None,
                duration_ms: 0,
                body_size,
                response_size: 0,
                response_snippet: None,
            };
        }

        let start = Instant::now();
        let response = self.client.post(&url).json(body).send().await;
        let duration_ms = start.elapsed().as_millis() as u64;

        self.classify_response(
            response,
            mutator_name,
            resource_type,
            iteration,
            "POST",
            url,
            body_size,
            duration_ms,
        )
        .await
    }

    /// Send a fuzzed PUT (update) request with a mutated body.
    pub async fn send_fuzzed_put(
        &self,
        resource_type: &str,
        id: &str,
        body: &serde_json::Value,
        mutator_name: &str,
        iteration: usize,
    ) -> FuzzResult {
        let url = format!("{}/{}/{}", self.base_url, resource_type, id);
        let body_bytes = serde_json::to_vec(body).unwrap_or_default();
        let body_size = body_bytes.len();

        if self.dry_run {
            return FuzzResult {
                mutator: mutator_name.to_string(),
                resource_type: resource_type.to_string(),
                iteration,
                method: "PUT".to_string(),
                url: url.clone(),
                status_code: 0,
                is_anomaly: false,
                reason: None,
                duration_ms: 0,
                body_size,
                response_size: 0,
                response_snippet: None,
            };
        }

        let start = Instant::now();
        let response = self.client.put(&url).json(body).send().await;
        let duration_ms = start.elapsed().as_millis() as u64;

        self.classify_response(
            response,
            mutator_name,
            resource_type,
            iteration,
            "PUT",
            url,
            body_size,
            duration_ms,
        )
        .await
    }

    /// Send a fuzzed GET (search) request with fuzzed query parameters.
    pub async fn send_fuzzed_search(
        &self,
        resource_type: &str,
        query_string: &str,
        mutator_name: &str,
        iteration: usize,
    ) -> FuzzResult {
        let url = format!("{}/{}{}", self.base_url, resource_type, query_string);

        if self.dry_run {
            return FuzzResult {
                mutator: mutator_name.to_string(),
                resource_type: resource_type.to_string(),
                iteration,
                method: "GET".to_string(),
                url: url.clone(),
                status_code: 0,
                is_anomaly: false,
                reason: None,
                duration_ms: 0,
                body_size: 0,
                response_size: 0,
                response_snippet: None,
            };
        }

        let start = Instant::now();
        let response = self.client.get(&url).send().await;
        let duration_ms = start.elapsed().as_millis() as u64;

        self.classify_response(
            response,
            mutator_name,
            resource_type,
            iteration,
            "GET",
            url,
            0,
            duration_ms,
        )
        .await
    }

    /// Classify a response: extract status, detect anomalies, check body for leaks.
    #[allow(clippy::too_many_arguments)]
    async fn classify_response(
        &self,
        response: Result<reqwest::Response, reqwest::Error>,
        mutator_name: &str,
        resource_type: &str,
        iteration: usize,
        method: &str,
        url: String,
        body_size: usize,
        duration_ms: u64,
    ) -> FuzzResult {
        match response {
            Ok(resp) => {
                let status = resp.status().as_u16();

                // Read the response body for leak detection
                let (response_text, response_size) = match resp.text().await {
                    Ok(text) => {
                        let size = text.len();
                        (Some(text), size)
                    }
                    Err(_) => (None, 0),
                };

                let mut result = FuzzResult {
                    mutator: mutator_name.to_string(),
                    resource_type: resource_type.to_string(),
                    iteration,
                    method: method.to_string(),
                    url,
                    status_code: status,
                    is_anomaly: false,
                    reason: None,
                    duration_ms,
                    body_size,
                    response_size,
                    response_snippet: response_text.as_ref().map(|t| {
                        let snippet: String = t.chars().take(500).collect();
                        snippet
                    }),
                };

                // Classify anomalies
                if status >= 500 {
                    result.is_anomaly = true;
                    result.reason = Some(format!("Server error: HTTP {}", status));
                } else if status == 0 {
                    result.is_anomaly = true;
                    result.reason = Some("Connection refused or timeout".to_string());
                }

                // Check for information leaks in the response body
                if let Some(ref text) = response_text
                    && let Some(leak_reason) = detect_leak(text)
                {
                    result.is_anomaly = true;
                    result.reason = match result.reason {
                        Some(existing) => Some(format!("{}; {}", existing, leak_reason)),
                        None => Some(leak_reason),
                    };
                }

                result
            }
            Err(e) => FuzzResult {
                mutator: mutator_name.to_string(),
                resource_type: resource_type.to_string(),
                iteration,
                method: method.to_string(),
                url,
                status_code: 0,
                is_anomaly: true,
                reason: Some(format!("Request failed: {}", e)),
                duration_ms,
                body_size,
                response_size: 0,
                response_snippet: None,
            },
        }
    }
}

/// Scan response body text for information leak patterns.
/// Returns a description of the first leak found, or None.
fn detect_leak(body: &str) -> Option<String> {
    for (label, pattern) in LEAK_PATTERNS {
        if body.contains(pattern) {
            return Some(format!("Information leak detected: {}", label));
        }
    }

    // Additional heuristic checks
    if body.contains("Exception") && (body.contains("at ") || body.contains("line ")) {
        return Some("Information leak detected: exception_with_stack".to_string());
    }
    if body.contains("Error") && body.contains("line ") && body.contains(".php") {
        return Some("Information leak detected: php_error".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_java_stack_trace() {
        let body = "java.lang.NullPointerException\n\tat com.example.PatientService.getPatient(PatientService.java:42)";
        assert!(detect_leak(body).is_some());
    }

    #[test]
    fn detect_python_traceback() {
        let body =
            "Traceback (most recent call last):\n  File \"/app/server.py\", line 23, in handle";
        assert!(detect_leak(body).is_some());
    }

    #[test]
    fn detect_sql_error() {
        let body = "SQL error: Table 'fhir.patients' doesn't exist";
        assert!(detect_leak(body).is_some());
    }

    #[test]
    fn detect_path_disclosure() {
        let body = "File not found: /etc/fhir/config.yaml";
        assert!(detect_leak(body).is_some());
    }

    #[test]
    fn detect_rust_panic() {
        let body = "panicked at src/main.rs:100:";
        assert!(detect_leak(body).is_some());
    }

    #[test]
    fn clean_response_no_leak() {
        let body = r#"{"resourceType": "OperationOutcome", "issue": [{"severity": "error", "code": "invalid"}]}"#;
        assert!(detect_leak(body).is_none());
    }

    #[test]
    fn detect_html_error_page() {
        let body = "<html><head><title>500 Internal Server Error</title></head><body>";
        assert!(detect_leak(body).is_some());
    }

    // ── Integration tests with mock FHIR server ─────────────────────────

    /// Start the mock server on a random port and return the base URL.
    async fn setup_mock_server() -> String {
        let app = fhir_autotest::mock_server::create_mock_app();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{}/fhir", addr)
    }

    #[tokio::test]
    async fn send_fuzzed_post_returns_result() {
        let base_url = setup_mock_server().await;
        let runner = FuzzRunner::new(&base_url, 1, false);

        let body = serde_json::json!({"resourceType": "Patient", "name": [{"family": "Test"}]});
        let result = runner.send_fuzzed("Patient", &body, "boundary", 0).await;

        assert_eq!(result.method, "POST");
        assert_eq!(result.status_code, 201);
        assert!(!result.is_anomaly());
        assert!(result.response_size > 0);
    }

    #[tokio::test]
    async fn send_fuzzed_put_returns_result() {
        let base_url = setup_mock_server().await;
        let runner = FuzzRunner::new(&base_url, 1, false);

        let body = serde_json::json!({"resourceType": "Patient", "name": [{"family": "Test"}]});
        let result = runner
            .send_fuzzed_put("Patient", "test-put-id", &body, "put_boundary", 0)
            .await;

        assert_eq!(result.method, "PUT");
        assert_eq!(result.status_code, 201); // update-as-create
        assert!(!result.is_anomaly());
        assert!(result.response_size > 0);
    }

    #[tokio::test]
    async fn send_fuzzed_search_returns_result() {
        let base_url = setup_mock_server().await;
        let runner = FuzzRunner::new(&base_url, 1, false);

        let result = runner
            .send_fuzzed_search("Patient", "?name=Test", "search_param", 0)
            .await;

        assert_eq!(result.method, "GET");
        assert_eq!(result.status_code, 200);
        assert!(!result.is_anomaly());
        assert!(result.response_size > 0);
    }

    #[tokio::test]
    async fn send_fuzzed_post_with_invalid_body_flagged_as_anomaly() {
        let base_url = setup_mock_server().await;
        let runner = FuzzRunner::new(&base_url, 1, false);

        // Send an array — mock server returns 400, which is not 200/201/500
        // so it should NOT be flagged as anomaly (400 is expected rejection)
        let result = runner
            .send_fuzzed("Patient", &serde_json::json!([1, 2, 3]), "type_mismatch", 0)
            .await;

        assert_eq!(result.status_code, 400);
        assert!(!result.is_anomaly(), "400 should not be flagged as anomaly");
    }

    #[tokio::test]
    async fn send_fuzzed_dry_run_returns_zero_status() {
        let runner = FuzzRunner::new("http://localhost:1", 1, true);

        let body = serde_json::json!({"resourceType": "Patient"});
        let result = runner.send_fuzzed("Patient", &body, "boundary", 0).await;

        assert_eq!(result.status_code, 0);
        assert!(!result.is_anomaly());
        assert_eq!(result.duration_ms, 0);
    }

    #[tokio::test]
    async fn send_fuzzed_put_dry_run_returns_zero_status() {
        let runner = FuzzRunner::new("http://localhost:1", 1, true);

        let body = serde_json::json!({"resourceType": "Patient"});
        let result = runner
            .send_fuzzed_put("Patient", "id", &body, "put_boundary", 0)
            .await;

        assert_eq!(result.status_code, 0);
        assert!(!result.is_anomaly());
    }

    #[tokio::test]
    async fn send_fuzzed_search_dry_run_returns_zero_status() {
        let runner = FuzzRunner::new("http://localhost:1", 1, true);

        let result = runner
            .send_fuzzed_search("Patient", "?name=test", "search_param", 0)
            .await;

        assert_eq!(result.status_code, 0);
        assert!(!result.is_anomaly());
    }
}
