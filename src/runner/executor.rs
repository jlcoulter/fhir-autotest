use crate::config::models::WriteEndpoint;
use crate::generate::model::*;
use anyhow::{Context, Result};
use std::collections::HashMap;

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
        let url = format!("{}{}", self.read_url, test.request.url);

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

        Ok(TestResult {
            test_name: test.name.clone(),
            passed: status == test.validation.expected_status,
            status_code: status,
            response_body: body,
            validation_errors: Vec::new(),
            request_url: url,
            request_method: test.request.method.clone(),
            request_body: test.request.body.clone(),
        })
    }

    /// Create a resource on the repository (POST to /{resource_type}).
    /// Returns the created resource's ID and the full response body.
    pub async fn create_resource(
        &self,
        resource_type: &str,
        body: &serde_json::Value,
    ) -> Result<(String, serde_json::Value)> {
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

    /// Delete a resource from the repository.
    pub async fn delete_resource(&self, resource_type: &str, id: &str) -> Result<()> {
        let url = format!("{}/{}/{}", self.write_base_url(), resource_type, id);

        let req = self.client.delete(&url);
        let req = self.add_write_auth(req);
        let req = req.header("Accept", "application/fhir+json");

        req.send()
            .await
            .with_context(|| format!("Failed to delete {}/{}", resource_type, id))?;

        Ok(())
    }
}
