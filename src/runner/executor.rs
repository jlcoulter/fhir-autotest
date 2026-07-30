use crate::config::models::{WriteEndpoint, default_concurrency, default_upload_method};
use crate::generate::model::*;
use anyhow::{Context, Result};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::Request, http::StatusCode, routing::any};
    use http_body_util::BodyExt;
    use std::sync::{Arc, Mutex};

    /// A recorded request captured by the test server.
    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        uri: String,
        headers: std::collections::HashMap<String, String>,
        body: Option<serde_json::Value>,
    }

    /// A test server that records requests and returns configurable responses.
    struct TestServer {
        addr: String,
        responses: Arc<Mutex<Vec<serde_json::Value>>>,
        recorded: Arc<Mutex<Vec<RecordedRequest>>>,
    }

    impl TestServer {
        async fn new() -> Self {
            let responses: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
            let recorded: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));

            let responses_clone = responses.clone();
            let recorded_clone = recorded.clone();

            let app = Router::new().route(
                "/{*path}",
                any(move |req: Request| {
                    let responses = responses_clone.clone();
                    let recorded = recorded_clone.clone();
                    async move {
                        // Record the request
                        let method = req.method().to_string();
                        let uri = req.uri().to_string();
                        let headers: std::collections::HashMap<String, String> = req
                            .headers()
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                            .collect();

                        // Collect body bytes
                        let (_parts, body) = req.into_parts();
                        let body_bytes = BodyExt::collect(body)
                            .await
                            .map(|collected| collected.to_bytes())
                            .unwrap_or_default();
                        let body: Option<serde_json::Value> = if body_bytes.is_empty() {
                            None
                        } else {
                            serde_json::from_slice(&body_bytes).ok()
                        };

                        recorded.lock().unwrap().push(RecordedRequest {
                            method,
                            uri,
                            headers,
                            body,
                        });

                        // Pop the next response or use default
                        let mut store = responses.lock().unwrap();
                        let response = store.pop().unwrap_or(serde_json::json!({
                            "resourceType": "Bundle",
                            "type": "searchset",
                            "entry": []
                        }));

                        // Allow the response to specify a custom status code via _status field
                        let status_code = response
                            .get("_status")
                            .and_then(|v| v.as_u64())
                            .map(|s| StatusCode::from_u16(s as u16).unwrap_or(StatusCode::OK))
                            .unwrap_or(StatusCode::OK);

                        (status_code, Json(response))
                    }
                }),
            );

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });

            TestServer {
                addr: format!("http://{}", addr),
                responses,
                recorded,
            }
        }

        fn push_response(&self, value: serde_json::Value) {
            self.responses.lock().unwrap().push(value);
        }

        fn last_request(&self) -> Option<RecordedRequest> {
            let store = self.recorded.lock().unwrap();
            store.last().cloned()
        }
    }

    fn make_test_case(
        name: &str,
        method: &str,
        url: &str,
        expected_status: u16,
        body: Option<serde_json::Value>,
    ) -> TestCase {
        TestCase {
            name: name.to_string(),
            kind: TestCaseKind::Interaction,
            interaction: Interaction::Read,
            resource_type: "Patient".to_string(),
            profile_url: None,
            request: HttpRequest {
                method: method.to_string(),
                url: url.to_string(),
                headers: HashMap::new(),
                body,
            },
            validation: ValidationSpec {
                expected_status,
                profile_url: None,
                required_elements: vec![],
                forbidden_elements: vec![],
                response_assertion: None,
            },
        }
    }

    #[tokio::test]
    async fn execute_get_request() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        let test = make_test_case("test_get", "GET", "/Patient/test-id", 200, None);
        let result = executor.execute_test(&test).await.unwrap();

        assert_eq!(result.status_code, 200);
        assert!(result.passed);

        let recorded = server.last_request().unwrap();
        assert_eq!(recorded.method, "GET");
        assert!(recorded.uri.contains("/Patient/test-id"));
    }

    #[tokio::test]
    async fn execute_post_request() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        let body = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}]
        });
        let test = make_test_case("test_post", "POST", "/Patient", 200, Some(body.clone()));
        let result = executor.execute_test(&test).await.unwrap();

        assert_eq!(result.status_code, 200);
        assert!(result.passed);

        let recorded = server.last_request().unwrap();
        assert_eq!(recorded.method, "POST");
        // Verify the body was sent
        assert_eq!(
            recorded
                .body
                .as_ref()
                .and_then(|b| b.get("resourceType"))
                .and_then(|v| v.as_str()),
            Some("Patient")
        );
    }

    #[tokio::test]
    async fn execute_test_passes_on_expected_status() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        // Server returns 200, expected_status is 200
        let test = make_test_case("test_pass", "GET", "/Patient/1", 200, None);
        let result = executor.execute_test(&test).await.unwrap();

        assert!(result.passed);
        assert_eq!(result.status_code, 200);
    }

    #[tokio::test]
    async fn execute_test_fails_on_wrong_status() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        // Push a 404 response
        server.push_response(serde_json::json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "not-found"}]
        }));

        // Override the default response: the test server always returns 200,
        // so we need a way to return a different status. We'll use a custom
        // route or push a response that the test server interprets differently.
        //
        // Actually, the test server always returns 200. To test wrong status,
        // we need the server to return a non-200. Let's use a different approach:
        // we'll make a second server that returns 404.
        //
        // For simplicity, let's just verify the sentinel logic by using
        // expected_status: 200 with a server that returns 200 (passes) vs
        // expected_status: 404 with a server that returns 200 (fails).
        let test = make_test_case("test_fail", "GET", "/Patient/missing", 404, None);
        let result = executor.execute_test(&test).await.unwrap();

        // Server returned 200, expected 404 → should fail
        assert!(!result.passed);
        assert_eq!(result.status_code, 200);
    }

    #[tokio::test]
    async fn execute_test_sentinel_zero_accepts_non_2xx() {
        // expected_status: 0, server returns 403 → should pass
        // We need a server that returns 403. Let's use a dedicated server
        // with a custom handler for this test.
        let app = Router::new().route(
            "/{*path}",
            any(|| async {
                (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "resourceType": "OperationOutcome",
                        "issue": [{"severity": "error", "code": "forbidden"}]
                    })),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let server_url = format!("http://{}", addr);

        let executor = TestExecutor::from_server_config(&server_url, HashMap::new()).unwrap();

        let test = make_test_case("test_sentinel_non_2xx", "GET", "/Patient/1", 0, None);
        let result = executor.execute_test(&test).await.unwrap();

        // expected_status=0, server returned 403 (non-2xx) → pass
        assert!(result.passed);
        assert_eq!(result.status_code, 403);
    }

    #[tokio::test]
    async fn execute_test_sentinel_zero_accepts_200_bundle() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        // Server returns 200 with a Bundle (default response)
        let test = make_test_case(
            "test_sentinel_bundle",
            "GET",
            "/Patient?unknown=foo",
            0,
            None,
        );
        let result = executor.execute_test(&test).await.unwrap();

        // expected_status=0, server returned 200 with Bundle → pass
        assert!(result.passed);
        assert_eq!(result.status_code, 200);
    }

    #[tokio::test]
    async fn execute_test_sentinel_zero_rejects_200_non_bundle() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        // Push a non-Bundle response
        server.push_response(serde_json::json!({
            "resourceType": "Patient",
            "id": "test-id",
            "name": [{"family": "Test"}]
        }));

        let test = make_test_case("test_sentinel_non_bundle", "GET", "/Patient/1", 0, None);
        let result = executor.execute_test(&test).await.unwrap();

        // expected_status=0, server returned 200 with non-Bundle → fail
        assert!(!result.passed);
        assert_eq!(result.status_code, 200);
    }

    #[tokio::test]
    async fn create_resource_with_put() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        // Push a response that looks like a created resource
        server.push_response(serde_json::json!({
            "resourceType": "Patient",
            "id": "test-put-id",
            "name": [{"family": "Created"}]
        }));

        let body = serde_json::json!({
            "resourceType": "Patient",
            "id": "test-put-id",
            "name": [{"family": "Created"}]
        });

        let (id, _response) = executor.create_resource("Patient", &body).await.unwrap();

        assert_eq!(id, "test-put-id");

        let recorded = server.last_request().unwrap();
        assert_eq!(recorded.method, "PUT");
        assert!(recorded.uri.contains("/Patient/test-put-id"));
    }

    #[tokio::test]
    async fn create_resource_with_post() {
        let server = TestServer::new().await;
        // Use a custom config with POST upload method
        let executor = TestExecutor::new(
            server.addr.clone(),
            HashMap::new(),
            WriteEndpoint::Server {
                base_url: server.addr.clone(),
                headers: HashMap::new(),
                upload_method: "POST".to_string(),
                concurrency: 1,
            },
        )
        .unwrap();

        // Push a response that looks like a created resource with 201 status
        server.push_response(serde_json::json!({
            "_status": 201,
            "resourceType": "Patient",
            "id": "test-post-id",
            "name": [{"family": "Created"}]
        }));

        let body = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Created"}]
        });

        let (id, _response) = executor.create_resource("Patient", &body).await.unwrap();

        assert_eq!(id, "test-post-id");

        let recorded = server.last_request().unwrap();
        assert_eq!(recorded.method, "POST");
        assert!(recorded.uri.contains("/Patient"));
    }

    #[tokio::test]
    async fn create_resource_put_missing_id_errors() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        let body = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "NoId"}]
        });

        let result = executor.create_resource("Patient", &body).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("id") || err.contains("PUT"));
    }

    #[tokio::test]
    async fn delete_resource() {
        let server = TestServer::new().await;
        let executor = TestExecutor::from_server_config(&server.addr, HashMap::new()).unwrap();

        let result = executor.delete_resource("Patient", "test-del-id").await;
        assert!(result.is_ok());

        let recorded = server.last_request().unwrap();
        assert_eq!(recorded.method, "DELETE");
        assert!(recorded.uri.contains("/Patient/test-del-id"));
    }

    #[tokio::test]
    async fn add_read_auth_headers() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer test-token".to_string());
        headers.insert("X-Custom".to_string(), "custom-value".to_string());

        let server = TestServer::new().await;
        let executor = TestExecutor::new(
            server.addr.clone(),
            headers,
            WriteEndpoint::Server {
                base_url: server.addr.clone(),
                headers: HashMap::new(),
                upload_method: "PUT".to_string(),
                concurrency: 1,
            },
        )
        .unwrap();

        let test = make_test_case("test_auth", "GET", "/Patient/1", 200, None);
        let _result = executor.execute_test(&test).await.unwrap();

        let recorded = server.last_request().unwrap();
        assert_eq!(
            recorded.headers.get("authorization").map(|s| s.as_str()),
            Some("Bearer test-token")
        );
        assert_eq!(
            recorded.headers.get("x-custom").map(|s| s.as_str()),
            Some("custom-value")
        );
    }

    #[tokio::test]
    async fn add_write_auth_basic_auth() {
        let server = TestServer::new().await;
        let executor = TestExecutor::new(
            server.addr.clone(),
            HashMap::new(),
            WriteEndpoint::Repository {
                base_url: server.addr.clone(),
                username: "admin".to_string(),
                password: "s3cret".to_string(),
                upload_method: "PUT".to_string(),
                concurrency: 1,
            },
        )
        .unwrap();

        // Push a response for the PUT
        server.push_response(serde_json::json!({
            "resourceType": "Patient",
            "id": "test-auth-id",
            "name": [{"family": "Auth"}]
        }));

        let body = serde_json::json!({
            "resourceType": "Patient",
            "id": "test-auth-id",
            "name": [{"family": "Auth"}]
        });

        let _result = executor.create_resource("Patient", &body).await.unwrap();

        let recorded = server.last_request().unwrap();
        // Basic auth header should be present
        let auth_header = recorded.headers.get("authorization");
        assert!(auth_header.is_some());
        let auth_value = auth_header.unwrap();
        assert!(auth_value.starts_with("Basic "));
    }
}

/// Executes HTTP requests against FHIR servers.
///
/// Test queries (GET) go to the public FHIR server.
/// Write operations (POST/PUT/DELETE) go to the repository (if configured)
/// or fall back to the public server.
pub struct TestExecutor {
    client: reqwest::Client,
    /// Base URL for read/search requests (the public FHIR server).
    read_url: String,
    /// Headers to send with read requests.
    read_headers: HashMap<String, String>,
    /// Write endpoint configuration (repository with basic auth, or server with headers).
    write_endpoint: WriteEndpoint,
}

/// Result of a single test case execution.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TestResult {
    pub test_name: String,
    pub passed: bool,
    pub status_code: u16,
    pub response_body: Option<serde_json::Value>,
    pub validation_errors: Vec<String>,
    /// The full request URL that was executed.
    #[serde(default)]
    pub request_url: String,
    /// The HTTP method used.
    #[serde(default)]
    pub request_method: String,
    /// The request body (for POST/PUT).
    #[serde(default)]
    pub request_body: Option<serde_json::Value>,
    /// The test group this result belongs to (e.g. "Patient", "_conformance").
    #[serde(default)]
    pub test_group: String,
}

impl TestExecutor {
    /// Create a new executor with separate read and write endpoints.
    pub fn new(
        read_url: String,
        read_headers: HashMap<String, String>,
        write_endpoint: WriteEndpoint,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            read_url: read_url.trim_end_matches('/').to_string(),
            read_headers,
            write_endpoint,
        })
    }

    /// Convenience constructor that uses the same server for reads and writes
    /// (backward-compatible with the old ServerConfig-only approach).
    pub fn from_server_config(base_url: &str, headers: HashMap<String, String>) -> Result<Self> {
        Self::new(
            base_url.to_string(),
            headers.clone(),
            WriteEndpoint::Server {
                base_url: base_url.to_string(),
                headers,
                upload_method: default_upload_method(),
                concurrency: default_concurrency(),
            },
        )
    }

    /// Build a reqwest request with the appropriate auth for the given endpoint.
    fn add_write_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.write_endpoint {
            WriteEndpoint::Repository {
                username, password, ..
            } => req.basic_auth(username.clone(), Some(password.clone())),
            WriteEndpoint::Server { headers, .. } => {
                let mut r = req;
                for (key, value) in headers {
                    r = r.header(key.as_str(), value.as_str());
                }
                r
            }
        }
    }

    fn add_read_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut r = req;
        for (key, value) in &self.read_headers {
            r = r.header(key.as_str(), value.as_str());
        }
        r
    }

    fn write_base_url(&self) -> &str {
        match &self.write_endpoint {
            WriteEndpoint::Repository { base_url, .. } => base_url,
            WriteEndpoint::Server { base_url, .. } => base_url,
        }
    }

    /// Execute a single test case against the server.
    ///
    /// Test requests are sent to the read endpoint (public FHIR server).
    pub async fn execute_test(&self, test: &TestCase) -> Result<TestResult> {
        let url = format!(
            "{}/{}",
            self.read_url.trim_end_matches('/'),
            test.request.url.trim_start_matches('/')
        );

        let mut req = match test.request.method.as_str() {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            "PUT" => self.client.put(&url),
            "DELETE" => self.client.delete(&url),
            "PATCH" => self.client.patch(&url),
            method => anyhow::bail!("Unsupported HTTP method: {}", method),
        };

        req = self.add_read_auth(req);
        req = req
            .header("Content-Type", "application/fhir+json")
            .header("Accept", "application/fhir+json");

        if let Some(body) = &test.request.body {
            req = req.json(body);
        }

        let resp = req
            .send()
            .await
            .with_context(|| format!("Failed to execute test: {}", test.name))?;

        let status = resp.status().as_u16();
        let body: Option<serde_json::Value> = resp.json().await.ok();

        // expected_status == 0 is a sentinel meaning "expect non-2xx"
        // (used by negative conformance tests for undeclared interactions/params)
        //
        // For undeclared search params, the FHIR spec allows servers to ignore
        // unknown parameters and return a filtered Bundle (200 OK). So we accept
        // both outcomes:
        //   - Non-2xx status (server rejected the request) → pass
        //   - 200 with a Bundle (server ignored the unknown param) → pass
        // For undeclared interactions (read/vread/update/etc.), only non-2xx passes.
        let passed = if test.validation.expected_status == 0 {
            if !(200..=299).contains(&status) {
                // Server rejected — always passes for negative tests
                true
            } else if let Some(body) = &body {
                // Server returned 2xx — check if it's a Bundle (acceptable for search params)
                body.get("resourceType").and_then(|v| v.as_str()) == Some("Bundle")
            } else {
                // 2xx with no parseable body — fails
                false
            }
        } else {
            status == test.validation.expected_status
        };

        Ok(TestResult {
            test_name: test.name.clone(),
            passed,
            status_code: status,
            response_body: body,
            validation_errors: Vec::new(),
            request_url: url,
            request_method: test.request.method.clone(),
            request_body: test.request.body.clone(),
            test_group: String::new(), // stamped by orchestrator
        })
    }

    /// Create a resource on the repository.
    ///
    /// Uses PUT (update-as-create) when `upload_method` is "PUT", which sends
    /// `PUT /{resource_type}/{id}` with the ID included in the resource body.
    /// Uses POST (server-assigned ID) when `upload_method` is "POST".
    ///
    /// Returns the resource's ID and the full response body.
    pub async fn create_resource(
        &self,
        resource_type: &str,
        body: &serde_json::Value,
    ) -> Result<(String, serde_json::Value)> {
        let method = match &self.write_endpoint {
            WriteEndpoint::Repository { upload_method, .. }
            | WriteEndpoint::Server { upload_method, .. } => upload_method.to_uppercase(),
        };

        if method == "PUT" {
            // PUT (update-as-create): include client-assigned ID in URL and body
            let id = body.get("id").and_then(|v| v.as_str()).ok_or_else(|| {
                anyhow::anyhow!("PUT upload requires resource to have an 'id' field")
            })?;
            let url = format!("{}/{}/{}", self.write_base_url(), resource_type, id);

            let req = self.client.put(&url);
            let req = self.add_write_auth(req);
            let req = req
                .header("Content-Type", "application/fhir+json")
                .header("Accept", "application/fhir+json")
                .json(body);

            let resp = req
                .send()
                .await
                .with_context(|| format!("Failed to PUT {}", resource_type))?;
            let status = resp.status();
            let created: serde_json::Value = resp
                .json()
                .await
                .context("Failed to parse PUT resource response")?;
            if status.as_u16() != 200 && status.as_u16() != 201 {
                anyhow::bail!(
                    "Expected 200/201 for PUT {}, got {}: {:?}",
                    resource_type,
                    status.as_u16(),
                    created
                );
            }

            let id = created
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("PUT response missing id"))?
                .to_string();
            Ok((id, created))
        } else {
            // POST: server-assigned ID — remove client id before sending
            let url = format!("{}/{}", self.write_base_url(), resource_type);

            let req = self.client.post(&url);
            let req = self.add_write_auth(req);
            let req = req
                .header("Content-Type", "application/fhir+json")
                .header("Accept", "application/fhir+json")
                .json(body);

            let resp = req
                .send()
                .await
                .with_context(|| format!("Failed to create {}", resource_type))?;
            let status = resp.status();
            let created: serde_json::Value = resp
                .json()
                .await
                .context("Failed to parse created resource response")?;
            if status.as_u16() != 201 {
                anyhow::bail!(
                    "Expected 201 Created for {}, got {}: {:?}",
                    resource_type,
                    status.as_u16(),
                    created
                );
            }

            let id = created
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Created resource missing id"))?
                .to_string();
            Ok((id, created))
        }
    }

    /// Delete a resource from the repository.
    ///
    /// Returns `Ok(())` on success or if the resource is already gone (404).
    /// Returns an error for any other non-2xx response.
    pub async fn delete_resource(&self, resource_type: &str, id: &str) -> Result<()> {
        let url = format!("{}/{}/{}", self.write_base_url(), resource_type, id);

        let req = self.client.delete(&url);
        let req = self.add_write_auth(req);
        let req = req.header("Accept", "application/fhir+json");

        let resp = req
            .send()
            .await
            .with_context(|| format!("Failed to delete {}/{}", resource_type, id))?;

        let status = resp.status();
        if !status.is_success() && status != reqwest::StatusCode::NOT_FOUND {
            anyhow::bail!("DELETE {}/{} returned {}", resource_type, id, status);
        }

        Ok(())
    }
}
