use serde::Serialize;
use std::time::Instant;

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
    /// Size of the request body in bytes.
    pub body_size: usize,
}

impl FuzzResult {
    pub fn is_anomaly(&self) -> bool {
        self.is_anomaly
    }
}

/// Sends fuzzed FHIR resources to the target server and records responses.
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
            };
        }

        let start = Instant::now();
        let response = self.client.post(&url).json(body).send().await;
        let duration_ms = start.elapsed().as_millis() as u64;

        match response {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let mut result = FuzzResult {
                    mutator: mutator_name.to_string(),
                    resource_type: resource_type.to_string(),
                    iteration,
                    method: "POST".to_string(),
                    url,
                    status_code: status,
                    is_anomaly: false,
                    reason: None,
                    duration_ms,
                    body_size,
                };

                // Classify anomalies
                if status >= 500 {
                    result.is_anomaly = true;
                    result.reason = Some(format!("Server error: HTTP {}", status));
                } else if status == 200 || status == 201 {
                    // A 200/201 on clearly invalid data is suspicious
                    result.is_anomaly = true;
                    result.reason = Some(format!(
                        "Accepted potentially invalid data: HTTP {}",
                        status
                    ));
                } else if status == 0 {
                    result.is_anomaly = true;
                    result.reason = Some("Connection refused or timeout".to_string());
                }

                result
            }
            Err(e) => FuzzResult {
                mutator: mutator_name.to_string(),
                resource_type: resource_type.to_string(),
                iteration,
                method: "POST".to_string(),
                url,
                status_code: 0,
                is_anomaly: true,
                reason: Some(format!("Request failed: {}", e)),
                duration_ms,
                body_size,
            },
        }
    }
}
