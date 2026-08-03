use serde::Serialize;

use crate::runner::FuzzResult;

/// Summary report of a fuzz run.
#[derive(Debug, Clone, Serialize)]
pub struct FuzzReport {
    pub total: usize,
    pub anomalies: usize,
    pub categories_used: Vec<String>,
    pub anomaly_details: Vec<FuzzResult>,
}

impl FuzzReport {
    pub fn new() -> Self {
        Self {
            total: 0,
            anomalies: 0,
            categories_used: Vec::new(),
            anomaly_details: Vec::new(),
        }
    }
}

impl Default for FuzzReport {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for FuzzReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== FHIR Fuzz Report ===")?;
        writeln!(f, "Total requests: {}", self.total)?;
        writeln!(f, "Anomalies: {}", self.anomalies)?;
        writeln!(f, "Categories: {}", self.categories_used.join(", "))?;
        writeln!(f)?;

        if self.anomaly_details.is_empty() {
            writeln!(f, "No anomalies detected.")?;
        } else {
            writeln!(f, "Anomaly details:")?;
            for (i, detail) in self.anomaly_details.iter().enumerate() {
                writeln!(
                    f,
                    "  {}. [{}] {} {} (HTTP {}) — {}",
                    i + 1,
                    detail.mutator,
                    detail.method,
                    detail.url,
                    detail.status_code,
                    detail.reason.as_deref().unwrap_or("unknown")
                )?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzz_report_new() {
        let report = FuzzReport::new();
        assert_eq!(report.total, 0);
        assert_eq!(report.anomalies, 0);
        assert!(report.categories_used.is_empty());
        assert!(report.anomaly_details.is_empty());
    }

    #[test]
    fn fuzz_report_default() {
        let report = FuzzReport::default();
        assert_eq!(report.total, 0);
        assert_eq!(report.anomalies, 0);
        assert!(report.categories_used.is_empty());
        assert!(report.anomaly_details.is_empty());
    }

    #[test]
    fn fuzz_report_display_no_anomalies() {
        let report = FuzzReport::new();
        let output = format!("{}", report);
        assert!(output.contains("=== FHIR Fuzz Report ==="));
        assert!(output.contains("Total requests: 0"));
        assert!(output.contains("Anomalies: 0"));
        assert!(output.contains("No anomalies detected."));
    }

    #[test]
    fn fuzz_report_display_with_anomalies() {
        let report = FuzzReport {
            total: 5,
            anomalies: 2,
            categories_used: vec!["boundary".to_string(), "encoding".to_string()],
            anomaly_details: vec![
                FuzzResult {
                    mutator: "boundary".to_string(),
                    resource_type: "Patient".to_string(),
                    iteration: 0,
                    method: "POST".to_string(),
                    url: "http://example.com/Patient".to_string(),
                    status_code: 500,
                    is_anomaly: true,
                    reason: Some("Server error: HTTP 500".to_string()),
                    duration_ms: 100,
                    body_size: 256,
                    response_size: 1024,
                    response_snippet: Some("error".to_string()),
                },
                FuzzResult {
                    mutator: "encoding".to_string(),
                    resource_type: "Observation".to_string(),
                    iteration: 1,
                    method: "PUT".to_string(),
                    url: "http://example.com/Observation/123".to_string(),
                    status_code: 0,
                    is_anomaly: true,
                    reason: None,
                    duration_ms: 5000,
                    body_size: 512,
                    response_size: 0,
                    response_snippet: None,
                },
            ],
        };
        let output = format!("{}", report);
        assert!(output.contains("Total requests: 5"));
        assert!(output.contains("Anomalies: 2"));
        assert!(output.contains("boundary, encoding"));
        assert!(output.contains("Anomaly details:"));
        assert!(output.contains("[boundary] POST http://example.com/Patient (HTTP 500)"));
        assert!(output.contains("Server error: HTTP 500"));
        assert!(output.contains("[encoding] PUT http://example.com/Observation/123 (HTTP 0)"));
        assert!(output.contains("unknown"));
    }

    #[test]
    fn fuzz_report_serialization() {
        let report = FuzzReport {
            total: 10,
            anomalies: 1,
            categories_used: vec!["boundary".to_string()],
            anomaly_details: vec![FuzzResult {
                mutator: "boundary".to_string(),
                resource_type: "Patient".to_string(),
                iteration: 0,
                method: "POST".to_string(),
                url: "http://example.com/Patient".to_string(),
                status_code: 500,
                is_anomaly: true,
                reason: Some("Server error: HTTP 500".to_string()),
                duration_ms: 100,
                body_size: 256,
                response_size: 1024,
                response_snippet: Some("error".to_string()),
            }],
        };
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"total\": 10"));
        assert!(json.contains("\"anomalies\": 1"));
        assert!(json.contains("\"boundary\""));
        assert!(json.contains("\"status_code\": 500"));
    }

    #[test]
    fn fuzz_report_debug_and_clone() {
        let report = FuzzReport::new();
        let _debug = format!("{:?}", report);
        let cloned = report.clone();
        assert_eq!(report.total, cloned.total);
    }
}
