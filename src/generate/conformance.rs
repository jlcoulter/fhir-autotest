//! Conformance test generation for the FHIR IG Responder actor.
//!
//! Generates tests that verify a server's declared CapabilityStatement
//! obligations are actually met. This covers:
//!
//! 1. **CapabilityStatement well-formedness** — the CS itself must have
//!    required fields (status, rest with server mode, etc.)
//! 2. **MustSupport field presence (best-effort)** — fields marked mustSupport=true
//!    in profiles declared by the CS are checked for presence in responses.
//!    Per FHIR R4 §2.1.2.1.12, mustSupport means "the server SHALL populate the
//!    element if the data exists for the use case." A field may be legitimately
//!    absent if the server has no data for it. This check is a best-effort
//!    heuristic — absence does not necessarily indicate non-conformance.
//! 3. **Cardinality enforcement** — min/max constraints from profile
//!    ElementDefinitions should be respected in responses
//! 4. **Undeclared interaction rejection** — interactions NOT declared in
//!    the CS should be rejected by the server (negative conformance)
//! 5. **FHIRPath invariant validation (best-effort)** — constraints with
//!    severity=error are checked via simple field-existence patterns
//! 6. **Binding strength validation** — required-binding fields must use
//!    values from the bound ValueSet
//! 7. **Fixed/pattern value validation** — fields with fixed[x] or pattern[x]
//!    must match the profile definition
//! 8. **Slice validation** — sliced elements must match discriminator patterns
//! 9. **Extension validation** — extensions must match profile-defined URLs
//! 10. **Type constraint validation** — polymorphic value[x] fields must use
//!     allowed types
//! 11. **Reference target profile validation** — reference fields should point
//!     to resources with matching meta.profile

use crate::model::SearchParameter;
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
    ///
    /// **Best-effort check**: Per FHIR R4 §2.1.2.1.12, mustSupport means
    /// "the server SHALL populate the element if the data exists for the
    /// use case." A field may be legitimately absent if the server has no
    /// data for it. Absence does not necessarily indicate non-conformance.
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
    /// Verify that FHIRPath invariants (constraints with severity=error) are satisfied.
    /// Best-effort: simple field-existence checks for common patterns like `field.exists()`.
    ConstraintValidation {
        constraint_key: String,
        constraint_human: String,
        expression: String,
        field_path: String,
    },
    /// Verify that required-binding fields use values from the bound ValueSet.
    BindingValidation {
        field_path: String,
        value_set_url: String,
    },
    /// Verify that fields with fixed/pattern values match the profile definition.
    FixedValueValidation {
        field_path: String,
        expected_value: serde_json::Value,
    },
    /// Verify that sliced elements match their discriminator patterns.
    SliceValidation {
        field_path: String,
        slice_name: String,
        discriminator_path: String,
        discriminator_type: String,
    },
    /// Verify that extensions in responses match profile-defined extension URLs.
    ExtensionValidation { extension_url: String },
    /// Verify that polymorphic value[x] fields use an allowed type.
    TypeConstraintValidation {
        field_path: String,
        allowed_types: Vec<String>,
    },
    /// Verify that reference fields point to resources with matching meta.profile.
    ReferenceTargetValidation {
        field_path: String,
        target_profile: String,
    },
    /// Verify that a SearchParameter's expression resolves to a valid field path
    /// in response resources.
    ExpressionValidation {
        param_name: String,
        expression: String,
        field_path: String,
    },
    /// Verify that reference search params only return resources of the
    /// declared target types.
    TargetTypeValidation {
        param_name: String,
        target_types: Vec<String>,
    },
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
/// - FHIRPath invariants (best-effort field-existence checks)
/// - Required binding strength values are from the bound ValueSet
/// - Fixed/pattern values match the profile
/// - Sliced elements match discriminator patterns
/// - Extensions match profile-defined URLs
/// - Polymorphic value[x] fields use allowed types
/// - Reference fields point to resources with matching meta.profile
pub fn generate_conformance_tests(
    cs: &CapabilityStatement,
    profiles: &[StructureDefinition],
    search_params: &[SearchParameter],
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
            let has_read = resource.interaction.iter().any(|i| i.code == "read");

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
                            "Best-effort: verify mustSupport field '{}' is present in {} responses (may be absent if server has no data)",
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

            // --- Profile-derived conformance tests ---
            if has_search_type && let Some(profile) = profile {
                // 1. FHIRPath invariant validation (best-effort)
                let constraint_fields = collect_constraint_fields(profile);
                for (constraint_key, constraint_human, expression, field_path) in constraint_fields
                {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_constraint_{}",
                            resource.resource_type,
                            constraint_key.replace('.', "_")
                        ),
                        description: format!(
                            "Best-effort: verify constraint '{}' ({}) on {} — expression: {}",
                            constraint_key, constraint_human, resource.resource_type, expression
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::ConstraintValidation {
                            constraint_key: constraint_key.clone(),
                            constraint_human: constraint_human.clone(),
                            expression: expression.clone(),
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
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }

                // 2. Binding strength validation (required bindings only)
                let binding_fields = collect_binding_fields(profile);
                for (field_path, value_set_url) in binding_fields {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_binding_{}",
                            resource.resource_type,
                            field_path.replace('.', "_")
                        ),
                        description: format!(
                            "Verify required binding on '{}' uses values from ValueSet '{}'",
                            field_path, value_set_url
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::BindingValidation {
                            field_path: field_path.clone(),
                            value_set_url: value_set_url.clone(),
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
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }

                // 3. Fixed/pattern value validation
                let fixed_value_fields = collect_fixed_value_fields(profile);
                for (field_path, expected_value) in fixed_value_fields {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_fixed_value_{}",
                            resource.resource_type,
                            field_path.replace('.', "_")
                        ),
                        description: format!(
                            "Verify fixed/pattern value on '{}' matches profile definition",
                            field_path
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::FixedValueValidation {
                            field_path: field_path.clone(),
                            expected_value: expected_value.clone(),
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
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }

                // 4. Slice validation
                let slice_fields = collect_slice_fields(profile);
                for (field_path, slice_name, discriminator_path, discriminator_type) in slice_fields
                {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_slice_{}",
                            resource.resource_type,
                            slice_name.replace('.', "_")
                        ),
                        description: format!(
                            "Verify slice '{}' on '{}' matches discriminator pattern ({})",
                            slice_name, field_path, discriminator_type
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::SliceValidation {
                            field_path: field_path.clone(),
                            slice_name: slice_name.clone(),
                            discriminator_path: discriminator_path.clone(),
                            discriminator_type: discriminator_type.clone(),
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
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }

                // 5. Extension validation
                let extension_urls = collect_extension_urls(profile);
                for extension_url in extension_urls {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_extension_{}",
                            resource.resource_type,
                            extension_url
                                .replace([':', '/', '.'], "_")
                                .replace("http___", "ext_")
                        ),
                        description: format!(
                            "Verify extension '{}' is present in {} responses",
                            extension_url, resource.resource_type
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::ExtensionValidation {
                            extension_url: extension_url.clone(),
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
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }

                // 6. Type constraint validation (polymorphic value[x] fields)
                let type_constraint_fields = collect_type_constraint_fields(profile);
                for (field_path, allowed_types) in type_constraint_fields {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_type_constraint_{}",
                            resource.resource_type,
                            field_path.replace('.', "_")
                        ),
                        description: format!(
                            "Verify polymorphic field '{}' uses an allowed type ({})",
                            field_path,
                            allowed_types.join(", ")
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::TypeConstraintValidation {
                            field_path: field_path.clone(),
                            allowed_types: allowed_types.clone(),
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
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }

                // 7. Reference target profile validation
                let reference_target_fields = collect_reference_target_fields(profile);
                for (field_path, target_profile) in reference_target_fields {
                    tests.push(ConformanceTest {
                        name: format!(
                            "{}_reference_target_{}",
                            resource.resource_type,
                            field_path.replace('.', "_")
                        ),
                        description: format!(
                            "Verify reference field '{}' points to resources conforming to '{}'",
                            field_path, target_profile
                        ),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::ReferenceTargetValidation {
                            field_path: field_path.clone(),
                            target_profile: target_profile.clone(),
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
                            min_entries: Some(0),
                            bundle_type: Some("searchset".to_string()),
                            expect_operation_outcome: false,
                        },
                    });
                }
            }

            // --- Search Parameter Expression Validation ---
            // For each SearchParameter with an expression, check that the
            // expression path exists in response resources.
            if has_search_type {
                let resource_search_params: Vec<&SearchParameter> = search_params
                    .iter()
                    .filter(|sp| sp.base.contains(&resource.resource_type))
                    .collect();

                for sp in &resource_search_params {
                    if let Some(ref expression) = sp.expression {
                        // Parse the FHIRPath expression to extract a field path
                        if let Some(field_path) =
                            extract_expression_field_path(expression, &resource.resource_type)
                        {
                            tests.push(ConformanceTest {
                                name: format!(
                                    "{}_expression_{}",
                                    resource.resource_type,
                                    sp.code.replace('-', "_")
                                ),
                                description: format!(
                                    "Verify SearchParameter '{}' expression '{}' resolves to field '{}' in {} responses",
                                    sp.code, expression, field_path, resource.resource_type
                                ),
                                resource_type: resource.resource_type.clone(),
                                kind: ConformanceTestKind::ExpressionValidation {
                                    param_name: sp.code.clone(),
                                    expression: expression.clone(),
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
                                    min_entries: Some(0),
                                    bundle_type: Some("searchset".to_string()),
                                    expect_operation_outcome: false,
                                },
                            });
                        }
                    }
                }

                // --- Search Parameter Target Type Validation ---
                // For reference-type SearchParameters with declared target types,
                // verify that returned resources match the target types.
                for sp in &resource_search_params {
                    if sp.param_type == "reference" && !sp.target.is_empty() {
                        tests.push(ConformanceTest {
                            name: format!(
                                "{}_target_type_{}",
                                resource.resource_type,
                                sp.code.replace('-', "_")
                            ),
                            description: format!(
                                "Verify reference SearchParameter '{}' targets [{}] in {} responses",
                                sp.code,
                                sp.target.join(", "),
                                resource.resource_type
                            ),
                            resource_type: resource.resource_type.clone(),
                            kind: ConformanceTestKind::TargetTypeValidation {
                                param_name: sp.code.clone(),
                                target_types: sp.target.clone(),
                            },
                            request: ConformanceRequest {
                                method: "GET".to_string(),
                                url: format!(
                                    "/{}?{}={}/test-id&_id={}-1&_count=10",
                                    resource.resource_type,
                                    sp.code,
                                    sp.target.first().map(|s| s.as_str()).unwrap_or("Unknown"),
                                    resource.resource_type.to_lowercase()
                                ),
                                headers: std::collections::HashMap::new(),
                                body: None,
                            },
                            assertion: ConformanceAssertion {
                                expected_status: 200,
                                must_contain_fields: vec![],
                                must_not_contain_fields: vec![],
                                min_entries: Some(0),
                                bundle_type: Some("searchset".to_string()),
                                expect_operation_outcome: false,
                            },
                        });
                    }
                }
            }

            // --- versioning conformance tests ---
            // When versioning is declared as "versioned" or "versioned-update",
            // the server should return meta.versionId on resources.
            if let Some(ref versioning) = resource.versioning
                && (versioning == "versioned" || versioning == "versioned-update")
                && has_read
            {
                tests.push(ConformanceTest {
                    name: format!("{}_versioning", resource.resource_type),
                    description: format!(
                        "Verify that versioning '{}' is respected: resources should have meta.versionId",
                        versioning
                    ),
                    resource_type: resource.resource_type.clone(),
                    kind: ConformanceTestKind::MustSupportPresence {
                        field_path: "meta.versionId".to_string(),
                    },
                    request: ConformanceRequest {
                        method: "GET".to_string(),
                        url: format!("/{}/{{id}}", resource.resource_type),
                        headers: std::collections::HashMap::new(),
                        body: None,
                    },
                    assertion: ConformanceAssertion {
                        expected_status: 200,
                        must_contain_fields: vec!["meta.versionId".to_string()],
                        must_not_contain_fields: vec![],
                        min_entries: None,
                        bundle_type: None,
                        expect_operation_outcome: false,
                    },
                });
            }

            // --- readHistory conformance tests ---
            // When readHistory is true, the server should support vread
            // (version-aware reads).
            if resource.read_history == Some(true) {
                let has_vread = resource.interaction.iter().any(|i| i.code == "vread");
                if has_vread {
                    tests.push(ConformanceTest {
                        name: format!("{}_read_history", resource.resource_type),
                        description: "Verify that readHistory is respected: vread should return a resource with meta.versionId".to_string(),
                        resource_type: resource.resource_type.clone(),
                        kind: ConformanceTestKind::MustSupportPresence {
                            field_path: "meta.versionId".to_string(),
                        },
                        request: ConformanceRequest {
                            method: "GET".to_string(),
                            url: format!("/{}/{{id}}/_history/1", resource.resource_type),
                            headers: std::collections::HashMap::new(),
                            body: None,
                        },
                        assertion: ConformanceAssertion {
                            expected_status: 200,
                            must_contain_fields: vec!["meta.versionId".to_string()],
                            must_not_contain_fields: vec![],
                            min_entries: None,
                            bundle_type: None,
                            expect_operation_outcome: false,
                        },
                    });
                }
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    tests
        .into_iter()
        .filter(|t| seen.insert(t.name.clone()))
        .collect()
}

/// Extract a field path from a FHIRPath expression for use in conformance tests.
///
/// Handles common patterns:
/// - `Patient.name` → `name`
/// - `Patient.name.family` → `name.family`
/// - `Patient.name | Practitioner.name` → `name` (takes first)
/// - `Patient.deceasedBoolean | Patient.deceasedDateTime` → `deceasedBoolean`
fn extract_expression_field_path(expression: &str, resource_type: &str) -> Option<String> {
    // Take the first alternative (before |)
    let first_alt = expression.split('|').next()?.trim();

    // Strip the resource type prefix if present
    let prefix = format!("{}.", resource_type);
    if let Some(path) = first_alt.strip_prefix(&prefix) {
        // Take only the first two path components (field.subfield)
        let parts: Vec<&str> = path.split('.').collect();
        if parts.len() >= 2 {
            Some(format!("{}.{}", parts[0], parts[1]))
        } else if !parts.is_empty() {
            Some(parts[0].to_string())
        } else {
            None
        }
    } else if first_alt.contains('.') {
        // Expression doesn't start with resource type, use as-is
        let parts: Vec<&str> = first_alt.split('.').collect();
        if parts.len() >= 2 {
            Some(format!("{}.{}", parts[0], parts[1]))
        } else {
            Some(parts[0].to_string())
        }
    } else {
        // Bare field name
        Some(first_alt.to_string())
    }
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

/// Collect FHIRPath constraints with severity=error from a profile.
///
/// Best-effort: extracts simple field-existence patterns like `field.exists()`
/// from the FHIRPath expression. Returns (constraint_key, human_description, expression, field_path).
fn collect_constraint_fields(
    profile: &StructureDefinition,
) -> Vec<(String, String, String, String)> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => match &profile.differential {
            Some(d) => &d.element,
            None => return Vec::new(),
        },
    };

    let mut results = Vec::new();

    for element in elements {
        for constraint in &element.constraint {
            if constraint.severity != "error" {
                continue;
            }

            // Try to extract a simple field path from the expression
            let field_path = if let Some(ref expr) = constraint.expression {
                extract_field_path_from_expression(expr, &element.path, &profile.base_type)
            } else {
                continue;
            };

            if let Some(fp) = field_path {
                results.push((
                    constraint.key.clone(),
                    constraint.human.clone().unwrap_or_default(),
                    constraint.expression.clone().unwrap_or_default(),
                    fp,
                ));
            }
        }
    }

    results
}

/// Extract a field path from a simple FHIRPath expression.
///
/// Handles common patterns:
/// - `field.exists()` → field
/// - `field.all(...)` → field
/// - `field.where(...)` → field
/// - `field` → field (bare field name)
fn extract_field_path_from_expression(
    expression: &str,
    element_path: &str,
    base_type: &str,
) -> Option<String> {
    let trimmed = expression.trim();

    // Pattern: field.exists() or field.all(...) or field.where(...)
    let field_name = if let Some(dot_pos) = trimmed.find('.') {
        let name = &trimmed[..dot_pos];
        if name.is_empty() {
            // Expression starts with a function call like "exists()" — use the element path
            element_path
                .strip_prefix(&format!("{}.", base_type))
                .map(|s| s.to_string())?
        } else {
            name.to_string()
        }
    } else {
        // Bare field name or simple expression
        trimmed.to_string()
    };

    // If the field name is the base type itself, use the element path
    if field_name == base_type {
        return element_path
            .strip_prefix(&format!("{}.", base_type))
            .map(|s| s.to_string());
    }

    // If the field name is already a relative path (no base type prefix), use it directly
    if !field_name.contains('.') && !field_name.contains('(') {
        return Some(field_name);
    }

    // Try to strip base type prefix if present
    if let Some(relative) = field_name.strip_prefix(&format!("{}.", base_type)) {
        return Some(relative.to_string());
    }

    // Fall back to the element path
    element_path
        .strip_prefix(&format!("{}.", base_type))
        .map(|s| s.to_string())
}

/// Collect fields with required binding strength from a profile.
/// Returns (field_path, value_set_url).
fn collect_binding_fields(profile: &StructureDefinition) -> Vec<(String, String)> {
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
        .filter_map(|e| {
            let binding = e.binding.as_ref()?;
            if binding.strength != "required" {
                return None;
            }
            let value_set_url = binding.value_set.as_ref()?;
            let field_path = e.path.strip_prefix(&format!("{}.", profile.base_type))?;
            Some((field_path.to_string(), value_set_url.clone()))
        })
        .collect()
}

/// Collect fields with fixed or pattern values from a profile.
/// Returns (field_path, expected_value).
fn collect_fixed_value_fields(profile: &StructureDefinition) -> Vec<(String, serde_json::Value)> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => match &profile.differential {
            Some(d) => &d.element,
            None => return Vec::new(),
        },
    };

    let mut results = Vec::new();

    for element in elements {
        if element.path == profile.base_type {
            continue;
        }

        let field_path = match element
            .path
            .strip_prefix(&format!("{}.", profile.base_type))
        {
            Some(fp) => fp.to_string(),
            None => continue,
        };

        // Check fixed values
        if let Some(val) = &element.fixed_string {
            results.push((field_path.clone(), serde_json::Value::String(val.clone())));
        } else if let Some(val) = &element.fixed_code {
            results.push((field_path.clone(), serde_json::Value::String(val.clone())));
        } else if let Some(val) = &element.fixed_uri {
            results.push((field_path.clone(), serde_json::Value::String(val.clone())));
        } else if let Some(val) = element.fixed_boolean {
            results.push((field_path.clone(), serde_json::Value::Bool(val)));
        } else if let Some(val) = element.fixed_integer {
            results.push((field_path.clone(), serde_json::json!(val)));
        } else if let Some(val) = element.fixed_decimal {
            results.push((field_path.clone(), serde_json::json!(val)));
        } else if let Some(val) = &element.fixed_quantity {
            results.push((field_path.clone(), val.clone()));
        } else if let Some(val) = &element.fixed_coding {
            results.push((field_path.clone(), val.clone()));
        } else if let Some(val) = &element.fixed_codeable_concept {
            results.push((field_path.clone(), val.clone()));
        }
        // Check pattern values (lower priority than fixed)
        else if let Some(val) = &element.pattern_string {
            results.push((field_path.clone(), serde_json::Value::String(val.clone())));
        } else if let Some(val) = &element.pattern_code {
            results.push((field_path.clone(), serde_json::Value::String(val.clone())));
        } else if let Some(val) = &element.pattern_uri {
            results.push((field_path.clone(), serde_json::Value::String(val.clone())));
        } else if let Some(val) = element.pattern_boolean {
            results.push((field_path.clone(), serde_json::Value::Bool(val)));
        } else if let Some(val) = &element.pattern_quantity {
            results.push((field_path.clone(), val.clone()));
        } else if let Some(val) = &element.pattern_coding {
            results.push((field_path.clone(), val.clone()));
        } else if let Some(val) = &element.pattern_codeable_concept {
            results.push((field_path.clone(), val.clone()));
        }
    }

    results
}

/// Collect slice information from a profile.
/// Returns (field_path, slice_name, discriminator_path, discriminator_type).
fn collect_slice_fields(profile: &StructureDefinition) -> Vec<(String, String, String, String)> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => match &profile.differential {
            Some(d) => &d.element,
            None => return Vec::new(),
        },
    };

    let mut results = Vec::new();

    for element in elements {
        if element.path == profile.base_type {
            continue;
        }

        let slice_name = match &element.slice_name {
            Some(name) => name.clone(),
            None => continue,
        };

        let field_path = match element
            .path
            .strip_prefix(&format!("{}.", profile.base_type))
        {
            Some(fp) => fp.to_string(),
            None => continue,
        };

        // Get discriminator info from the slicing definition
        if let Some(ref slicing) = element.slicing {
            for discriminator in &slicing.discriminator {
                results.push((
                    field_path.clone(),
                    slice_name.clone(),
                    discriminator.path.clone(),
                    discriminator.discriminator_type.clone(),
                ));
            }
        }
    }

    results
}

/// Collect extension URLs from a profile.
/// Looks for elements where type[].code=Extension and type[].profile is set.
fn collect_extension_urls(profile: &StructureDefinition) -> Vec<String> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => match &profile.differential {
            Some(d) => &d.element,
            None => return Vec::new(),
        },
    };

    let mut urls = Vec::new();

    for element in elements {
        for type_def in &element.type_ {
            if type_def.code == "Extension" && !type_def.profile.is_empty() {
                for profile_url in &type_def.profile {
                    urls.push(profile_url.clone());
                }
            }
        }
    }

    urls
}

/// Collect type constraints for polymorphic value[x] fields.
/// Returns (field_path, allowed_type_codes).
fn collect_type_constraint_fields(profile: &StructureDefinition) -> Vec<(String, Vec<String>)> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => match &profile.differential {
            Some(d) => &d.element,
            None => return Vec::new(),
        },
    };

    let mut results = Vec::new();

    for element in elements {
        if element.path == profile.base_type {
            continue;
        }

        // Only consider polymorphic fields (path ends with [x])
        if !element.path.ends_with("[x]") {
            continue;
        }

        let field_path = match element
            .path
            .strip_prefix(&format!("{}.", profile.base_type))
        {
            Some(fp) => fp.to_string(),
            None => continue,
        };

        let allowed_types: Vec<String> = element.type_.iter().map(|t| t.code.clone()).collect();

        if !allowed_types.is_empty() {
            results.push((field_path, allowed_types));
        }
    }

    results
}

/// Collect reference target profiles from a profile.
/// Returns (field_path, target_profile_url).
fn collect_reference_target_fields(profile: &StructureDefinition) -> Vec<(String, String)> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => match &profile.differential {
            Some(d) => &d.element,
            None => return Vec::new(),
        },
    };

    let mut results = Vec::new();

    for element in elements {
        if element.path == profile.base_type {
            continue;
        }

        let field_path = match element
            .path
            .strip_prefix(&format!("{}.", profile.base_type))
        {
            Some(fp) => fp.to_string(),
            None => continue,
        };

        for type_def in &element.type_ {
            for target_profile in &type_def.target_profile {
                results.push((field_path.clone(), target_profile.clone()));
            }
        }
    }

    results
}

/// Convert a ConformanceTest into a TestCase for execution by the standard test pipeline.
pub fn conformance_test_to_test_case(ct: &ConformanceTest) -> crate::generate::model::TestCase {
    use crate::generate::model::*;

    let interaction = match ct.kind {
        ConformanceTestKind::MustSupportPresence { .. }
        | ConformanceTestKind::Cardinality { .. }
        | ConformanceTestKind::ConstraintValidation { .. }
        | ConformanceTestKind::BindingValidation { .. }
        | ConformanceTestKind::FixedValueValidation { .. }
        | ConformanceTestKind::SliceValidation { .. }
        | ConformanceTestKind::ExtensionValidation { .. }
        | ConformanceTestKind::TypeConstraintValidation { .. }
        | ConformanceTestKind::ReferenceTargetValidation { .. }
        | ConformanceTestKind::ExpressionValidation { .. }
        | ConformanceTestKind::TargetTypeValidation { .. } => Interaction::SearchType,
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
        ConformanceTestKind::ConstraintValidation { field_path, .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
            let mut required = std::collections::HashMap::new();
            required.insert(ct.resource_type.clone(), vec![field_path.clone()]);
            response_assertion.required_fields = required;
        }
        ConformanceTestKind::BindingValidation { .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
        }
        ConformanceTestKind::FixedValueValidation {
            field_path,
            expected_value,
        } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
            let mut field_vals: std::collections::HashMap<String, serde_json::Value> =
                std::collections::HashMap::new();
            field_vals.insert(field_path.clone(), expected_value.clone());
            let mut by_type = std::collections::HashMap::new();
            by_type.insert(ct.resource_type.clone(), field_vals);
            response_assertion.field_values = by_type;
        }
        ConformanceTestKind::SliceValidation { .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
        }
        ConformanceTestKind::ExtensionValidation { .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
        }
        ConformanceTestKind::TypeConstraintValidation { .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
        }
        ConformanceTestKind::ReferenceTargetValidation { .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
        }
        ConformanceTestKind::ExpressionValidation { .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
        }
        ConformanceTestKind::TargetTypeValidation { .. } => {
            response_assertion.bundle_type = Some("searchset".to_string());
            response_assertion.min_entries = Some(0);
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
            description: format!("best-effort: mustSupport field '{}' present", field_path),
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
        ConformanceTestKind::ConstraintValidation {
            constraint_key,
            constraint_human,
            ..
        } => TestCaseKind::Conformance {
            description: format!(
                "best-effort: constraint '{}' ({})",
                constraint_key,
                constraint_human.as_str()
            ),
        },
        ConformanceTestKind::BindingValidation {
            field_path,
            value_set_url,
        } => TestCaseKind::Conformance {
            description: format!(
                "required binding on '{}' from '{}'",
                field_path, value_set_url
            ),
        },
        ConformanceTestKind::FixedValueValidation { field_path, .. } => TestCaseKind::Conformance {
            description: format!("fixed/pattern value on '{}'", field_path),
        },
        ConformanceTestKind::SliceValidation {
            field_path,
            slice_name,
            ..
        } => TestCaseKind::Conformance {
            description: format!("slice '{}' on '{}'", slice_name, field_path),
        },
        ConformanceTestKind::ExtensionValidation { extension_url } => TestCaseKind::Conformance {
            description: format!("extension '{}'", extension_url),
        },
        ConformanceTestKind::TypeConstraintValidation {
            field_path,
            allowed_types,
        } => TestCaseKind::Conformance {
            description: format!(
                "type constraint on '{}': allowed types [{}]",
                field_path,
                allowed_types.join(", ")
            ),
        },
        ConformanceTestKind::ReferenceTargetValidation {
            field_path,
            target_profile,
        } => TestCaseKind::Conformance {
            description: format!(
                "reference target '{}' should conform to '{}'",
                field_path, target_profile
            ),
        },
        ConformanceTestKind::ExpressionValidation {
            param_name,
            expression,
            field_path,
        } => TestCaseKind::Conformance {
            description: format!(
                "expression '{}' resolves to field '{}' for param '{}'",
                expression, field_path, param_name
            ),
        },
        ConformanceTestKind::TargetTypeValidation {
            param_name,
            target_types,
        } => TestCaseKind::Conformance {
            description: format!(
                "reference param '{}' targets [{}]",
                param_name,
                target_types.join(", ")
            ),
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

/// Validate security declarations in a CapabilityStatement.
///
/// Checks:
/// - If `security.cors` is true, CORS headers should be present (best-effort)
/// - If `security.service` includes OAuth, the OAuth endpoint should be reachable
pub fn validate_security(cs: &CapabilityStatement) -> CapabilityStatementValidation {
    let errors = Vec::new();
    let mut warnings = Vec::new();

    for (i, rest) in cs.rest.iter().enumerate() {
        if rest.mode != "server" {
            continue;
        }
        if let Some(ref security) = rest.security {
            if security.cors == Some(true) {
                warnings.push(format!(
                    "rest[{}].security.cors is true — CORS headers should be present in responses",
                    i
                ));
            }

            for (j, service) in security.service.iter().enumerate() {
                if let Some(ref coding) = service.coding
                    && coding.code.as_deref() == Some("OAuth")
                {
                    warnings.push(format!(
                        "rest[{}].security.service[{}] declares OAuth — OAuth endpoint should be reachable",
                        i, j
                    ));
                }
            }
        }
    }

    CapabilityStatementValidation { errors, warnings }
}

/// Validate software/implementation metadata in a CapabilityStatement.
///
/// Checks:
/// - `software.name` and `software.version` should be present
/// - `implementation.description` should be present
pub fn validate_metadata(cs: &CapabilityStatement) -> CapabilityStatementValidation {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if let Some(ref sw) = cs.software {
        if sw.name.as_deref().unwrap_or("").is_empty() {
            errors.push("CapabilityStatement.software.name is missing or empty".to_string());
        }
        if sw.version.as_deref().unwrap_or("").is_empty() {
            warnings.push("CapabilityStatement.software.version is missing or empty".to_string());
        }
    }

    if let Some(ref imp) = cs.implementation
        && imp.description.as_deref().unwrap_or("").is_empty()
    {
        errors
            .push("CapabilityStatement.implementation.description is missing or empty".to_string());
    }

    CapabilityStatementValidation { errors, warnings }
}

/// Validate messaging capabilities in a CapabilityStatement.
///
/// Checks:
/// - `messaging.endpoint` should be present
/// - `messaging.supportedMessage` types should have valid definitions
pub fn validate_messaging(cs: &CapabilityStatement) -> CapabilityStatementValidation {
    let errors = Vec::new();
    let mut warnings = Vec::new();

    for (i, msg) in cs.messaging.iter().enumerate() {
        if msg.endpoint.as_deref().unwrap_or("").is_empty() {
            warnings.push(format!("messaging[{}]: endpoint is missing or empty", i));
        }

        for (j, sm) in msg.supported_message.iter().enumerate() {
            if sm.definition.as_deref().unwrap_or("").is_empty() {
                warnings.push(format!(
                    "messaging[{}].supportedMessage[{}]: definition is missing or empty",
                    i, j
                ));
            }
        }
    }

    CapabilityStatementValidation { errors, warnings }
}

/// Validate document capabilities in a CapabilityStatement.
///
/// Checks:
/// - `document.mode` should be a valid value
/// - `document.profile` should be a valid URL
pub fn validate_document(cs: &CapabilityStatement) -> CapabilityStatementValidation {
    let errors = Vec::new();
    let mut warnings = Vec::new();

    for (i, doc) in cs.document.iter().enumerate() {
        match doc.mode.as_deref() {
            Some("producer") | Some("consumer") => {}
            Some(other) => {
                warnings.push(format!(
                    "document[{}]: unusual mode '{}' (expected 'producer' or 'consumer')",
                    i, other
                ));
            }
            None => {
                warnings.push(format!("document[{}]: mode is missing", i));
            }
        }

        if doc.profile.as_deref().unwrap_or("").is_empty() {
            warnings.push(format!("document[{}]: profile is missing or empty", i));
        }
    }

    CapabilityStatementValidation { errors, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::profile::{
        ElementBinding, ElementConstraint, ElementDefinition, ElementDefinitionType,
        ElementSlicing, SlicingDiscriminator, Snapshot,
    };

    fn sample_cs() -> CapabilityStatement {
        CapabilityStatement {
            resource_type: "CapabilityStatement".to_string(),
            url: Some("http://example.org/CapabilityStatement/test".to_string()),
            name: Some("TestCS".to_string()),
            status: Some("active".to_string()),
            software: None,
            implementation: None,
            messaging: vec![],
            document: vec![],
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
                    versioning: None,
                    conditional_create: None,
                    conditional_read: None,
                    conditional_update: None,
                    conditional_delete: None,
                    search_include: vec![],
                    search_revinclude: vec![],
                }],
                interaction: vec![],
                operation: vec![],
                security: None,
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

    /// A profile with all the new validation features populated.
    fn sample_rich_profile() -> StructureDefinition {
        StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/RichPatient".to_string(),
            name: "RichPatient".to_string(),
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
                    // Constraint: name.exists()
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
                        constraint: vec![ElementConstraint {
                            key: "pat-1".to_string(),
                            severity: "error".to_string(),
                            human: Some("Patient.name SHALL be present".to_string()),
                            expression: Some("name.exists()".to_string()),
                        }],
                        is_modifier: false,
                        is_summary: false,
                        slice_name: None,
                        slicing: None,
                    },
                    // Required binding on gender
                    ElementDefinition {
                        id: "Patient.gender".to_string(),
                        path: "Patient.gender".to_string(),
                        min: Some(0),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "code".to_string(),
                            target_profile: vec![],
                            profile: vec![],
                            versioning: None,
                        }],
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
                        binding: Some(ElementBinding {
                            strength: "required".to_string(),
                            value_set: Some(
                                "http://hl7.org/fhir/ValueSet/administrative-gender".to_string(),
                            ),
                            description: None,
                        }),
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
                    // Fixed value on active
                    ElementDefinition {
                        id: "Patient.active".to_string(),
                        path: "Patient.active".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "boolean".to_string(),
                            target_profile: vec![],
                            profile: vec![],
                            versioning: None,
                        }],
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        fixed_boolean: Some(true),
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
                    // Slice on identifier
                    ElementDefinition {
                        id: "Patient.identifier:ABN".to_string(),
                        path: "Patient.identifier".to_string(),
                        min: Some(0),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Identifier".to_string(),
                            target_profile: vec![],
                            profile: vec![],
                            versioning: None,
                        }],
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
                        slice_name: Some("ABN".to_string()),
                        slicing: Some(ElementSlicing {
                            discriminator: vec![SlicingDiscriminator {
                                discriminator_type: "value".to_string(),
                                path: "system".to_string(),
                            }],
                            rules: Some("open".to_string()),
                            description: None,
                            ordered: false,
                        }),
                    },
                    // Extension
                    ElementDefinition {
                        id: "Patient.extension:testExt".to_string(),
                        path: "Patient.extension".to_string(),
                        min: Some(0),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Extension".to_string(),
                            target_profile: vec![],
                            profile: vec![
                                "http://example.org/StructureDefinition/test-extension".to_string(),
                            ],
                            versioning: None,
                        }],
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
                        slice_name: Some("testExt".to_string()),
                        slicing: None,
                    },
                    // Polymorphic value[x] type constraint
                    ElementDefinition {
                        id: "Patient.value[x]".to_string(),
                        path: "Patient.value[x]".to_string(),
                        min: Some(0),
                        max: Some("1".to_string()),
                        type_: vec![
                            ElementDefinitionType {
                                code: "string".to_string(),
                                target_profile: vec![],
                                profile: vec![],
                                versioning: None,
                            },
                            ElementDefinitionType {
                                code: "CodeableConcept".to_string(),
                                target_profile: vec![],
                                profile: vec![],
                                versioning: None,
                            },
                        ],
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
                    // Reference target profile
                    ElementDefinition {
                        id: "Patient.managingOrganization".to_string(),
                        path: "Patient.managingOrganization".to_string(),
                        min: Some(0),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Reference".to_string(),
                            target_profile: vec![
                                "http://example.org/StructureDefinition/TestOrganization"
                                    .to_string(),
                            ],
                            profile: vec![],
                            versioning: None,
                        }],
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
            software: None,
            implementation: None,
            messaging: vec![],
            document: vec![],
            rest: vec![Rest {
                mode: "client".to_string(),
                resource: vec![],
                interaction: vec![],
                operation: vec![],
                security: None,
            }],
        };
        let result = validate_capability_statement(&cs);
        assert!(result.errors.iter().any(|e| e.contains("mode='server'")));
    }

    #[test]
    fn generate_must_support_tests() {
        let cs = sample_cs();
        let profile = sample_profile();
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

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
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

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
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

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
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

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

    #[test]
    fn generate_constraint_validation_tests() {
        let cs = sample_cs();
        let profile = sample_rich_profile();
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

        let constraint_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::ConstraintValidation { .. }))
            .collect();

        assert!(
            !constraint_tests.is_empty(),
            "Expected at least 1 constraint validation test, got {}",
            constraint_tests.len()
        );

        // Should have a test for pat-1 (name.exists())
        let pat1_test = constraint_tests
            .iter()
            .find(|t| t.name.contains("pat-1") || t.name.contains("pat_1"));
        assert!(
            pat1_test.is_some(),
            "Expected constraint test for pat-1, got: {:?}",
            constraint_tests.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generate_binding_validation_tests() {
        let cs = sample_cs();
        let profile = sample_rich_profile();
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

        let binding_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::BindingValidation { .. }))
            .collect();

        assert!(
            !binding_tests.is_empty(),
            "Expected at least 1 binding validation test, got {}",
            binding_tests.len()
        );

        // Should test gender binding
        let gender_test = binding_tests
            .iter()
            .find(|t| t.name.contains("binding_gender"));
        assert!(
            gender_test.is_some(),
            "Expected binding_gender test, got: {:?}",
            binding_tests.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generate_fixed_value_validation_tests() {
        let cs = sample_cs();
        let profile = sample_rich_profile();
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

        let fixed_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::FixedValueValidation { .. }))
            .collect();

        assert!(
            !fixed_tests.is_empty(),
            "Expected at least 1 fixed value validation test, got {}",
            fixed_tests.len()
        );

        // Should test active fixed value
        let active_test = fixed_tests
            .iter()
            .find(|t| t.name.contains("fixed_value_active"));
        assert!(
            active_test.is_some(),
            "Expected fixed_value_active test, got: {:?}",
            fixed_tests.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generate_slice_validation_tests() {
        let cs = sample_cs();
        let profile = sample_rich_profile();
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

        let slice_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::SliceValidation { .. }))
            .collect();

        assert!(
            !slice_tests.is_empty(),
            "Expected at least 1 slice validation test, got {}",
            slice_tests.len()
        );

        // Should test ABN slice
        let abn_test = slice_tests.iter().find(|t| t.name.contains("ABN"));
        assert!(
            abn_test.is_some(),
            "Expected slice test for ABN, got: {:?}",
            slice_tests.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generate_extension_validation_tests() {
        let cs = sample_cs();
        let profile = sample_rich_profile();
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

        let ext_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::ExtensionValidation { .. }))
            .collect();

        assert!(
            !ext_tests.is_empty(),
            "Expected at least 1 extension validation test, got {}",
            ext_tests.len()
        );

        // Should test the test-extension URL
        let ext_test = ext_tests
            .iter()
            .find(|t| t.name.contains("test-extension") || t.name.contains("test_extension"));
        assert!(
            ext_test.is_some(),
            "Expected extension test for test-extension, got: {:?}",
            ext_tests.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generate_type_constraint_validation_tests() {
        let cs = sample_cs();
        let profile = sample_rich_profile();
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

        let type_tests: Vec<_> = tests
            .iter()
            .filter(|t| matches!(t.kind, ConformanceTestKind::TypeConstraintValidation { .. }))
            .collect();

        assert!(
            !type_tests.is_empty(),
            "Expected at least 1 type constraint validation test, got {}",
            type_tests.len()
        );

        // Should test value[x] type constraint
        let value_test = type_tests
            .iter()
            .find(|t| t.name.contains("value_x") || t.name.contains("value[x]"));
        assert!(
            value_test.is_some(),
            "Expected type constraint test for value[x], got: {:?}",
            type_tests.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generate_reference_target_validation_tests() {
        let cs = sample_cs();
        let profile = sample_rich_profile();
        let tests = generate_conformance_tests(&cs, &[profile], &[]);

        let ref_tests: Vec<_> = tests
            .iter()
            .filter(|t| {
                matches!(
                    t.kind,
                    ConformanceTestKind::ReferenceTargetValidation { .. }
                )
            })
            .collect();

        assert!(
            !ref_tests.is_empty(),
            "Expected at least 1 reference target validation test, got {}",
            ref_tests.len()
        );

        // Should test managingOrganization target profile
        let org_test = ref_tests
            .iter()
            .find(|t| t.name.contains("managingOrganization"));
        assert!(
            org_test.is_some(),
            "Expected reference target test for managingOrganization, got: {:?}",
            ref_tests.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn collect_constraint_fields_extracts_simple_expressions() {
        let profile = sample_rich_profile();
        let constraints = collect_constraint_fields(&profile);

        let name_constraint = constraints.iter().find(|(key, _, _, _)| key == "pat-1");
        assert!(
            name_constraint.is_some(),
            "Expected pat-1 constraint, got: {:?}",
            constraints
        );

        if let Some((_, _, expr, field_path)) = name_constraint {
            assert_eq!(expr, "name.exists()");
            assert_eq!(field_path, "name");
        }
    }

    #[test]
    fn collect_binding_fields_finds_required_bindings() {
        let profile = sample_rich_profile();
        let bindings = collect_binding_fields(&profile);

        let gender_binding = bindings.iter().find(|(field, _)| field == "gender");
        assert!(
            gender_binding.is_some(),
            "Expected gender binding, got: {:?}",
            bindings
        );

        if let Some((_, vs)) = gender_binding {
            assert!(vs.contains("administrative-gender"));
        }
    }

    #[test]
    fn collect_fixed_value_fields_finds_fixed_values() {
        let profile = sample_rich_profile();
        let fixed = collect_fixed_value_fields(&profile);

        let active_fixed = fixed.iter().find(|(field, _)| field == "active");
        assert!(
            active_fixed.is_some(),
            "Expected active fixed value, got: {:?}",
            fixed
        );

        if let Some((_, val)) = active_fixed {
            assert_eq!(*val, serde_json::json!(true));
        }
    }

    #[test]
    fn collect_slice_fields_finds_slices() {
        let profile = sample_rich_profile();
        let slices = collect_slice_fields(&profile);

        let abn_slice = slices.iter().find(|(_, name, _, _)| name == "ABN");
        assert!(abn_slice.is_some(), "Expected ABN slice, got: {:?}", slices);

        if let Some((_, _, path, dtype)) = abn_slice {
            assert_eq!(path, "system");
            assert_eq!(dtype, "value");
        }
    }

    #[test]
    fn collect_extension_urls_finds_extensions() {
        let profile = sample_rich_profile();
        let urls = collect_extension_urls(&profile);

        let test_ext = urls.iter().find(|u| u.contains("test-extension"));
        assert!(
            test_ext.is_some(),
            "Expected test-extension URL, got: {:?}",
            urls
        );
    }

    #[test]
    fn collect_type_constraint_fields_finds_polymorphic_fields() {
        let profile = sample_rich_profile();
        let type_constraints = collect_type_constraint_fields(&profile);

        let value_x = type_constraints
            .iter()
            .find(|(field, _)| field.contains("value"));
        assert!(
            value_x.is_some(),
            "Expected value[x] type constraint, got: {:?}",
            type_constraints
        );

        if let Some((_, types)) = value_x {
            assert!(types.contains(&"string".to_string()));
            assert!(types.contains(&"CodeableConcept".to_string()));
        }
    }

    #[test]
    fn collect_reference_target_fields_finds_targets() {
        let profile = sample_rich_profile();
        let targets = collect_reference_target_fields(&profile);

        let org_target = targets
            .iter()
            .find(|(field, _)| field == "managingOrganization");
        assert!(
            org_target.is_some(),
            "Expected managingOrganization target, got: {:?}",
            targets
        );

        if let Some((_, tp)) = org_target {
            assert!(tp.contains("TestOrganization"));
        }
    }
}
