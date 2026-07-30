use crate::generate::model::*;
use crate::generate::value_resolver::resolve_search_value;
use crate::model::*;
use std::collections::HashMap;

/// Build a ResponseAssertion appropriate for the test case kind.
pub fn assertion_for_kind(kind: &TestCaseKind, resource_type: &str) -> Option<ResponseAssertion> {
    match kind {
        // CRUD interactions: expect a single resource, not a Bundle
        TestCaseKind::Interaction => None,

        // Search single param: expect Bundle searchset with at least 0 entries
        // (may be 0 if test data doesn't match; we check Bundle structure)
        TestCaseKind::SearchSingle { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0), // valid to get 0 results for test values
            ..ResponseAssertion::none()
        }),

        // Search with modifier: expect Bundle searchset
        TestCaseKind::SearchModifier { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0),
            ..ResponseAssertion::none()
        }),

        // Search with prefix: expect Bundle searchset
        TestCaseKind::SearchPrefix { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0),
            ..ResponseAssertion::none()
        }),

        // Near/proximity search: expect Bundle searchset
        TestCaseKind::SearchNear { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0),
            ..ResponseAssertion::none()
        }),

        // Combo search: expect Bundle searchset
        TestCaseKind::SearchCombo { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0),
            ..ResponseAssertion::none()
        }),

        // Chained search: expect Bundle searchset
        TestCaseKind::SearchChained { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0),
            ..ResponseAssertion::none()
        }),

        // Include: expect Bundle searchset with the included resource type present
        TestCaseKind::Include { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0),
            ..ResponseAssertion::none()
        }),

        // _summary: expect Bundle searchset, resources should lack text field
        // but still include id, meta, and resourceType
        TestCaseKind::ResultParam { param } => match param.as_str() {
            "_summary" => {
                let mut required = HashMap::new();
                required.insert(
                    resource_type.to_string(),
                    vec!["id".to_string(), "meta".to_string()],
                );
                Some(ResponseAssertion {
                    bundle_type: Some("searchset".to_string()),
                    min_entries: Some(0),
                    absent_fields: vec!["text".to_string()],
                    required_fields: required,
                    ..ResponseAssertion::none()
                })
            }
            "_count" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                max_entries: Some(1), // we request _count=1
                ..ResponseAssertion::none()
            }),
            _ => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(0),
                ..ResponseAssertion::none()
            }),
        },

        // $operation: varies, minimal assertion
        TestCaseKind::Operation { .. } => Some(ResponseAssertion {
            response_contains_key: Some("resourceType".to_string()),
            ..ResponseAssertion::none()
        }),

        // Negative tests: expected_status already encodes what HTTP response
        // to accept.  Do NOT assert OperationOutcome severity here because:
        //  - read_nonexistent expects 404 (no body)
        //  - search_invalid_param accepts either reject or ignore
        // If a server returns an OperationOutcome, that's fine but not required.
        TestCaseKind::Negative { .. } => None,

        // Conformance tests carry their own assertions
        TestCaseKind::Conformance { .. } => None,
    }
}

/// Generate a test plan from a CapabilityStatement's rest resources, their
/// supported interactions, search parameters, and operations.
///
/// Generates:
/// - CRUD interaction tests (read, create, update, etc.)
/// - Single search param tests
/// - Search param modifier tests (:exact, :contains, :missing, etc.)
/// - Search prefix tests for number/date/quantity (eq, ne, gt, lt, etc.)
/// - Combinatorial search param tests (all 2-combos within a resource)
/// - Chained search tests (reference param → target param)
/// - _include / _revinclude tests
/// - Result parameter tests (_summary, _count, _sort, _elements)
/// - $operation tests from OperationDefinition
/// - Negative / error tests
///
/// `field_values` maps resource_type → field_path → value, extracted from
/// generated resources. `created_ids` maps resource_type → server-assigned ID.
/// These are used to embed real values in test URLs instead of sentinel placeholders.
pub fn generate_test_plan(
    cs: &CapabilityStatement,
    _profiles: &[StructureDefinition],
    search_params: &[SearchParameter],
    operations: Option<&[OperationDefinition]>,
    ig_url: Option<&str>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestPlan {
    let mut test_groups = Vec::new();

    for rest in &cs.rest {
        if rest.mode != "server" {
            continue;
        }

        for resource in &rest.resource {
            // Skip non-resource types (e.g. Parameters) that are declared
            // in the CapabilityStatement but are not persistable resources.
            if super::NON_RESOURCE_TYPES.contains(&resource.resource_type.as_str()) {
                continue;
            }

            let profile_url = resource
                .supported_profile
                .first()
                .cloned()
                .or_else(|| resource.profile.clone());

            let group = build_test_group(
                resource,
                &profile_url,
                search_params,
                operations,
                field_values,
                created_ids,
            );
            test_groups.push(group);
        }

        // System-level operations (e.g., $export at the rest level)
        for op in &rest.operation {
            let op_def = operations.and_then(|ops| ops.iter().find(|o| o.code == op.name));

            let mut test = build_operation_test(
                "", // system-level, no resource type prefix
                &op.name,
                op_def,
                &None,
                field_values,
                created_ids,
            );
            test.validation.response_assertion =
                assertion_for_kind(&test.kind, &test.resource_type);

            test_groups.push(TestGroup {
                resource_type: format!("$${}", op.name),
                profile_url: None,
                tests: vec![test],
            });
        }
    }

    let name = cs.name.clone().unwrap_or_else(|| "Unnamed IG".to_string());

    TestPlan {
        name,
        ig_url: ig_url.map(|s| s.to_string()),
        test_groups,
        creation_order: Vec::new(),
        setup_resources: HashMap::new(),
    }
}

fn build_test_group(
    resource: &RestResource,
    profile_url: &Option<String>,
    search_params: &[SearchParameter],
    operations: Option<&[OperationDefinition]>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestGroup {
    let mut tests = Vec::new();
    let has_search_type = resource.interaction.iter().any(|i| i.code == "search-type");
    let has_read = resource.interaction.iter().any(|i| i.code == "read");

    // --- Interaction tests (CRUD) ---
    for interaction in &resource.interaction {
        let interaction_type = match Interaction::from_code(&interaction.code) {
            Some(i) => i,
            None => continue,
        };
        // Skip search-type here; we handle it separately below
        if matches!(interaction_type, Interaction::SearchType) {
            continue;
        }
        let test_case =
            build_interaction_test(&resource.resource_type, &interaction_type, profile_url);
        tests.push(test_case);
    }

    if has_search_type {
        // --- Search param tests ---
        // Find all SearchParameters applicable to this resource type
        let resource_search_params: Vec<&SearchParameter> = search_params
            .iter()
            .filter(|sp| sp.base.contains(&resource.resource_type))
            .collect();

        // Also include inline search params from the CapabilityStatement
        let inline_params: Vec<RestSearchParam> = resource.search_param.clone();

        // --- Single param tests (from inline CS params) ---
        for sp in &inline_params {
            // Basic single-param test
            tests.push(build_search_single_test(
                &resource.resource_type,
                &sp.name,
                &sp.param_type,
                profile_url,
                field_values,
                created_ids,
            ));

            // Modifier tests
            let modifiers = SearchModifier::applicable_to(&sp.param_type);
            for modifier in modifiers {
                tests.push(build_search_modifier_test(
                    &resource.resource_type,
                    &sp.name,
                    &sp.param_type,
                    &modifier,
                    profile_url,
                    field_values,
                    created_ids,
                ));
            }

            // Prefix tests (number, date, quantity)
            let prefixes = SearchPrefix::applicable_to(&sp.param_type);
            for prefix in prefixes {
                tests.push(build_search_prefix_test(
                    &resource.resource_type,
                    &sp.name,
                    &sp.param_type,
                    &prefix,
                    profile_url,
                    field_values,
                    created_ids,
                ));
            }

            // Near/proximity tests for the canonical near parameter.
            if sp.param_type == "special" && sp.name.eq_ignore_ascii_case("near") {
                tests.push(build_search_near_test(
                    &resource.resource_type,
                    &sp.name,
                    profile_url,
                ));
            }
        }

        // Also generate tests from standalone SearchParameter resources
        for sp in &resource_search_params {
            // Avoid duplicates with inline params
            if inline_params.iter().any(|ip| ip.name == sp.code) {
                continue;
            }
            tests.push(build_search_single_from_sp(
                &resource.resource_type,
                sp,
                profile_url,
                field_values,
                created_ids,
            ));

            // Modifier + prefix tests for standalone params too
            let modifiers = SearchModifier::applicable_to(&sp.param_type);
            for modifier in modifiers {
                tests.push(build_search_modifier_test(
                    &resource.resource_type,
                    &sp.code,
                    &sp.param_type,
                    &modifier,
                    profile_url,
                    field_values,
                    created_ids,
                ));
            }

            let prefixes = SearchPrefix::applicable_to(&sp.param_type);
            for prefix in prefixes {
                tests.push(build_search_prefix_test(
                    &resource.resource_type,
                    &sp.code,
                    &sp.param_type,
                    &prefix,
                    profile_url,
                    field_values,
                    created_ids,
                ));
            }

            // Near/proximity tests for standalone near parameters only.
            if sp.param_type == "special" && sp.code.eq_ignore_ascii_case("near") {
                tests.push(build_search_near_test(
                    &resource.resource_type,
                    &sp.code,
                    profile_url,
                ));
            }
        }

        // --- Combinatorial search tests (2-combinations) ---
        if inline_params.len() >= 2 {
            for i in 0..inline_params.len() {
                for j in (i + 1)..inline_params.len() {
                    tests.push(build_search_combo_test(
                        &resource.resource_type,
                        &[
                            (&inline_params[i].name, &inline_params[i].param_type),
                            (&inline_params[j].name, &inline_params[j].param_type),
                        ],
                        profile_url,
                        field_values,
                        created_ids,
                    ));
                }
            }
        }

        // --- Chained search tests (reference params → target param) ---
        for sp in &inline_params {
            if sp.param_type == "reference" {
                // Try to find target resource search params to chain into
                if let Some(target_type) = infer_reference_target(&sp.name, search_params) {
                    // Find search params for the target resource
                    let target_params: Vec<&SearchParameter> = search_params
                        .iter()
                        .filter(|tsp| tsp.base.contains(&target_type) && tsp.param_type == "string")
                        .take(2) // limit to 2 to avoid explosion
                        .collect();

                    for target_sp in target_params {
                        tests.push(build_chained_search_test(
                            &resource.resource_type,
                            &sp.name,
                            &target_sp.code,
                            profile_url,
                            field_values,
                            created_ids,
                        ));
                    }
                }
            }
        }

        // --- _include / _revinclude tests from CS declarations ---
        for include_spec in &resource.search_include {
            // Format: "ResourceName:paramName" e.g. "Organization:partOf"
            // FHIR search parameter codes are lowercase, so normalise the
            // param portion (e.g. "partOf" → "partof") to match the server.
            if let Some((_res, param)) = include_spec.split_once(':') {
                let expected_include_type =
                    infer_reference_target(&param.to_lowercase(), search_params);
                tests.push(build_include_test(
                    &resource.resource_type,
                    &param.to_lowercase(),
                    false,
                    None,
                    expected_include_type,
                    profile_url,
                ));
            }
        }
        for revinclude_spec in &resource.search_revinclude {
            // Format: "ResourceName:paramName" e.g. "Location:organization"
            if let Some((res, param)) = revinclude_spec.split_once(':') {
                // For _revinclude, we can't guarantee that test data has
                // resources referencing the queried primary resource, so
                // we don't set expected_include_type or
                // include_requires_distinct_from — we only verify the
                // server returns a valid search Bundle.
                tests.push(build_include_test(
                    &resource.resource_type,
                    &param.to_lowercase(),
                    true,
                    Some(res),
                    None,
                    profile_url,
                ));
            }
        }

        // --- Result parameter tests ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_summary",
            "true",
            profile_url,
            &inline_params,
            created_ids,
        ));
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_count",
            "1",
            profile_url,
            &inline_params,
            created_ids,
        ));
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_sort",
            "_lastUpdated",
            profile_url,
            &inline_params,
            created_ids,
        ));
    }

    // --- $operation tests from CS rest.operation ---
    for op in &resource.operation {
        let op_def = operations.and_then(|ops| ops.iter().find(|o| o.code == op.name));

        tests.push(build_operation_test(
            &resource.resource_type,
            &op.name,
            op_def,
            profile_url,
            field_values,
            created_ids,
        ));
    }

    // --- Negative / error tests ---
    let rt = &resource.resource_type;
    if has_read {
        tests.push(build_negative_test(
            rt,
            "read_nonexistent",
            "GET",
            &format!("/{rt}/nonexistent-id-99999"),
            404,
            profile_url,
        ));
    }
    // Per FHIR spec, unknown search parameters may be ignored (2xx Bundle)
    // or rejected (4xx OperationOutcome), so we accept either behaviour.
    if has_search_type {
        tests.push(build_negative_test(
            rt,
            "search_invalid_param",
            "GET",
            &format!("/{rt}?__invalid_param__=value"),
            0,
            profile_url,
        ));
    }

    // Stamp response assertions based on test kind
    for test in &mut tests {
        if test.validation.response_assertion.is_none() {
            test.validation.response_assertion =
                assertion_for_kind(&test.kind, &test.resource_type);
        }
    }

    TestGroup {
        resource_type: resource.resource_type.clone(),
        profile_url: profile_url.clone(),
        tests,
    }
}

// ─── Test builders ────────────────────────────────────────────────────────────

fn build_interaction_test(
    resource_type: &str,
    interaction: &Interaction,
    profile_url: &Option<String>,
) -> TestCase {
    let base_path = format!("/{resource_type}");
    let id_path = format!("/{resource_type}/{{id}}");

    let (method, url, expected_status) = match interaction {
        Interaction::Read => ("GET", id_path.clone(), 200),
        Interaction::Vread => ("GET", format!("{id_path}/_history/1"), 200),
        Interaction::Create => ("POST", base_path, 201),
        Interaction::Update => ("PUT", id_path.clone(), 200),
        Interaction::Patch => ("PATCH", id_path.clone(), 200),
        Interaction::Delete => ("DELETE", id_path.clone(), 204),
        Interaction::SearchType => ("GET", format!("{base_path}?_count=1"), 200),
        Interaction::HistoryInstance => ("GET", format!("{id_path}/_history"), 200),
        Interaction::HistoryType => ("GET", format!("{base_path}/_history"), 200),
        Interaction::Operation(name) => ("POST", format!("/{resource_type}/${name}"), 200),
    };

    let required_elements = if matches!(interaction, Interaction::Read | Interaction::Create) {
        vec![format!("{resource_type}.id")]
    } else {
        Vec::new()
    };

    let validate_profile = matches!(
        interaction,
        Interaction::Read | Interaction::Create | Interaction::Update
    );

    TestCase {
        name: format!("{}_{}", resource_type.to_lowercase(), interaction.label()),
        kind: TestCaseKind::Interaction,
        interaction: interaction.clone(),
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: method.to_string(),
            url,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status,
            profile_url: if validate_profile {
                profile_url.clone()
            } else {
                None
            },
            required_elements,
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Resolve a search parameter value from generated resources, falling back
/// to a sentinel value if no real value is available.
fn resolve_param_value(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> String {
    let rt_values = field_values.get(resource_type);
    let empty = HashMap::new();
    let rt_values = rt_values.unwrap_or(&empty);
    let value = resolve_search_value(
        resource_type,
        param_name,
        param_type,
        rt_values,
        created_ids,
    );
    value.unwrap_or_else(|| sample_value(param_type).to_string())
}

/// Sample value for a search param based on its type.
/// Used as fallback when no generated resource value is available.
fn sample_value(param_type: &str) -> &'static str {
    match param_type {
        "string" => "test-value",
        "exact" => "test-value",
        "token" => "test-code",
        "reference" => "Patient/test-id",
        "number" => "1",
        "date" => "2024-01-01",
        "dateTime" => "2024-01-01T00:00:00Z",
        "quantity" => "5.0||http://unitsofmeasure.org|kg",
        "uri" => "http://example.org",
        "composite" => "test-value",
        "special" => "-25.0%7C133.0%7C3000%7Ckm", // near format: lat%7Clon%7Cdistance%7Cunits (pipes must be %-encoded for HAPI)
        _ => "test-value",
    }
}

fn build_search_single_test(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let value = resolve_param_value(
        resource_type,
        param_name,
        param_type,
        field_values,
        created_ids,
    );
    let url = format!("/{resource_type}?{param_name}={value}&_id={{id}}");

    TestCase {
        name: format!(
            "{}_search_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_")
        ),
        kind: TestCaseKind::SearchSingle {
            param_name: param_name.to_string(),
            param_type: param_type.to_string(),
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(0),
                ..ResponseAssertion::none()
            }),
        },
    }
}

fn build_search_single_from_sp(
    resource_type: &str,
    sp: &SearchParameter,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    build_search_single_test(
        resource_type,
        &sp.code,
        &sp.param_type,
        profile_url,
        field_values,
        created_ids,
    )
}

fn build_search_modifier_test(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    modifier: &SearchModifier,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let value = resolve_param_value(
        resource_type,
        param_name,
        param_type,
        field_values,
        created_ids,
    );
    let url = if matches!(modifier, SearchModifier::Missing) {
        // :missing takes true/false
        format!("/{resource_type}?{param_name}:missing=true")
    } else {
        format!("/{resource_type}?{param_name}{}={value}", modifier.suffix())
    };

    TestCase {
        name: format!(
            "{}_search_{}_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_"),
            format!("{:?}", modifier).to_lowercase()
        ),
        kind: TestCaseKind::SearchModifier {
            param_name: param_name.to_string(),
            modifier: modifier.clone(),
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

fn build_search_prefix_test(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    prefix: &SearchPrefix,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let value = resolve_param_value(
        resource_type,
        param_name,
        param_type,
        field_values,
        created_ids,
    );
    let url = format!(
        "/{resource_type}?{param_name}={prefix}{value}",
        prefix = prefix.prefix_str()
    );

    TestCase {
        name: format!(
            "{}_search_{}_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_"),
            prefix.prefix_str()
        ),
        kind: TestCaseKind::SearchPrefix {
            param_name: param_name.to_string(),
            prefix: prefix.clone(),
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build a proximity/near search test for FHIR special-type params (e.g. Location?near).
///
/// FHIR near format: `?near=lat|lon[|distance[|units]]`
/// Use a conservative lat|lon payload for broader server compatibility.
fn build_search_near_test(
    resource_type: &str,
    param_name: &str,
    profile_url: &Option<String>,
) -> TestCase {
    // Include distance|units — HAPI FHIR R4 requires all four components.
    // Pipes must be %-encoded (%7C) because HAPI's servlet rejects bare | in query strings.
    let url = format!("/{resource_type}?{param_name}=-25.0%7C133.0%7C3000%7Ckm");

    TestCase {
        name: format!(
            "{}_search_{}_near_10km",
            resource_type.to_lowercase(),
            param_name.replace('-', "_"),
        ),
        kind: TestCaseKind::SearchNear {
            param_name: param_name.to_string(),
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

fn build_search_combo_test(
    resource_type: &str,
    params: &[(&str, &str)],
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let query: Vec<String> = params
        .iter()
        .map(|(name, ptype)| {
            let value = resolve_param_value(resource_type, name, ptype, field_values, created_ids);
            format!("{}={}", name, value)
        })
        .collect();
    let url = format!("/{}?{}", resource_type, query.join("&"));

    let param_names: Vec<String> = params.iter().map(|(n, _)| n.to_string()).collect();

    TestCase {
        name: format!(
            "{}_search_combo_{}",
            resource_type.to_lowercase(),
            param_names.join("_and_").replace('-', "_")
        ),
        kind: TestCaseKind::SearchCombo {
            params: param_names,
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

fn build_chained_search_test(
    resource_type: &str,
    chain_param: &str,
    target_param: &str,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    // For chained searches, resolve the target param value from the target resource type.
    // We don't know the target resource type at this point, so use the chain param's
    // reference target as a hint.
    let value = resolve_param_value(
        resource_type,
        target_param,
        "string",
        field_values,
        created_ids,
    );
    let url = format!("/{resource_type}?{chain_param}.{target_param}={value}");

    TestCase {
        name: format!(
            "{}_search_chain_{}_{}",
            resource_type.to_lowercase(),
            chain_param.replace('-', "_"),
            target_param.replace('-', "_")
        ),
        kind: TestCaseKind::SearchChained {
            chain_param: chain_param.to_string(),
            target_param: target_param.to_string(),
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

fn build_include_test(
    resource_type: &str,
    param_name: &str,
    revinclude: bool,
    source_resource: Option<&str>,
    expected_include_type: Option<String>,
    profile_url: &Option<String>,
) -> TestCase {
    let param = if revinclude {
        "_revinclude"
    } else {
        "_include"
    };
    let target = if revinclude {
        let source = source_resource.unwrap_or("Patient");
        format!("{}:{}", source, param_name)
    } else {
        format!("{resource_type}:{param_name}")
    };
    let url = format!("/{resource_type}?{param}={target}&_id={{id}}");

    let mut include_types = HashMap::new();
    if let Some(include_type) = expected_include_type {
        include_types.insert(include_type, param_name.to_string());
    }
    // For _include: when we don't know the target type, require at least
    // one distinct resource type in the bundle.
    // For _revinclude: we can't guarantee test data has resources
    // referencing the queried primary resource, so don't assert
    // distinct types — just verify the server returns a valid Bundle.
    let include_requires_distinct_from = if revinclude {
        None
    } else if include_types.is_empty() {
        Some(resource_type.to_string())
    } else {
        None
    };

    TestCase {
        name: format!(
            "{}_{}_{}",
            resource_type.to_lowercase(),
            if revinclude { "revinclude" } else { "include" },
            param_name.replace('-', "_")
        ),
        kind: TestCaseKind::Include {
            param: param_name.to_string(),
            revinclude,
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(0),
                include_types,
                include_requires_distinct_from,
                ..ResponseAssertion::none()
            }),
        },
    }
}

fn build_result_param_test(
    resource_type: &str,
    param: &str,
    value: &str,
    profile_url: &Option<String>,
    declared_params: &[RestSearchParam],
    created_ids: &HashMap<String, String>,
) -> Vec<TestCase> {
    // For _sort, determine the actual sort field based on declared params
    let (actual_param, actual_value, sort_field): (&str, String, Option<String>) =
        if param == "_sort" {
            // Check if _lastUpdated is declared as a search param for this resource
            let has_last_updated = declared_params.iter().any(|sp| {
                sp.name.eq_ignore_ascii_case("_lastUpdated")
                    || sp.name.eq_ignore_ascii_case("lastUpdated")
            });

            if has_last_updated {
                (
                    "_sort",
                    "_lastUpdated".to_string(),
                    Some("_lastUpdated".to_string()),
                )
            } else {
                // Find first string or date param to use as fallback
                let fallback = declared_params
                    .iter()
                    .find(|sp| sp.param_type == "string" || sp.param_type == "date")
                    .map(|sp| sp.name.clone());

                let fb = match fallback {
                    Some(fb) => fb,
                    None => return Vec::new(), // No suitable param, skip sort test
                };
                ("_sort", fb.clone(), Some(fb))
            }
        } else {
            (param, value.to_string(), None)
        };

    let base_name = if param == "_sort" {
        format!(
            "{}_result_sort_{}",
            resource_type.to_lowercase(),
            actual_value.replace('-', "_")
        )
    } else {
        format!(
            "{}_result_{}",
            resource_type.to_lowercase(),
            param.trim_start_matches('_')
        )
    };

    let mut tests = Vec::new();

    // Test A: with real resource ID (uses {id} placeholder resolved at runtime)
    // This exercises the result param behaviour on actual data.
    if created_ids.contains_key(resource_type) {
        let url = format!("/{resource_type}?{actual_param}={actual_value}&_id={{id}}");

        let response_assertion = match param {
            "_count" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(1),
                max_entries: Some(1),
                ..ResponseAssertion::none()
            }),
            "_summary" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(1),
                absent_fields: vec!["text".to_string()],
                ..ResponseAssertion::none()
            }),
            "_sort" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                sort_by: Some(SortAssertion {
                    field: sort_field
                        .clone()
                        .unwrap_or_else(|| "_lastUpdated".to_string()),
                    direction: "asc".to_string(),
                }),
                ..ResponseAssertion::none()
            }),
            _ => None,
        };

        tests.push(TestCase {
            name: base_name.clone(),
            kind: TestCaseKind::ResultParam {
                param: param.to_string(),
            },
            interaction: Interaction::SearchType,
            resource_type: resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url,
                headers: HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 200,
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion,
            },
        });
    }

    // Test B: with nonexistent ID — always returns empty Bundle
    // Verifies Bundle structure on empty results.
    let url_empty =
        format!("/{resource_type}?{actual_param}={actual_value}&_id=nonexistent-id-99999");

    let response_assertion_empty = if param == "_sort" {
        Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            sort_by: Some(SortAssertion {
                field: sort_field.unwrap_or_else(|| "_lastUpdated".to_string()),
                direction: "asc".to_string(),
            }),
            ..ResponseAssertion::none()
        })
    } else {
        None
    };

    tests.push(TestCase {
        name: format!("{}_empty", base_name),
        kind: TestCaseKind::ResultParam {
            param: param.to_string(),
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url: url_empty,
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: response_assertion_empty,
        },
    });

    tests
}

fn build_operation_test(
    resource_type: &str,
    code: &str,
    op_def: Option<&OperationDefinition>,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    // Build request body from operation parameters
    let has_required_input_params = op_def
        .map(|def| {
            def.parameter
                .iter()
                .any(|p| p.use_.as_deref() == Some("in") && p.min.unwrap_or(0) > 0)
        })
        .unwrap_or(false);

    let body = if has_required_input_params {
        op_def.map(|def| {
            let mut params = serde_json::Map::new();
            params.insert(
                "resourceType".to_string(),
                serde_json::Value::String("Parameters".to_string()),
            );
            let mut param_array = Vec::new();
            for p in &def.parameter {
                if p.use_.as_deref() == Some("in") && p.min.unwrap_or(0) > 0 {
                    let mut param_obj = serde_json::Map::new();
                    param_obj.insert(
                        "name".to_string(),
                        serde_json::Value::String(p.name.clone()),
                    );
                    if let Some(ptype) = &p.param_type {
                        let value = resolve_param_value(
                            resource_type,
                            &p.name,
                            ptype,
                            field_values,
                            created_ids,
                        );
                        param_obj.insert("value".to_string(), serde_json::Value::String(value));
                    }
                    param_array.push(serde_json::Value::Object(param_obj));
                }
            }
            params.insert(
                "parameter".to_string(),
                serde_json::Value::Array(param_array),
            );
            serde_json::Value::Object(params)
        })
    } else {
        // No required input params — use GET with optional params as
        // query-string parameters instead of a POST body.  Many FHIR
        // operations (e.g. $export) only support GET and return 404/405
        // for POST.
        None
    };

    let method = if has_required_input_params {
        "POST"
    } else {
        "GET"
    };

    // Determine URL based on operation scope
    let url = match op_def {
        Some(def)
            if def.system.unwrap_or(false)
                && !def.type_.unwrap_or(false)
                && !def.instance.unwrap_or(false) =>
        {
            format!("/${code}")
        }
        Some(def) if def.instance.unwrap_or(false) => {
            format!("/{resource_type}/{{id}}/${code}")
        }
        Some(def) if def.type_.unwrap_or(false) => {
            format!("/{resource_type}/${code}")
        }
        _ => {
            tracing::warn!(
                "Unknown operation scope for {}, defaulting to resource-level",
                code
            );
            format!("/{resource_type}/${code}")
        }
    };

    let mut assertion = ResponseAssertion {
        response_contains_key: Some("resourceType".to_string()),
        response_resource_types: vec![
            "Bundle".to_string(),
            "Parameters".to_string(),
            "OperationOutcome".to_string(),
        ],
        ..ResponseAssertion::none()
    };
    if op_def
        .map(|d| {
            d.parameter
                .iter()
                .any(|p| p.use_.as_deref() == Some("out") && p.min.unwrap_or(0) > 0)
        })
        .unwrap_or(false)
    {
        assertion.response_contains_key = Some("parameter".to_string());
        assertion.response_resource_types =
            vec!["Parameters".to_string(), "OperationOutcome".to_string()];
    }

    TestCase {
        name: format!(
            "{}_operation_{}",
            resource_type.to_lowercase(),
            code.replace('-', "_")
        ),
        kind: TestCaseKind::Operation {
            code: code.to_string(),
        },
        interaction: Interaction::Operation(code.to_string()),
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: method.to_string(),
            url,
            headers: HashMap::new(),
            body,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: Some(assertion),
        },
    }
}

fn build_negative_test(
    resource_type: &str,
    description: &str,
    method: &str,
    url: &str,
    expected_status: u16,
    profile_url: &Option<String>,
) -> TestCase {
    TestCase {
        name: format!(
            "{}_negative_{}",
            resource_type.to_lowercase(),
            description.replace('-', "_")
        ),
        kind: TestCaseKind::Negative {
            description: description.to_string(),
        },
        interaction: Interaction::Read, // nominal; actual request may differ
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: method.to_string(),
            url: url.to_string(),
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Infer a reference target resource type from a SearchParameter's expression field.
///
/// Extracts the first resource type from the FHIRPath expression (e.g.,
/// `"Patient.name | Practitioner.name"` → `"Patient"`). Falls back to a
/// hardcoded mapping of common search parameter names when no SearchParameter
/// definition is found.
fn infer_reference_target(param_name: &str, search_params: &[SearchParameter]) -> Option<String> {
    // Try to find the SearchParameter by code and extract from its expression
    if let Some(sp) = search_params.iter().find(|sp| sp.code == param_name)
        && let Some(expression) = sp.expression.as_deref()
    {
        let types: Vec<&str> = expression
            .split('|')
            .filter_map(|part| {
                let part = part.trim();
                let rtype = part.split('.').next()?;
                if rtype.chars().next()?.is_uppercase() && !rtype.contains('-') {
                    Some(rtype)
                } else {
                    None
                }
            })
            .collect();
        if !types.is_empty() {
            return Some(types.first()?.to_string());
        }
    }

    // Fallback: hardcoded mapping for common FHIR search parameter names
    match param_name {
        "subject" | "patient" => Some("Patient".to_string()),
        "encounter" => Some("Encounter".to_string()),
        "organization" => Some("Organization".to_string()),
        "partof" => Some("Organization".to_string()),
        "practitioner" => Some("Practitioner".to_string()),
        "device" => Some("Device".to_string()),
        "location" => Some("Location".to_string()),
        "service" => Some("HealthcareService".to_string()),
        "endpoint" => Some("Endpoint".to_string()),
        "group" => Some("Group".to_string()),
        "specimen" => Some("Specimen".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_capability_statement() -> CapabilityStatement {
        CapabilityStatement {
            resource_type: "CapabilityStatement".to_string(),
            url: Some("http://example.org/CapabilityStatement/test".to_string()),
            name: Some("TestCS".to_string()),
            status: Some("active".to_string()),
            rest: vec![Rest {
                mode: "server".to_string(),
                resource: vec![RestResource {
                    resource_type: "Patient".to_string(),
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![
                        "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient"
                            .to_string(),
                    ],
                    interaction: vec![
                        RestInteraction {
                            code: "read".to_string(),
                        },
                        RestInteraction {
                            code: "search-type".to_string(),
                        },
                        RestInteraction {
                            code: "create".to_string(),
                        },
                        RestInteraction {
                            code: "update".to_string(),
                        },
                    ],
                    search_param: vec![
                        RestSearchParam {
                            name: "name".to_string(),
                            param_type: "string".to_string(),
                            definition: None,
                            documentation: None,
                        },
                        RestSearchParam {
                            name: "birthdate".to_string(),
                            param_type: "date".to_string(),
                            definition: None,
                            documentation: None,
                        },
                    ],
                    operation: vec![RestOperation {
                        name: "everything".to_string(),
                        definition: Some(
                            "http://hl7.org/fhir/OperationDefinition/Patient-everything"
                                .to_string(),
                        ),
                    }],
                    read_history: None,
                    update_create: None,
                    conditional_create: None,
                    conditional_read: None,
                    conditional_update: None,
                    conditional_delete: None,
                    search_include: vec!["Patient:organization".to_string()],
                    search_revinclude: vec!["Observation:subject".to_string()],
                }],
                interaction: vec![],
                operation: vec![RestOperation {
                    name: "export".to_string(),
                    definition: Some(
                        "http://hl7.org/fhir/uv/bulkdata/OperationDefinition/export".to_string(),
                    ),
                }],
            }],
        }
    }

    fn sample_search_params() -> Vec<SearchParameter> {
        vec![SearchParameter {
            resource_type: "SearchParameter".to_string(),
            url: "http://hl7.org/fhir/SearchParameter/individual-birthdate".to_string(),
            name: "birthdate".to_string(),
            code: "birthdate".to_string(),
            base: vec!["Patient".to_string()],
            param_type: "date".to_string(),
            expression: Some("Patient.birthDate".to_string()),
            description: None,
        }]
    }

    fn sample_operations() -> Vec<OperationDefinition> {
        vec![OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/OperationDefinition/Patient-everything".to_string(),
            name: "everything".to_string(),
            code: "everything".to_string(),
            system: Some(false),
            type_: Some(false),
            instance: Some(true),
            parameter: vec![OperationParameter {
                name: "start".to_string(),
                use_: Some("in".to_string()),
                min: Some(0),
                max: Some("1".to_string()),
                param_type: Some("date".to_string()),
            }],
        }]
    }

    #[test]
    fn generate_test_plan_from_capability_statement() {
        let cs = sample_capability_statement();
        let ops = sample_operations();
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(
            &cs,
            &[],
            &sample_search_params(),
            Some(&ops),
            None,
            &empty_fv,
            &empty_ids,
        );

        assert_eq!(plan.test_groups.len(), 2); // Patient group + system $export group
        let patient_group = plan
            .test_groups
            .iter()
            .find(|g| g.resource_type == "Patient")
            .expect("Should have Patient group");
        assert_eq!(patient_group.resource_type, "Patient");

        let group = patient_group;

        // Should have interaction tests
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::Interaction))
        );

        // Should have single search param tests
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::SearchSingle { .. }))
        );

        // Should have modifier tests (string params get :exact and :contains)
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::SearchModifier { .. }))
        );

        // Should have prefix tests (date params get eq, ne, gt, etc.)
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::SearchPrefix { .. }))
        );

        // Should have combo tests (name + birthdate)
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::SearchCombo { .. }))
        );

        // Should have operation tests ($everything)
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::Operation { .. }))
        );

        // Should have negative tests
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::Negative { .. }))
        );

        // Should have result param tests
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::ResultParam { .. }))
        );
    }

    #[test]
    fn test_case_count_is_comprehensive() {
        let cs = sample_capability_statement();
        let ops = sample_operations();
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(
            &cs,
            &[],
            &sample_search_params(),
            Some(&ops),
            None,
            &empty_fv,
            &empty_ids,
        );

        let group = &plan.test_groups[0];

        // Count by kind
        let interactions = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::Interaction))
            .count();
        let single_search = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::SearchSingle { .. }))
            .count();
        let modifiers = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::SearchModifier { .. }))
            .count();
        let prefixes = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::SearchPrefix { .. }))
            .count();
        let combos = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::SearchCombo { .. }))
            .count();
        let operations = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::Operation { .. }))
            .count();
        let negatives = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::Negative { .. }))
            .count();
        let result_params = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::ResultParam { .. }))
            .count();

        // Patient: read, create, update = 3 interactions (search-type handled separately)
        assert_eq!(interactions, 3);
        // 2 inline search params → 2 single tests
        assert_eq!(single_search, 2);
        // name is string → :exact, :contains (2); birthdate is date → :missing (1)
        // But birthdate also appears as a standalone SearchParameter, so we get its modifiers too
        assert!(
            modifiers >= 3,
            "Expected at least 3 modifier tests, got {}",
            modifiers
        );
        // birthdate is date → 9 prefixes
        assert!(
            prefixes >= 9,
            "Expected at least 9 prefix tests, got {}",
            prefixes
        );
        // 2 params → 1 combo (name+birthdate)
        assert_eq!(combos, 1);
        // 1 operation ($everything)
        assert_eq!(operations, 1);
        // 2 negative tests per resource
        assert_eq!(negatives, 2);
        // 3 result params (_summary, _count, _sort)
        assert_eq!(result_params, 3);

        // Total should be substantially more than the old 4 interaction + 2 search
        assert!(
            group.tests.len() > 20,
            "Expected 20+ tests, got {}",
            group.tests.len()
        );
    }

    #[test]
    fn search_modifier_urls_are_correct() {
        let cs = sample_capability_statement();
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];

        // Find the :exact modifier test for "name"
        let exact_test = group.tests.iter().find(|t| {
            matches!(t.kind, TestCaseKind::SearchModifier { ref modifier, .. } if modifier == &SearchModifier::Exact)
                && matches!(t.kind, TestCaseKind::SearchModifier { ref param_name, .. } if param_name == "name")
        });
        assert!(
            exact_test.is_some(),
            "Should have :exact modifier test for name"
        );
        let exact_test = exact_test.unwrap();
        assert!(
            exact_test.request.url.contains("name:exact="),
            "URL should contain name:exact=, got {}",
            exact_test.request.url
        );

        // Find the :missing modifier test for birthdate
        let missing_test = group.tests.iter().find(|t| {
            matches!(t.kind, TestCaseKind::SearchModifier { ref modifier, .. } if modifier == &SearchModifier::Missing)
                && matches!(t.kind, TestCaseKind::SearchModifier { ref param_name, .. } if param_name == "birthdate")
        });
        assert!(
            missing_test.is_some(),
            "Should have :missing modifier test for birthdate"
        );
        let missing_test = missing_test.unwrap();
        assert!(
            missing_test.request.url.contains("birthdate:missing="),
            "URL should contain birthdate:missing=, got {}",
            missing_test.request.url
        );
    }

    #[test]
    fn search_prefix_urls_are_correct() {
        let cs = sample_capability_statement();
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];

        // Find the gt prefix test for birthdate
        let gt_test = group.tests.iter().find(|t| {
            matches!(t.kind, TestCaseKind::SearchPrefix { ref prefix, .. } if prefix == &SearchPrefix::Gt)
                && matches!(t.kind, TestCaseKind::SearchPrefix { ref param_name, .. } if param_name == "birthdate")
        });
        assert!(
            gt_test.is_some(),
            "Should have gt prefix test for birthdate"
        );
        let gt_test = gt_test.unwrap();
        assert!(
            gt_test.request.url.contains("birthdate=gt"),
            "URL should contain birthdate=gt, got {}",
            gt_test.request.url
        );
    }

    #[test]
    fn near_search_generates_coordinate_tests() {
        use crate::generate::model::TestCaseKind;
        use crate::model::*;

        // Build a CapabilityStatement with a Location resource that has a "near" search param
        let cs = CapabilityStatement {
            resource_type: "CapabilityStatement".to_string(),
            url: Some("http://example.org/CapabilityStatement/test".to_string()),
            name: Some("TestCS".to_string()),
            status: Some("active".to_string()),
            rest: vec![Rest {
                mode: "server".to_string(),
                resource: vec![RestResource {
                    resource_type: "Location".to_string(),
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Location".to_string()),
                    supported_profile: vec![],
                    interaction: vec![
                        RestInteraction {
                            code: "search-type".to_string(),
                        },
                        RestInteraction {
                            code: "read".to_string(),
                        },
                    ],
                    search_param: vec![RestSearchParam {
                        name: "near".to_string(),
                        param_type: "special".to_string(),
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
        };

        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];

        let near_test = group
            .tests
            .iter()
            .find(|t| matches!(t.kind, TestCaseKind::SearchNear { .. }));
        assert!(near_test.is_some(), "Should have a near search test");
        let near_test = near_test.unwrap();
        assert!(
            near_test.request.url.contains("near="),
            "URL should contain near=, got {}",
            near_test.request.url
        );
        assert!(
            near_test.request.url.contains("%7C"),
            "URL should contain %7C (pipe encoding), got {}",
            near_test.request.url
        );
    }

    // ─── Builder unit tests ────────────────────────────────────────────────────

    #[test]
    fn build_interaction_test_read() {
        let test = build_interaction_test("Patient", &Interaction::Read, &None);
        assert_eq!(test.request.method, "GET");
        assert_eq!(test.request.url, "/Patient/{id}");
        assert_eq!(test.name, "patient_read");
        assert_eq!(test.kind, TestCaseKind::Interaction);
        assert_eq!(test.validation.expected_status, 200);
        assert_eq!(test.validation.required_elements, vec!["Patient.id"]);
        assert!(test.validation.profile_url.is_none());
    }

    #[test]
    fn build_interaction_test_create() {
        let profile = Some("http://example.org/Profile".to_string());
        let test = build_interaction_test("Patient", &Interaction::Create, &profile);
        assert_eq!(test.request.method, "POST");
        assert_eq!(test.request.url, "/Patient");
        assert_eq!(test.name, "patient_create");
        assert_eq!(test.kind, TestCaseKind::Interaction);
        assert_eq!(test.validation.expected_status, 201);
        assert_eq!(test.validation.required_elements, vec!["Patient.id"]);
        assert_eq!(
            test.validation.profile_url,
            Some("http://example.org/Profile".to_string())
        );
    }

    #[test]
    fn build_interaction_test_delete() {
        let test = build_interaction_test("Observation", &Interaction::Delete, &None);
        assert_eq!(test.request.method, "DELETE");
        assert_eq!(test.request.url, "/Observation/{id}");
        assert_eq!(test.name, "observation_delete");
        assert_eq!(test.validation.expected_status, 204);
        assert!(test.validation.required_elements.is_empty());
    }

    #[test]
    fn build_interaction_test_search_type() {
        let test = build_interaction_test("Patient", &Interaction::SearchType, &None);
        assert_eq!(test.request.method, "GET");
        assert_eq!(test.request.url, "/Patient?_count=1");
        assert_eq!(test.name, "patient_searchtype");
        assert_eq!(test.validation.expected_status, 200);
    }

    #[test]
    fn build_interaction_test_operation() {
        let test = build_interaction_test(
            "Patient",
            &Interaction::Operation("everything".to_string()),
            &None,
        );
        assert_eq!(test.request.method, "POST");
        assert_eq!(test.request.url, "/Patient/$everything");
        assert_eq!(test.name, "patient_operation-everything");
        assert_eq!(test.validation.expected_status, 200);
    }

    #[test]
    fn build_search_single_test_produces_correct_url_and_kind() {
        let test = build_search_single_test(
            "Patient",
            "name",
            "string",
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("?name="),
            "URL should contain ?name=, got {}",
            test.request.url
        );
        assert!(
            test.request.url.contains("&_id={id}"),
            "URL should contain &_id={{id}}, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_name");
        assert!(
            matches!(test.kind, TestCaseKind::SearchSingle { ref param_name, .. } if param_name == "name")
        );
        assert_eq!(test.interaction, Interaction::SearchType);
        assert_eq!(test.validation.expected_status, 200);
        assert_eq!(
            test.validation
                .response_assertion
                .as_ref()
                .unwrap()
                .bundle_type,
            Some("searchset".to_string())
        );
    }

    #[test]
    fn build_search_single_test_with_profile() {
        let profile = Some("http://example.org/Profile".to_string());
        let test = build_search_single_test(
            "Patient",
            "birthdate",
            "date",
            &profile,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.profile_url, profile);
        assert!(test.request.url.contains("?birthdate="));
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchSingle { ref param_name, ref param_type }
                if param_name == "birthdate" && param_type == "date"
        ));
    }

    #[test]
    fn build_search_modifier_test_exact() {
        let test = build_search_modifier_test(
            "Patient",
            "name",
            "string",
            &SearchModifier::Exact,
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("name:exact="),
            "URL should contain name:exact=, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_name_exact");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchModifier { ref param_name, ref modifier }
                if param_name == "name" && modifier == &SearchModifier::Exact
        ));
    }

    #[test]
    fn build_search_modifier_test_missing() {
        let test = build_search_modifier_test(
            "Patient",
            "birthdate",
            "date",
            &SearchModifier::Missing,
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert_eq!(test.request.url, "/Patient?birthdate:missing=true");
        assert_eq!(test.name, "patient_search_birthdate_missing");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchModifier { ref modifier, .. }
                if modifier == &SearchModifier::Missing
        ));
    }

    #[test]
    fn build_search_prefix_test_eq() {
        let test = build_search_prefix_test(
            "Patient",
            "birthdate",
            "date",
            &SearchPrefix::Eq,
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("birthdate=eq"),
            "URL should contain birthdate=eq, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_birthdate_eq");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchPrefix { ref param_name, ref prefix }
                if param_name == "birthdate" && prefix == &SearchPrefix::Eq
        ));
    }

    #[test]
    fn build_search_prefix_test_gt() {
        let test = build_search_prefix_test(
            "Observation",
            "value",
            "quantity",
            &SearchPrefix::Gt,
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            test.request.url.contains("value=gt"),
            "URL should contain value=gt, got {}",
            test.request.url
        );
        assert_eq!(test.name, "observation_search_value_gt");
    }

    #[test]
    fn build_search_near_test_produces_coordinate_url() {
        let test = build_search_near_test("Location", "near", &None);
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("near="),
            "URL should contain near=, got {}",
            test.request.url
        );
        assert!(
            test.request.url.contains("%7C"),
            "URL should contain %7C (pipe encoding), got {}",
            test.request.url
        );
        assert_eq!(test.name, "location_search_near_near_10km");
        assert!(
            matches!(test.kind, TestCaseKind::SearchNear { ref param_name } if param_name == "near")
        );
        assert_eq!(test.validation.expected_status, 200);
    }

    #[test]
    fn build_search_combo_test_combines_params() {
        let params: [(&str, &str); 2] = [("name", "string"), ("birthdate", "date")];
        let test =
            build_search_combo_test("Patient", &params, &None, &HashMap::new(), &HashMap::new());
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("?name="),
            "URL should contain ?name=, got {}",
            test.request.url
        );
        assert!(
            test.request.url.contains("&birthdate="),
            "URL should contain &birthdate=, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_combo_name_and_birthdate");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchCombo { ref params } if params == &vec!["name".to_string(), "birthdate".to_string()]
        ));
    }

    #[test]
    fn build_chained_search_test_produces_chain_url() {
        let test = build_chained_search_test(
            "Patient",
            "organization",
            "name",
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("?organization.name="),
            "URL should contain ?organization.name=, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_chain_organization_name");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchChained { ref chain_param, ref target_param }
                if chain_param == "organization" && target_param == "name"
        ));
    }

    #[test]
    fn build_include_test_forward_include() {
        let test = build_include_test("Patient", "organization", false, None, None, &None);
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("?_include=Patient:organization"),
            "URL should contain ?_include=Patient:organization, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_include_organization");
        assert!(matches!(
            test.kind,
            TestCaseKind::Include { ref param, revinclude: false }
                if param == "organization"
        ));
        assert_eq!(
            test.validation
                .response_assertion
                .as_ref()
                .unwrap()
                .include_requires_distinct_from,
            Some("Patient".to_string())
        );
    }

    #[test]
    fn build_include_test_revinclude() {
        let test = build_include_test("Observation", "subject", true, Some("Patient"), None, &None);
        assert!(
            test.request.url.contains("?_revinclude=Patient:subject"),
            "URL should contain ?_revinclude=Patient:subject, got {}",
            test.request.url
        );
        assert_eq!(test.name, "observation_revinclude_subject");
        assert!(matches!(
            test.kind,
            TestCaseKind::Include { ref param, revinclude: true }
                if param == "subject"
        ));
    }

    #[test]
    fn build_include_test_with_expected_type() {
        let test = build_include_test(
            "Patient",
            "organization",
            false,
            None,
            Some("Organization".to_string()),
            &None,
        );
        let assertion = test.validation.response_assertion.unwrap();
        assert_eq!(
            assertion.include_types.get("Organization"),
            Some(&"organization".to_string())
        );
        assert!(assertion.include_requires_distinct_from.is_none());
    }

    #[test]
    fn build_result_param_test_count() {
        let test = build_result_param_test("Patient", "_count", "1", &None, &[], &HashMap::new());
        // With no created_ids, only the empty-id variant is produced
        assert_eq!(test.len(), 1);
        let tc = &test[0];
        assert_eq!(tc.request.method, "GET");
        assert!(
            tc.request.url.contains("?_count=1"),
            "URL: {}",
            tc.request.url
        );
        assert!(tc.request.url.contains("&_id=nonexistent-id-99999"));
        assert_eq!(tc.name, "patient_result_count_empty");
        assert!(matches!(tc.kind, TestCaseKind::ResultParam { ref param } if param == "_count"));
    }

    #[test]
    fn build_result_param_test_with_created_ids() {
        let mut created_ids = HashMap::new();
        created_ids.insert("Patient".to_string(), "pat-001".to_string());
        let tests = build_result_param_test("Patient", "_count", "1", &None, &[], &created_ids);
        assert_eq!(
            tests.len(),
            2,
            "Should produce both real-ID and empty-ID variants"
        );

        let real_test = tests.iter().find(|t| !t.name.contains("empty")).unwrap();
        assert!(
            real_test.request.url.contains("&_id={id}"),
            "Real URL should have {{id}} placeholder: {}",
            real_test.request.url
        );
        assert_eq!(real_test.name, "patient_result_count");
        assert_eq!(
            real_test
                .validation
                .response_assertion
                .as_ref()
                .unwrap()
                .max_entries,
            Some(1)
        );
    }

    #[test]
    fn build_result_param_test_sort() {
        let mut created_ids = HashMap::new();
        created_ids.insert("Patient".to_string(), "pat-001".to_string());
        let declared_params = vec![RestSearchParam {
            name: "birthdate".to_string(),
            param_type: "date".to_string(),
            definition: None,
            documentation: None,
        }];
        let tests = build_result_param_test(
            "Patient",
            "_sort",
            "_lastUpdated",
            &None,
            &declared_params,
            &created_ids,
        );
        assert!(!tests.is_empty());
        let sort_test = &tests[0];
        assert!(
            sort_test.request.url.contains("?_sort="),
            "URL: {}",
            sort_test.request.url
        );
        assert!(
            matches!(sort_test.kind, TestCaseKind::ResultParam { ref param } if param == "_sort")
        );
        let assertion = sort_test.validation.response_assertion.as_ref().unwrap();
        assert_eq!(assertion.sort_by.as_ref().unwrap().field, "birthdate");
        assert_eq!(assertion.sort_by.as_ref().unwrap().direction, "asc");
    }

    #[test]
    fn build_operation_test_instance_scope() {
        let op_def = OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/OperationDefinition/Patient-everything".to_string(),
            name: "everything".to_string(),
            code: "everything".to_string(),
            system: Some(false),
            type_: Some(false),
            instance: Some(true),
            parameter: vec![],
        };
        let test = build_operation_test(
            "Patient",
            "everything",
            Some(&op_def),
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert_eq!(test.request.url, "/Patient/{id}/$everything");
        assert_eq!(test.name, "patient_operation_everything");
        assert!(matches!(test.kind, TestCaseKind::Operation { ref code } if code == "everything"));
        assert_eq!(test.validation.expected_status, 200);
        let assertion = test.validation.response_assertion.as_ref().unwrap();
        assert_eq!(
            assertion.response_contains_key,
            Some("resourceType".to_string())
        );
        // No required input params → GET with no body
        assert!(test.request.body.is_none());
    }

    #[test]
    fn build_operation_test_system_scope() {
        let op_def = OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/uv/bulkdata/OperationDefinition/export".to_string(),
            name: "export".to_string(),
            code: "export".to_string(),
            system: Some(true),
            type_: Some(false),
            instance: Some(false),
            parameter: vec![],
        };
        let test = build_operation_test(
            "System",
            "export",
            Some(&op_def),
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.url, "/$export");
        assert_eq!(test.request.method, "GET"); // No required params → GET
        assert_eq!(test.name, "system_operation_export");
    }

    #[test]
    fn build_operation_test_with_body_params() {
        let op_def = OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/OperationDefinition/Patient-everything".to_string(),
            name: "everything".to_string(),
            code: "everything".to_string(),
            system: Some(false),
            type_: Some(false),
            instance: Some(true),
            parameter: vec![OperationParameter {
                name: "start".to_string(),
                use_: Some("in".to_string()),
                min: Some(1),
                max: Some("1".to_string()),
                param_type: Some("date".to_string()),
            }],
        };
        let test = build_operation_test(
            "Patient",
            "everything",
            Some(&op_def),
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            test.request.body.is_some(),
            "Should have a request body for operations with required params"
        );
        assert_eq!(test.request.method, "POST"); // Has required params → POST
        let body = test.request.body.unwrap();
        assert_eq!(body["resourceType"], "Parameters");
        assert!(body["parameter"].is_array());
        assert_eq!(body["parameter"][0]["name"], "start");
    }

    #[test]
    fn build_negative_test_produces_correct_structure() {
        let test = build_negative_test("Patient", "bad-request", "POST", "/Patient", 400, &None);
        assert_eq!(test.request.method, "POST");
        assert_eq!(test.request.url, "/Patient");
        assert_eq!(test.name, "patient_negative_bad_request");
        assert!(
            matches!(test.kind, TestCaseKind::Negative { ref description } if description == "bad-request")
        );
        assert_eq!(test.validation.expected_status, 400);
        assert_eq!(test.interaction, Interaction::Read);
    }

    #[test]
    fn assertion_for_kind_search_single() {
        let kind = TestCaseKind::SearchSingle {
            param_name: "name".to_string(),
            param_type: "string".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
        assert_eq!(assertion.min_entries, Some(0));
    }

    #[test]
    fn assertion_for_kind_interaction() {
        let kind = TestCaseKind::Interaction;
        let assertion = assertion_for_kind(&kind, "Patient");
        assert!(
            assertion.is_none(),
            "Interaction should have no response assertion"
        );
    }

    #[test]
    fn assertion_for_kind_include() {
        let kind = TestCaseKind::Include {
            param: "organization".to_string(),
            revinclude: false,
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
        assert_eq!(assertion.min_entries, Some(0));
    }

    #[test]
    fn assertion_for_kind_negative() {
        let kind = TestCaseKind::Negative {
            description: "bad-request".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient");
        assert!(
            assertion.is_none(),
            "Negative should have no response assertion"
        );
    }

    #[test]
    fn sample_value_returns_correct_defaults() {
        assert_eq!(sample_value("string"), "test-value");
        assert_eq!(sample_value("token"), "test-code");
        assert_eq!(sample_value("reference"), "Patient/test-id");
        assert_eq!(sample_value("number"), "1");
        assert_eq!(sample_value("date"), "2024-01-01");
        assert_eq!(sample_value("dateTime"), "2024-01-01T00:00:00Z");
        assert_eq!(sample_value("uri"), "http://example.org");
        assert_eq!(
            sample_value("quantity"),
            "5.0||http://unitsofmeasure.org|kg"
        );
        assert!(
            sample_value("special").contains("%7C"),
            "special should contain pipe encoding"
        );
        assert_eq!(sample_value("unknown"), "test-value");
    }

    #[test]
    fn resolve_param_value_falls_back_to_sample_value() {
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let value = resolve_param_value("Patient", "name", "string", &empty_fv, &empty_ids);
        assert_eq!(value, "test-value");
    }

    #[test]
    fn resolve_param_value_uses_field_values() {
        let mut field_values = HashMap::new();
        let mut patient_values = HashMap::new();
        patient_values.insert("Patient.name".to_string(), "John".to_string());
        field_values.insert("Patient".to_string(), patient_values);
        let empty_ids = HashMap::new();
        let value = resolve_param_value("Patient", "name", "string", &field_values, &empty_ids);
        assert_eq!(value, "John");
    }
}
