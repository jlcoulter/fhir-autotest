use crate::generate::model::*;
use crate::config::models::ServerConfig;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Executes HTTP requests against a FHIR server.
pub struct TestExecutor {
    client: reqwest::Client,
    base_url: String,
    headers: HashMap<String, String>,
}

/// Result of a single test case execution.
#[derive(Debug)]
pub struct TestResult {
    pub test_name: String,
    pub passed: bool,
    pub status_code: u16,
    pub response_body: Option<serde_json::Value>,
    pub validation_errors: Vec<String>,
}

impl TestExecutor {
    pub fn new(config: &ServerConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            headers: config.headers.clone(),
        })
    }

    /// Execute a single test case against the server.
    pub async fn execute_test(&self, test: &TestCase) -> Result<TestResult> {
        let url = format!("{}{}", self.base_url, test.request.url);

        let mut req = match test.request.method.as_str() {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            "PUT" => self.client.put(&url),
            "DELETE" => self.client.delete(&url),
            "PATCH" => self.client.patch(&url),
            method => anyhow::bail!("Unsupported HTTP method: {}", method),
        };

        for (key, value) in &self.headers {
            req = req.header(key.as_str(), value.as_str());
        }
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
        })
    }

    /// Create a resource on the server (POST to /{resource_type}).
    /// Returns the created resource's ID and the full response body.
    pub async fn create_resource(
        &self,
        resource_type: &str,
        body: &serde_json::Value,
    ) -> Result<(String, serde_json::Value)> {
        let url = format!("{}/{}", self.base_url, resource_type);

        let mut req = self.client.post(&url);
        for (key, value) in &self.headers {
            req = req.header(key.as_str(), value.as_str());
        }
        req = req
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

    /// Delete a resource from the server.
    pub async fn delete_resource(&self, resource_type: &str, id: &str) -> Result<()> {
        let url = format!("{}/{}/{}", self.base_url, resource_type, id);

        let mut req = self.client.delete(&url);
        for (key, value) in &self.headers {
            req = req.header(key.as_str(), value.as_str());
        }
        req = req
            .header("Accept", "application/fhir+json");

        req.send()
            .await
            .with_context(|| format!("Failed to delete {}/{}", resource_type, id))?;

        Ok(())
    }
}