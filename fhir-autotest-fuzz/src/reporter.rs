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
