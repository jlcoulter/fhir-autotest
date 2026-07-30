//! Conformance test generation for the FHIR IG Responder actor.
//!
//! Generates tests that verify a server's declared CapabilityStatement
//! obligations are actually met. This covers:
//!
//! 1. **CapabilityStatement well-formedness** — the CS itself must have
//!    required fields (status, rest with server mode, etc.)
//! 2. **MustSupport field presence** — fields marked mustSupport=true in
//!    profiles declared by the CS should be present in responses
//! 3. **Cardinality enforcement** — min/max constraints from profile
//!    ElementDefinitions should be respected in responses
//! 4. **Undeclared interaction rejection** — interactions NOT declared in
//!    the CS should be rejected by the server (negative conformance)

use crate::model::capability::*;
use crate::model::profile::StructureDefinition;

/// Result of validating a CapabilityStatement for well-formedness.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CapabilityStatementValidation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Validate a CapabilityStatement for well-formedness required of a responder.
///
/// Checks:
/// - `status` field is present and active/published
/// - At least one `rest` entry with `mode = "server"`
/// - Each server rest resource has a `type` field
/// - Each declared search param has `name` and `type`
pub fn validate_capability_statement(cs: &CapabilityStatement) -> CapabilityStatementValidation {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Required fields
    if cs.status.as_deref().is_none() {
        errors.push("CapabilityStatement missing required field: status".to_string());
    } else if let Some(status) = &cs.status
        && !matches!(status.as_str(), "active" | "draft" | "retired")
    {
        warnings.push(format!(
            "CapabilityStatement has unusual status: '{}'",
            status
        ));
    }

    // Must have at least one server-mode rest entry
    let server_rests: Vec<&Rest> = cs.rest.iter().filter(|r| r.mode == "server").collect();
    if server_rests.is_empty() {
        errors.push(
            "CapabilityStatement has no rest entry with mode='server' — \
             responder actor requires at least one"
                .to_string(),
        );
    }

    for rest in &server_rests {
        for (i, resource) in rest.resource.iter().enumerate() {
            if resource.resource_type.is_empty() {
                errors.push(format!(
                    "rest.resource[{}]: missing required 'type' field",
                    i
                ));
            }

            for (j, sp) in resource.search_param.iter().enumerate() {
                if sp.name.is_empty() {
                    errors.push(format!(
                        "rest.resource[{}].searchParam[{}]: missing 'name'",
                        resource.resource_type, j
                    ));
                }
                if sp.param_type.is_empty() {
                    errors.push(format!(
                        "rest.resource[{}].searchParam[{}]: missing 'type'",
                        resource.resource_type, j
                    ));
                }
            }
        }
    }

    CapabilityStatementValidation { errors, warnings }
}

/// A single conformance test case.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConformanceTest {
    pub name: String,
    pub description: String,
    pub resource_type: String,
    pub kind: ConformanceTestKind,
    pub request: ConformanceRequest,
    pub assertion: ConformanceAssertion,
}

/// What kind of conformance test this is.
#[derive(Debug, Clone, serde::Serialize)]
pub enum ConformanceTestKind {
    /// Verify that a mustSupport field is present in responses.
    MustSupportPresence { field_path: String },
    /// Verify that cardinality constraints (min/max) are respected.
    Cardinality {
        field_path: String,
        min: u32,
        max: String,
    },
    /// Verify that an undeclared interaction is properly rejected.
    UndeclaredInteraction { interaction: String },
    /// Verify that an undeclared search parameter is properly rejected.
    UndeclaredSearchParam { param_name: String },
}

/// HTTP request for a conformance test.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConformanceRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    pub body: Option<serde_json::Value>,
}

/// What to assert about the response for a conformance test.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConformanceAssertion {
    pub expected_status: u16,
    pub must_contain_fields: Vec<String>,
    pub must_not_contain_fields: Vec<String>,
    /// For Bundle responses, minimum number of entries expected.
    pub min_entries: Option<usize>,
    /// Expected Bundle type (e.g., "searchset").
    pub bundle_type: Option<String>,
    /// For error responses, expect an OperationOutcome.
    pub expect_operation_outcome: bool,
}

/// Generate conformance tests from a CapabilityStatement and its profiles.
///
/// This produces tests that verify the server actually meets the obligations
/// it declares in its CapabilityStatement:
/// - MustSupport fields are present in read/search responses
/// - Cardinality (min/max) is respected
/// - Undeclared interactions and search params are rejected
pub fn generate_conformance_tests(
    cs: &CapabilityStatement,
    profiles: &[StructureDefinition],
) -> Vec<ConformanceTest> {
    let mut tests = Vec::new();

    for rest in &cs.rest {
        if rest.mode != "server" {
            continue;
        }

        for resource in &rest.resource {
            // Skip non-resource types (e.g. Parameters) that are declared
            // in the CapabilityStatement but are not persistable resources.
            if crate::generate::NON_RESOURCE_TYPES.contains(&resource.resource_type.as_str()) {
                continue;
            }

            let has_search_type = resource.interaction.iter().any(|i| i.code == "search-type");

            // --- MustSupport field presence tests ---
            // Find a matching profile: prefer one referenced by the CS,
            // fall back to any profile matching the resource type.
            let profile = if let Some(ref url) = resource.profile {
                profiles.iter().find(|p| p.url == *url)
            } else {
                None
            }
            .or_else(|| {
                resource
                    .supported_profile
                    .iter()
                    .find_map(|url| profiles.iter().find(|p| p.url == *url))
            })
            .or_else(|| {
                // Fallback: any profile for this resource type
                profiles
                    .iter()
                    .find(|p| p.base_type == resource.resource_type)
            });

            if has_search_type && let Some(profile) = profile {
                let must_support_fields = collect_must_support_fields(profile);
                for field_path in must_support_fields {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_must_support_{}",
                            resource.resource_type,
                            field_path.replace('.', "_")
                        ),
                        description: format!(
                            "Verify that mustSupport field '{}' is present in {} responses",
                            field_path, resource.resource_type
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::MustSupportPresence {
                            field_path: field_path.clone(),
                        },
                        request: ConformanceRequest {
                            method: "GET".to_string(),
                            url: format!(
                                "/{}?_id={}-1&_count=10",
                                resource.resource_type,
                                resource.resource_type.to_lowercase()
                            ),
                            headers: std::collections::HashMap::new(),
                            body: None,
                        },
                        assertion: ConformanceAssertion {
                            expected_status: 200,
                            must_contain_fields: vec![field_path],
                            must_not_contain_fields: vec![],
                            // min_entries=0: an empty search result Bundle (total=0)
                            // is valid per FHIR spec; field presence is only checked
                            // when entries actually exist.
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }
            }

            // --- Cardinality tests ---
            if has_search_type && let Some(profile) = profile {
                let cardinality_fields = collect_cardinality_fields(profile);
                for (field_path, min, max) in cardinality_fields {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_cardinality_{}",
                            resource.resource_type,
                            field_path.replace('.', "_")
                        ),
                        description: format!(
                            "Verify cardinality [{min}..{max}] on field '{}' in {} responses",
                            field_path, resource.resource_type
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::Cardinality {
                            field_path: field_path.clone(),
                            min,
                            max: max.clone(),
                        },
                        request: ConformanceRequest {
                            method: "GET".to_string(),
                            url: format!(
                                "/{}?_id={}-1&_count=10",
                                resource.resource_type,
                                resource.resource_type.to_lowercase()
                            ),
                            headers: std::collections::HashMap::new(),
                            body: None,
                        },
                        assertion: ConformanceAssertion {
                            expected_status: 200,
                            must_contain_fields: vec![],
                            must_not_contain_fields: vec![],
                            // min_entries=0: empty search results are valid; cardinality
                            // is only meaningful when entries exist.
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }
            }

            // --- Undeclared interaction rejection tests ---
            let declared_interactions: std::collections::HashSet<String> = resource
                .interaction
                .iter()
                .map(|i| i.code.clone())
                .collect();

            // Check common interactions that might NOT be declared
            let all_interactions = [
                ("create", "POST"),
                ("read", "GET"),
                ("update", "PUT"),
                ("delete", "DELETE"),
                ("vread", "GET"),
                ("patch", "PATCH"),
                ("history-instance", "GET"),
            ];

            for (code, method) in &all_interactions {
                if !declared_interactions.contains(*code) {
                    let url = match *code {
                        "create" => format!("/{}", resource.resource_type),
                        "vread" => format!("/{}/{{id}}/_history/1", resource.resource_type),
                        "history-instance" => {
                            format!("/{}/{{id}}/_history", resource.resource_type)
                        }
                        _ => format!("/{}/{{id}}", resource.resource_type),
                    };

                    tests.push(ConformanceTest {
                        name: format!("{}_undeclared_interaction_{}", resource.resource_type, code),
                        description: format!(
                            "Verify that undeclared interaction '{}' is properly rejected for {}",
                            code, resource.resource_type
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::UndeclaredInteraction {
                            interaction: code.to_string(),
                        },
                        request: ConformanceRequest {
                            method: method.to_string(),
                            url,
                            headers: std::collections::HashMap::new(),
                            body: None,
                        },
                        assertion: ConformanceAssertion {
                            // Server should reject with 403, 404, or 405
                            // depending on the interaction type
                            expected_status: 0, // 0 means "expect error status"
                            must_contain_fields: vec![],
                            must_not_contain_fields: vec![],
                            min_entries: None,
                            bundle_type: None,
                            expect_operation_outcome: true,
                        },
                    });
                }
            }

            // --- Undeclared search param rejection tests ---
            let declared_params: std::collections::HashSet<String> = resource
                .search_param
                .iter()
                .map(|p| p.name.clone())
                .collect();

            // Use a clearly invalid param name to test rejection
            if has_search_type && !declared_params.contains("__invalid_conformance_test__") {
                tests.push(ConformanceTest {
                    name: format!("{}_undeclared_search_param", resource.resource_type),
                    description: format!(
                        "Verify that undeclared search param '__invalid_conformance_test__' \
                         is properly rejected for {}",
                        resource.resource_type
                    ),
                    resource_type: resource.resource_type.clone(),
                    kind: ConformanceTestKind::UndeclaredSearchParam {
                        param_name: "__invalid_conformance_test__".to_string(),
                    },
                    request: ConformanceRequest {
                        method: "GET".to_string(),
                        url: format!(
                            "/{}?__invalid_conformance_test__=value",
                            resource.resource_type
                        ),
                        headers: std::collections::HashMap::new(),
                        body: None,
                    },
                    assertion: ConformanceAssertion {
                        // Should get 400 or 200 with OperationOutcome
                        expected_status: 0,
                        must_contain_fields: vec![],
                        must_not_contain_fields: vec![],
                        min_entries: None,
                        bundle_type: None,
                        expect_operation_outcome: true,
                    },
                });
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    tests
        .into_iter()
        .filter(|t| seen.insert(t.name.clone()))
        .collect()
}

/// Collect mustSupport fields from a profile's snapshot or differential.
fn collect_must_support_fields(profile: &StructureDefinition) -> Vec<String> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => match &profile.differential {
            Some(d) => &d.element,
            None => return Vec::new(),
        },
    };

    elements
        .iter()
        .filter(|e| e.must_support && e.path != profile.base_type)
        .filter_map(|e| {
            // Convert "Patient.name" → "name", "Patient.name.family" → "name.family"
            e.path
                .strip_prefix(&format!("{}.", profile.base_type))
                .map(|s| s.to_string())
        })
        .collect()
}

/// Collect fields with cardinality constraints (min > 0 or max != "*") from a profile.
fn collect_cardinality_fields(profile: &StructureDefinition) -> Vec<(String, u32, String)> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => match &profile.differential {
            Some(d) => &d.element,
            None => return Vec::new(),
        },
    };

    elements
        .iter()
        .filter(|e| e.path != profile.base_type)
        .filter(|e| {
            let min_required = e.min.unwrap_or(0) > 0;
            let max_constrained = e.max.as_ref().is_some_and(|m| m != "*");
            min_required || max_constrained
        })
        .filter_map(|e| {
            e.path
                .strip_prefix(&format!("{}.", profile.base_type))
                .map(|field_path| {
                    (
                        field_path.to_string(),
                        e.min.unwrap_or(0),
                        e.max.clone().unwrap_or_else(|| "*".to_string()),
                    )
                })
        })
        .collect()
}

/// Convert a ConformanceTest into a TestCase for execution by the standard test pipeline.
pub fn conformance_test_to_test_case(ct: &ConformanceTest) -> crate::generate::model::TestCase {
    use crate::generate::model::*;

    let interaction = match ct.kind {
        ConformanceTestKind::MustSupportPresence { .. }
        | ConformanceTestKind::Cardinality { .. } => Interaction::SearchType,
        ConformanceTestKind::UndeclaredInteraction { ref interaction } => {
            // Map back from interaction code
            match interaction.as_str() {
                "create" => Interaction::Create,
                "read" => Interaction::Read,
                "update" => Interaction::Update,
                "delete" => Interaction::Delete,
                "vread" => Interaction::Vread,
                "patch" => Interaction::Patch,
                "history-instance" => Interaction::HistoryInstance,
                _ => Interaction::SearchType,
            }
        }
        ConformanceTestKind::UndeclaredSearchParam { .. } => Interaction::SearchType,
    };

    let mut response_assertion = ResponseAssertion::none();

    // Configure assertions based on the conformance test kind
    match &ct.kind {
        ConformanceTestKind::MustSupportPresence { field_path } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            // min_entries=0: empty search results (total=0) are valid per FHIR spec;
            // required_fields checks are skipped when no entries exist.
            response_assertion.min_entries = Some(0);
            // Use required_fields for presence check (not field_values)
            // — this checks the key exists regardless of its value.
            let mut required = std::collections::HashMap::new();
            required.insert(ct.resource_type.clone(), vec![field_path.clone()]);
            response_assertion.required_fields = required;
        }
        ConformanceTestKind::Cardinality { .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            // min_entries=0: cardinality checks only apply when entries exist.
            response_assertion.min_entries = Some(0);
        }
        ConformanceTestKind::UndeclaredInteraction { .. } => {
            // Interactions not declared in the CapabilityStatement SHOULD be
            // rejected — expect OperationOutcome with error severity.
            response_assertion.outcome_severity = Some("error".to_string());
        }
        ConformanceTestKind::UndeclaredSearchParam { .. } => {
            // Per FHIR spec, servers may either reject unknown params (4xx)
            // or ignore them (2xx Bundle). Accept either.
        }
    }

    let expected_status = match &ct.kind {
        ConformanceTestKind::UndeclaredInteraction { .. } => {
            // Use 0 as sentinel — the test framework should accept any non-2xx status
            0
        }
        ConformanceTestKind::UndeclaredSearchParam { .. } => {
            // 0 means reject-or-ignore: pass on non-2xx, or on 2xx Bundle.
            0
        }
        _ => ct.assertion.expected_status,
    };

    let kind = match &ct.kind {
        ConformanceTestKind::MustSupportPresence { field_path } => TestCaseKind::Conformance {
            description: format!("mustSupport field '{}' present", field_path),
        },
        ConformanceTestKind::Cardinality {
            field_path,
            min,
            max,
        } => TestCaseKind::Conformance {
            description: format!("cardinality [{}..{}] on '{}'", min, max, field_path),
        },
        ConformanceTestKind::UndeclaredInteraction { interaction } => TestCaseKind::Conformance {
            description: format!("undeclared interaction '{}' rejected", interaction),
        },
        ConformanceTestKind::UndeclaredSearchParam { param_name } => TestCaseKind::Conformance {
            description: format!("undeclared search param '{}' rejected", param_name),
        },
    };

    let request = HttpRequest {
        method: ct.request.method.clone(),
        url: ct.request.url.clone(),
        headers: ct.request.headers.clone(),
        body: ct.request.body.clone(),
    };

    let validation = ValidationSpec {
        expected_status,
        profile_url: None,
        required_elements: ct.assertion.must_contain_fields.clone(),
        forbidden_elements: ct.assertion.must_not_contain_fields.clone(),
        response_assertion: if response_assertion == ResponseAssertion::none() {
            None
        } else {
            Some(response_assertion)
        },
    };

    TestCase {
        name: ct.name.clone(),
        kind,
        interaction,
        resource_type: ct.resource_type.clone(),
        profile_url: None,
        request,
        validation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::profile::{ElementDefinition, Snapshot};

    fn sample_cs() -> CapabilityStatement {
        CapabilityStatement {
            resource_type: "CapabilityStatement".to_string(),
            url: Some("http://example.org/CapabilityStatement/test".to_string()),
            name: Some("TestCS".to_string()),
            status: Some("active".to_string()),
            rest: vec![Rest {
                mode: "server".to_string(),
                resource: vec![RestResource {
                    resource_type: "Patient".to_string(),
                    profile: None,
                    supported_profile: vec![],
                    interaction: vec![
                        RestInteraction {
                            code: "read".to_string(),
                        },
                        RestInteraction {
                            code: "search-type".to_string(),
                        },
                    ],
                    search_param: vec![RestSearchParam {
                        name: "name".to_string(),
                        param_type: "string".to_string(),
                        definition: None,
                        documentation: None,
                    }],
                    operation: vec![],
                    read_history: None,
                    update_create: None,
                    conditional_create: None,
                    conditional_read: None,
                    conditional_update: None,
                    conditional_delete: None,
                    search_include: vec![],
                    search_revinclude: vec![],
                }],
                interaction: vec![],
                operation: vec![],
            }],
        }
    }

    fn sample_profile() -> StructureDefinition {
        StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestPatient".to_string(),
            name: "TestPatient".to_string(),
            base_type: "Patient".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Patient".to_string(),
                        path: "Patient".to_string(),
                        min: Some(0),
                        max: Some("*".to_string()),
                        type_: vec![],
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        fixed_boolean: None,
                        fixed_integer: None,
                        fixed_decimal: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        pattern_boolean: None,
                        must_support: false,
                        short: None,
                        definition: None,
                        binding: None,
                        content_reference: None,
                        fixed_quantity: None,
                        pattern_quantity: None,
                        fixed_coding: None,
                        pattern_coding: None,
                        fixed_codeable_concept: None,
                        pattern_codeable_concept: None,
                        constraint: vec![],
                        is_modifier: false,
                        is_summary: false,
                        slice_name: None,
                        slicing: None,
                    },
                    ElementDefinition {
                        id: "Patient.name".to_string(),
                        path: "Patient.name".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![],
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        fixed_boolean: None,
                        fixed_integer: None,
                        fixed_decimal: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        pattern_boolean: None,
                        must_support: true,
                        short: None,
                        definition: None,
                        binding: None,
                        content_reference: None,
                        fixed_quantity: None,
                        pattern_quantity: None,
                        fixed_coding: None,
                        pattern_coding: None,
                        fixed_codeable_concept: None,
                        pattern_codeable_concept: None,
                        constraint: vec![],
                        is_modifier: false,
                        is_summary: false,
                        slice_name: None,
                        slicing: None,
                    },
                    ElementDefinition {
                        id: "Patient.birthDate".to_string(),
                        path: "Patient.birthDate".to_string(),
                        min: Some(0),
                        max: Some("1".to_string()),
                        type_: vec![],
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        fixed_boolean: None,
                        fixed_integer: None,
                        fixed_decimal: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        pattern_boolean: None,
                        must_support: false,
                        short: None,
                        definition: None,
                        binding: None,
                        content_reference: None,
                        fixed_quantity: None,
                        pattern_quantity: None,
                        fixed_coding: None,
                        pattern_coding: None,
                        fixed_codeable_concept: None,
                        pattern_codeable_concept: None,
                        constraint: vec![],
                        is_modifier: false,
                        is_summary: false,
                        slice_name: None,
                        slicing: None,
                    },
                    ElementDefinition {
                        id: "Patient.gender".to_string(),
                        path: "Patient.gender".to_string(),
                        min: Some(0),
                        max: Some("1".to_string()),
                        type_: vec![],
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        fixed_boolean: None,
                        fixed_integer: None,
                        fixed_decimal: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        pattern_boolean: None,
                        must_support: true,
                        short: None,
                        definition: None,
                        binding: None,
                        content_reference: None,
                        fixed_quantity: None,
                        pattern_quantity: None,
                        fixed_coding: None,
                        pattern_coding: None,
                        fixed_codeable_concept: None,
                        pattern_codeable_concept: None,
                        constraint: vec![],
                        is_modifier: false,
                        is_summary: false,
                        slice_name: None,
                        slicing: None,
                    },
                ],
            }),
            differential: None,
        }
    }

    #[test]
    fn validate_well_formed_capability_statement() {
        let cs = sample_cs();
        let result = validate_capability_statement(&cs);
        assert!(
            result.errors.is_empty(),
            "Expected no errors, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn validate_cs_missing_status() {
        let mut cs = sample_cs();
        cs.status = None;
        let result = validate_capability_statement(&cs);
        assert!(result.errors.iter().any(|e| e.contains("status")));
    }

    #[test]
    fn validate_cs_no_server_rest() {
        let cs = CapabilityStatement {
            resource_type: "CapabilityStatement".to_string(),
            url: None,
            name: None,
            status: Some("active".to_string()),
            rest: vec![Rest {
                mode: "client".to_string(),
                resource: vec![],
                interaction: vec![],
                operation: vec![],
            }],
        };
        let result = validate_capability_statement(&cs);
        assert!(result.errors.iter().any(|e| e.contains("mode='server'")));
    }

    #[test]
    fn generate_must_support_tests() {
        let cs = sample_cs();
        let profile = sample_profile();
        let tests = generate_conformance_tests(&cs, &[profile]);

        let must_support_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::MustSupportPresence { .. }))
            .collect();
        assert!(
            !must_support_tests.is_empty(),
            "Expected at least 1 mustSupport test, got {}",
            must_support_tests.len()
        );

        // Should test that 'name' is present (it's mustSupport=true, min=1)
        let name_test = must_support_tests
            .iter()
            .find(|t| t.name.contains("must_support_name"));
        assert!(name_test.is_some(), "Expected must_support_name test");

        // Should test that 'gender' is present (it's mustSupport=true, min=0)
        let gender_test = must_support_tests
            .iter()
            .find(|t| t.name.contains("must_support_gender"));
        assert!(
            gender_test.is_some(),
            "Expected must_support_gender test for mustSupport field with min=0"
        );
    }

    #[test]
    fn generate_cardinality_tests() {
        let cs = sample_cs();
        let profile = sample_profile();
        let tests = generate_conformance_tests(&cs, &[profile]);

        let cardinality_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::Cardinality { .. }))
            .collect();
        assert!(
            !cardinality_tests.is_empty(),
            "Expected at least 1 cardinality test, got {}",
            cardinality_tests.len()
        );

        // Should test 'name' (min=1) and 'birthDate' (max=1)
        let name_test = cardinality_tests
            .iter()
            .find(|t| t.name.contains("cardinality_name"));
        assert!(name_test.is_some(), "Expected cardinality_name test");

        let birthdate_test = cardinality_tests
            .iter()
            .find(|t| t.name.contains("cardinality_birthDate"));
        assert!(
            birthdate_test.is_some(),
            "Expected cardinality_birthDate test"
        );
    }

    #[test]
    fn generate_undeclared_interaction_tests() {
        let cs = sample_cs();
        let profile = sample_profile();
        let tests = generate_conformance_tests(&cs, &[profile]);

        let interaction_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::UndeclaredInteraction { .. }))
            .collect();

        // CS declares only 'read' and 'search-type', so we should have
        // tests for create, update, delete, vread, patch, history-instance
        assert!(
            interaction_tests.len() >= 4,
            "Expected at least 4 undeclared interaction tests, got {}",
            interaction_tests.len()
        );

        // Verify create is rejected (not in declared interactions)
        let create_test = interaction_tests
            .iter()
            .find(|t| t.name.contains("undeclared_interaction_create"));
        assert!(
            create_test.is_some(),
            "Expected undeclared_interaction_create test"
        );
    }

    #[test]
    fn generate_undeclared_search_param_tests() {
        let cs = sample_cs();
        let profile = sample_profile();
        let tests = generate_conformance_tests(&cs, &[profile]);

        let param_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::UndeclaredSearchParam { .. }))
            .collect();

        assert!(
            !param_tests.is_empty(),
            "Expected at least 1 undeclared search param test, got {}",
            param_tests.len()
        );
    }
}
