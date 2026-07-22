use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Supported FHIR RESTful interactions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Interaction {
    Read,
    Vread,
    Update,
    Patch,
    Delete,
    Create,
    SearchType,
    HistoryInstance,
    HistoryType,
    Operation(String),
}

impl Interaction {
    /// Parse a FHIR interaction code string into an Interaction variant.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "read" => Some(Interaction::Read),
            "vread" => Some(Interaction::Vread),
            "update" => Some(Interaction::Update),
            "patch" => Some(Interaction::Patch),
            "delete" => Some(Interaction::Delete),
            "create" => Some(Interaction::Create),
            "search-type" => Some(Interaction::SearchType),
            "history-instance" => Some(Interaction::HistoryInstance),
            "history-type" => Some(Interaction::HistoryType),
            other => Some(Interaction::Operation(other.to_string())),
        }
    }

    /// Get the HTTP method for this interaction.
    pub fn http_method(&self) -> &'static str {
        match self {
            Interaction::Read
            | Interaction::Vread
            | Interaction::SearchType
            | Interaction::HistoryInstance
            | Interaction::HistoryType => "GET",
            Interaction::Create => "POST",
            Interaction::Update => "PUT",
            Interaction::Patch => "PATCH",
            Interaction::Delete => "DELETE",
            Interaction::Operation(_) => "POST",
        }
    }

    /// Get a human-readable label for this interaction.
    pub fn label(&self) -> String {
        match self {
            Interaction::Operation(name) => format!("operation-{name}"),
            other => format!("{:?}", other).to_lowercase(),
        }
    }
}

/// An HTTP request template for a test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: Option<serde_json::Value>,
}

/// Validation specification for a test case response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSpec {
    pub expected_status: u16,
    pub profile_url: Option<String>,
    #[serde(default)]
    pub required_elements: Vec<String>,
    #[serde(default)]
    pub forbidden_elements: Vec<String>,
}

/// A single test case: one request + its validation criteria.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub name: String,
    pub interaction: Interaction,
    pub resource_type: String,
    pub profile_url: Option<String>,
    pub request: HttpRequest,
    pub validation: ValidationSpec,
}

/// A group of test cases for one resource type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestGroup {
    pub resource_type: String,
    pub profile_url: Option<String>,
    pub tests: Vec<TestCase>,
}

/// The full test plan generated from an IG package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPlan {
    pub name: String,
    pub ig_url: Option<String>,
    #[serde(default)]
    pub test_groups: Vec<TestGroup>,
    #[serde(default)]
    pub creation_order: Vec<String>,
    #[serde(default)]
    pub setup_resources: HashMap<String, Vec<serde_json::Value>>,
}

impl TestPlan {
    pub fn total_tests(&self) -> usize {
        self.test_groups.iter().map(|g| g.tests.len()).sum()
    }
}