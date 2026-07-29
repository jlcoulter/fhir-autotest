use crate::generate::model::*;
use crate::model::*;
use std::collections::HashMap;

/// Build a ResponseAssertion appropriate for the test case kind.
pub fn assertion_for_kind(kind: &TestCaseKind, _resource_type: &str) -> Option<ResponseAssertion> {
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
        TestCaseKind::ResultParam { param } => match param.as_str() {
            "_summary" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(0),
                absent_fields: vec!["text".to_string()],
                ..ResponseAssertion::none()
            }),
            "_count" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                max_entries: Some(1), // we request _count=1
                ..ResponseAssertion::none()
            }),
            "_sort" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                sort_by: Some(SortAssertion {
                    field: "_lastUpdated".to_string(),
                    direction: "asc".to_string(),
                }),
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
pub fn generate_test_plan(
    cs: &CapabilityStatement,
    _profiles: &[StructureDefinition],
    search_params: &[SearchParameter],
    operations: Option<&[OperationDefinition]>,
    ig_url: Option<&str>,
) -> TestPlan {
    let mut test_groups = Vec::new();

    for rest in &cs.rest {
        if rest.mode != "server" {
            continue;
        }

        for resource in &rest.resource {
            let profile_url = resource
                .supported_profile
                .first()
                .cloned()
                .or_else(|| resource.profile.clone());

            let group = build_test_group(resource, &profile_url, search_params, operations);
            test_groups.push(group);
        }

        // System-level operations (e.g., $export at the rest level)
        for op in &rest.operation {
            let op_def = operations.and_then(|ops| ops.iter().find(|o| o.code == op.name));

            let mut test = build_operation_test(
                "", // system-level, no resource type prefix
                &op.name, op_def, &None,
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
                    ));
                }
            }
        }

        // --- Chained search tests (reference params → target param) ---
        for sp in &inline_params {
            if sp.param_type == "reference" {
                // Try to find target resource search params to chain into
                if let Some(target_type) = infer_reference_target(&sp.name) {
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
                let expected_include_type = infer_reference_target(&param.to_lowercase());
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
        if let Some(test) = build_result_param_test(
            &resource.resource_type,
            "_summary",
            "true",
            profile_url,
            &inline_params,
        ) {
            tests.push(test);
        }
        if let Some(test) = build_result_param_test(
            &resource.resource_type,
            "_count",
            "1",
            profile_url,
            &inline_params,
        ) {
            tests.push(test);
        }
        if let Some(sort_test) = build_result_param_test(
            &resource.resource_type,
            "_sort",
            "_lastUpdated",
            profile_url,
            &inline_params,
        ) {
            tests.push(sort_test);
        }
    }

    // --- $operation tests from CS rest.operation ---
    for op in &resource.operation {
        let op_def = operations.and_then(|ops| ops.iter().find(|o| o.code == op.name));

        tests.push(build_operation_test(
            &resource.resource_type,
            &op.name,
            op_def,
            profile_url,
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

/// Sample value for a search param based on its type.
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
) -> TestCase {
    let value = sample_value(param_type);
    let url = format!("/{resource_type}?{param_name}={value}&_id={{id}}");

    let search_value_assertions = vec![SearchValueAssertion {
        resource_type: resource_type.to_string(),
        query_param: param_name.to_string(),
        field_paths: search_param_assertion_paths(resource_type, param_name, param_type),
        expected_value: None,
    }];

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
                search_value_assertions,
                ..ResponseAssertion::none()
            }),
        },
    }
}

fn build_search_single_from_sp(
    resource_type: &str,
    sp: &SearchParameter,
    profile_url: &Option<String>,
) -> TestCase {
    build_search_single_test(resource_type, &sp.code, &sp.param_type, profile_url)
}

fn build_search_modifier_test(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    modifier: &SearchModifier,
    profile_url: &Option<String>,
) -> TestCase {
    let value = sample_value(param_type);
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
) -> TestCase {
    let value = sample_value(param_type);
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
) -> TestCase {
    let query: Vec<String> = params
        .iter()
        .map(|(name, ptype)| format!("{}={}", name, sample_value(ptype)))
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
) -> TestCase {
    let url = format!("/{resource_type}?{chain_param}.{target_param}=test-value");

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
    let include_requires_distinct_from = if include_types.is_empty() {
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
) -> Option<TestCase> {
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

                match fallback {
                    Some(fb) => ("_sort", fb.clone(), Some(fb)),
                    None => return None, // No suitable param, skip sort test
                }
            }
        } else {
            (param, value.to_string(), None)
        };

    let url = format!("/{resource_type}?{actual_param}={actual_value}&_id=nonexistent-id-99999");

    let name = if param == "_sort" {
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

    // Build response assertion for _sort with the actual sort field
    let response_assertion = if param == "_sort" {
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

    Some(TestCase {
        name,
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
    })
}

fn build_operation_test(
    resource_type: &str,
    code: &str,
    op_def: Option<&OperationDefinition>,
    profile_url: &Option<String>,
) -> TestCase {
    // Build request body from operation parameters
    let body = op_def.map(|def| {
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
                    param_obj.insert(
                        "value".to_string(),
                        serde_json::Value::String(sample_value(ptype).to_string()),
                    );
                }
                param_array.push(serde_json::Value::Object(param_obj));
            }
        }
        params.insert(
            "parameter".to_string(),
            serde_json::Value::Array(param_array),
        );
        serde_json::Value::Object(params)
    });

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
            method: "POST".to_string(),
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

/// Infer a reference target resource type from common FHIR search param names.
fn infer_reference_target(param_name: &str) -> Option<String> {
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

fn search_param_assertion_paths(
    _resource_type: &str,
    param_name: &str,
    param_type: &str,
) -> Vec<String> {
    match param_name {
        "name" => vec!["name.family".to_string(), "name.given".to_string()],
        "identifier" => vec!["identifier.value".to_string()],
        "active" => vec!["active".to_string()],
        "status" => vec!["status".to_string()],
        "birthdate" => vec!["birthDate".to_string()],
        "gender" => vec!["gender".to_string()],
        "target" => vec!["target.reference".to_string()],
        "organization" => vec!["organization.reference".to_string()],
        "location" => vec!["location.reference".to_string()],
        "endpoint" => vec!["endpoint.reference".to_string()],
        "_id" => vec!["id".to_string()],
        _ => match param_type {
            "reference" => vec![format!("{}.reference", param_name)],
            _ => vec![param_name.to_string()],
        },
    }
    .into_iter()
    .collect()
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
        let plan = generate_test_plan(&cs, &[], &sample_search_params(), Some(&ops), None);

        assert_eq!(plan.test_groups.len(), 2); // Patient group + system $export group
        let patient_group = plan
            .test_groups
            .iter()
            .find(|g| g.resource_type == "Patient")
            .expect("Should have Patient group");
        assert_eq!(patient_group.resource_type, "Patient");

        let group = patient_group;

        // Should have interaction tests
        assert!(group
            .tests
            .iter()
            .any(|t| matches!(t.kind, TestCaseKind::Interaction)));

        // Should have single search param tests
        assert!(group
            .tests
            .iter()
            .any(|t| matches!(t.kind, TestCaseKind::SearchSingle { .. })));

        // Should have modifier tests (string params get :exact and :contains)
        assert!(group
            .tests
            .iter()
            .any(|t| matches!(t.kind, TestCaseKind::SearchModifier { .. })));

        // Should have prefix tests (date params get eq, ne, gt, etc.)
        assert!(group
            .tests
            .iter()
            .any(|t| matches!(t.kind, TestCaseKind::SearchPrefix { .. })));

        // Should have combo tests (name + birthdate)
        assert!(group
            .tests
            .iter()
            .any(|t| matches!(t.kind, TestCaseKind::SearchCombo { .. })));

        // Should have operation tests ($everything)
        assert!(group
            .tests
            .iter()
            .any(|t| matches!(t.kind, TestCaseKind::Operation { .. })));

        // Should have negative tests
        assert!(group
            .tests
            .iter()
            .any(|t| matches!(t.kind, TestCaseKind::Negative { .. })));

        // Should have result param tests
        assert!(group
            .tests
            .iter()
            .any(|t| matches!(t.kind, TestCaseKind::ResultParam { .. })));
    }

    #[test]
    fn test_case_count_is_comprehensive() {
        let cs = sample_capability_statement();
        let ops = sample_operations();
        let plan = generate_test_plan(&cs, &[], &sample_search_params(), Some(&ops), None);

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
        let plan = generate_test_plan(&cs, &[], &[], None, None);
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
        let plan = generate_test_plan(&cs, &[], &[], None, None);
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
                        name: "near".to_string(),
                        definition: None,
                        param_type: "special".to_string(),
                        documentation: None,
                    }],
                    search_include: vec![],
                    search_revinclude: vec![],
                    operation: vec![],
                    read_history: None,
                    update_create: None,
                    conditional_create: None,
                    conditional_read: None,
                    conditional_update: None,
                    conditional_delete: None,
                }],
                interaction: vec![],
                operation: vec![],
            }],
        };

        let plan = generate_test_plan(&cs, &[], &[], None, None);
        let group = plan
            .test_groups
            .iter()
            .find(|g| g.resource_type == "Location")
            .expect("Should have Location group");

        // Should have a SearchSingle test for "near"
        assert!(group.tests.iter().any(|t| matches!(t.kind, TestCaseKind::SearchSingle { ref param_name, .. } if param_name == "near")),
            "Should have SearchSingle test for near param");

        // Should have a SearchNear test with coordinate format
        let near_test = group
            .tests
            .iter()
            .find(|t| matches!(t.kind, TestCaseKind::SearchNear { .. }));
        assert!(
            near_test.is_some(),
            "Should have SearchNear test for special-type param"
        );
        let near_test = near_test.unwrap();
        assert!(
            near_test
                .request
                .url
                .contains("near=-25.0%7C133.0%7C3000%7Ckm"),
            "Near test URL should contain encoded near coordinate format, got {}",
            near_test.request.url
        );
        assert!(
            near_test.request.url.starts_with("/Location?"),
            "Near test URL should start with /Location?, got {}",
            near_test.request.url
        );
    }
}

#[cfg(test)]
mod debug_tests {
    use crate::generate::planner::generate_test_plan;
    use crate::parse::parse_package;

    #[test]
    #[ignore = "requires local package/package.tgz — run with `cargo test -- --ignored`"]
    fn debug_real_package_cs_parsing() {
        let pkg = parse_package("package/package.tgz").unwrap();
        for cs in &pkg.capability_statements {
            eprintln!(
                "CS: {:?} | rest: {}",
                cs.name.as_deref().unwrap_or("unknown"),
                cs.rest.len()
            );
            for rest in &cs.rest {
                eprintln!(
                    "  rest mode: {} | resources: {}",
                    rest.mode,
                    rest.resource.len()
                );
                for res in &rest.resource {
                    eprintln!(
                        "    resource: {} | interactions: {} | searchParams: {} | operations: {}",
                        res.resource_type,
                        res.interaction.len(),
                        res.search_param.len(),
                        res.operation.len()
                    );
                    for sp in &res.search_param {
                        eprintln!("      searchParam: {} type={}", sp.name, sp.param_type);
                    }
                    for op in &res.operation {
                        eprintln!("      operation: {} def={:?}", op.name, op.definition);
                    }
                }
            }
        }
        // Generate test plan from the responder CS (should have resources)
        let responder_cs = pkg
            .capability_statements
            .iter()
            .find(|cs| cs.name.as_deref() == Some("HealthConnectProviderDirectoryResponder"))
            .expect("Should find responder CS");
        let plan = generate_test_plan(
            responder_cs,
            &pkg.structure_definitions,
            &pkg.search_parameters,
            Some(&pkg.operation_definitions),
            None,
        );
        eprintln!(
            "Test plan: {} groups, {} total tests",
            plan.test_groups.len(),
            plan.test_groups
                .iter()
                .map(|g| g.tests.len())
                .sum::<usize>()
        );
        for group in &plan.test_groups {
            eprintln!(
                "  Group: {} | tests: {}",
                group.resource_type,
                group.tests.len()
            );
        }
        assert!(
            !plan.test_groups.is_empty(),
            "Should have test groups from real CS"
        );
    }
}
