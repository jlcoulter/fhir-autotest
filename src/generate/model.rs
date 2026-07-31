use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What to assert about a FHIR server response.
///
/// Each test case carries one of these to describe what the response MUST contain
/// beyond just the HTTP status code. The orchestrator evaluates these after execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseAssertion {
    /// The response MUST be a Bundle of this type (e.g. "searchset", "batch").
    #[serde(default)]
    pub bundle_type: Option<String>,

    /// The response Bundle MUST contain at least this many entries.
    #[serde(default)]
    pub min_entries: Option<usize>,

    /// The response Bundle MUST contain at most this many entries.
    #[serde(default)]
    pub max_entries: Option<usize>,

    /// The response Bundle entries MUST include at least one resource of each
    /// listed resource type (e.g. ["Patient", "Observation"]).
    #[serde(default)]
    pub resource_types: Vec<String>,

    /// The response MUST contain entries whose fields match these values.
    /// Keyed by resource type → field path → expected value.
    /// Example: { "Patient": { "name.family": "GeneratedFamily" } }
    #[serde(default)]
    pub field_values: HashMap<String, HashMap<String, serde_json::Value>>,

    /// For _include/_revinclude: the Bundle MUST contain resources of these
    /// types that were included via the parameter.
    #[serde(default)]
    pub include_types: HashMap<String, String>,

    /// For _include/_revinclude checks where the target resource type is not
    /// fixed (e.g., Provenance:target), require at least one non-primary
    /// resource type when primary resources are present.
    #[serde(default)]
    pub include_requires_distinct_from: Option<String>,

    /// For _sort: entries MUST be sorted by this field in this direction.
    #[serde(default)]
    pub sort_by: Option<SortAssertion>,

    /// For _summary=true: returned resources MUST NOT have the listed fields.
    #[serde(default)]
    pub absent_fields: Vec<String>,

    /// For negative/error tests: response MUST be an OperationOutcome
    /// with at least one issue whose severity matches.
    #[serde(default)]
    pub outcome_severity: Option<String>,

    /// For mustSupport conformance: these field paths MUST be present in
    /// Bundle entry resources, regardless of their value.
    /// Keyed by resource type → list of field paths.
    #[serde(default)]
    pub required_fields: HashMap<String, Vec<String>>,

    /// For $operation: response MUST contain this top-level key.
    #[serde(default)]
    pub response_contains_key: Option<String>,

    /// Top-level response resourceType MUST be one of these values.
    /// Useful for operation responses that may legally return one of several
    /// resource types (e.g. Parameters, Bundle, OperationOutcome).
    #[serde(default)]
    pub response_resource_types: Vec<String>,
}

/// Sort direction assertion for _sort tests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortAssertion {
    pub field: String,
    pub direction: String, // "asc" or "desc"
}

impl ResponseAssertion {
    /// Build a default (empty) assertion — only status code is checked.
    pub fn none() -> Self {
        Self {
            bundle_type: None,
            min_entries: None,
            max_entries: None,
            resource_types: Vec::new(),
            field_values: HashMap::new(),
            include_types: HashMap::new(),
            include_requires_distinct_from: None,
            sort_by: None,
            absent_fields: Vec::new(),
            outcome_severity: None,
            required_fields: HashMap::new(),
            response_contains_key: None,
            response_resource_types: Vec::new(),
        }
    }
}

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
            other => {
                tracing::warn!(
                    "Unknown interaction code '{}', treating as operation",
                    other
                );
                Some(Interaction::Operation(other.to_string()))
            }
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

/// Search parameter modifiers per FHIR R4 spec.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchModifier {
    Exact,
    Contains,
    Missing,
    Not,
    Above,
    Below,
    Text,
    In,
    NotIn,
    BelowType, // below for type-hierarchical params
    AboveType, // above for type-hierarchical params
}

impl SearchModifier {
    /// Which modifiers apply to a given search param type.
    pub fn applicable_to(param_type: &str) -> Vec<SearchModifier> {
        let mut modifiers = vec![SearchModifier::Missing]; // valid for all types
        match param_type {
            "string" => modifiers.extend([SearchModifier::Exact, SearchModifier::Contains]),
            "token" => modifiers.extend([SearchModifier::Not, SearchModifier::Text]),
            "reference" => {
                // :above/:below are only valid for hierarchical references
                // (containment hierarchy). Most servers reject them on
                // arbitrary reference params, so we omit them by default.
            }
            "uri" => {
                // :above/:below are only valid for hierarchical URI schemes.
                // Most servers reject them on arbitrary URI params.
            }
            _ => {}
        }
        modifiers
    }

    pub fn suffix(&self) -> &str {
        match self {
            SearchModifier::Exact => ":exact",
            SearchModifier::Contains => ":contains",
            SearchModifier::Missing => ":missing",
            SearchModifier::Not => ":not",
            SearchModifier::Above => ":above",
            SearchModifier::Below => ":below",
            SearchModifier::Text => ":text",
            SearchModifier::In => ":in",
            SearchModifier::NotIn => ":not-in",
            SearchModifier::BelowType => ":below",
            SearchModifier::AboveType => ":above",
        }
    }
}

/// Search comparison prefixes for number/date/quantity params.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchPrefix {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    Sa,
    Eb,
    Ap,
}

impl SearchPrefix {
    /// Which prefixes apply to a given search param type.
    pub fn applicable_to(param_type: &str) -> Vec<SearchPrefix> {
        match param_type {
            "number" | "quantity" => vec![
                SearchPrefix::Eq,
                SearchPrefix::Ne,
                SearchPrefix::Gt,
                SearchPrefix::Lt,
                SearchPrefix::Ge,
                SearchPrefix::Le,
            ],
            "date" | "dateTime" => vec![
                SearchPrefix::Eq,
                SearchPrefix::Ne,
                SearchPrefix::Gt,
                SearchPrefix::Lt,
                SearchPrefix::Ge,
                SearchPrefix::Le,
                SearchPrefix::Sa,
                SearchPrefix::Eb,
                SearchPrefix::Ap,
            ],
            _ => vec![],
        }
    }

    pub fn prefix_str(&self) -> &str {
        match self {
            SearchPrefix::Eq => "eq",
            SearchPrefix::Ne => "ne",
            SearchPrefix::Gt => "gt",
            SearchPrefix::Lt => "lt",
            SearchPrefix::Ge => "ge",
            SearchPrefix::Le => "le",
            SearchPrefix::Sa => "sa",
            SearchPrefix::Eb => "eb",
            SearchPrefix::Ap => "ap",
        }
    }
}

/// What kind of test case this is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TestCaseKind {
    /// Basic CRUD / interaction test (read, create, etc.)
    Interaction,
    /// Single search param test
    SearchSingle {
        param_name: String,
        param_type: String,
    },
    /// Search param with modifier test
    SearchModifier {
        param_name: String,
        modifier: SearchModifier,
    },
    /// Search param with prefix test (for number/date/quantity)
    SearchPrefix {
        param_name: String,
        prefix: SearchPrefix,
    },
    /// Proximity/near search test (FHIR special type: lat:lon[:distance[:units]])
    SearchNear { param_name: String },
    /// Combinatorial search: multiple params combined
    SearchCombo { params: Vec<String> },
    /// Chained search: reference param chained into target param
    SearchChained {
        chain_param: String,
        target_param: String,
    },
    /// _include / _revinclude test
    Include { param: String, revinclude: bool },
    /// Result parameter test (_summary, _elements, _count, _sort, _has, etc.)
    ResultParam { param: String },
    /// $operation test
    Operation { code: String },
    /// Negative / error test
    Negative { description: String },
    /// Conformance test: verifies responder obligations from the CapabilityStatement
    Conformance { description: String },
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
    /// Structured assertions about the response body beyond status code.
    #[serde(default)]
    pub response_assertion: Option<ResponseAssertion>,
}

/// A single test case: one request + its validation criteria.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub name: String,
    pub kind: TestCaseKind,
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
}

impl TestPlan {
    pub fn total_tests(&self) -> usize {
        self.test_groups.iter().map(|g| g.tests.len()).sum()
    }
}
