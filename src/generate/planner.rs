use crate::generate::model::*;
use crate::generate::test_builders::*;
use crate::generate::value_resolver::resolve_reference_target;
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

        // Chained search with modifier: expect Bundle searchset
        TestCaseKind::SearchChainedModifier { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0),
            ..ResponseAssertion::none()
        }),

        // Multi-hop chained search: expect Bundle searchset
        TestCaseKind::SearchChainedMultiHop { .. } => Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(0),
            ..ResponseAssertion::none()
        }),

        // Composite search: expect Bundle searchset
        TestCaseKind::SearchComposite { .. } => Some(ResponseAssertion {
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
                    absent_fields: summary_absent_fields(),
                    required_fields: required,
                    ..ResponseAssertion::none()
                })
            }
            "_count" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                max_entries: Some(1), // we request _count=1
                ..ResponseAssertion::none()
            }),
            "_total" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(0),
                ..ResponseAssertion::none()
            }),
            "_filter" | "_source" | "_language" | "_contained" | "_containedType"
            | "_getpagesoffset" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(0),
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

            let mut sys_tests = vec![test];

            // System-level operation conformance tests
            if let Some(def) = op_def {
                if def.affects_state == Some(false) {
                    sys_tests.push(build_operation_affects_state_test(
                        "",
                        &op.name,
                        op_def,
                        &None,
                        field_values,
                        created_ids,
                    ));
                }
                if def.idempotent == Some(true) {
                    sys_tests.push(build_operation_idempotent_test(
                        "",
                        &op.name,
                        op_def,
                        &None,
                        field_values,
                        created_ids,
                    ));
                }
                let has_required_input = def
                    .parameter
                    .iter()
                    .any(|p| p.use_.as_deref() == Some("in") && p.min.unwrap_or(0) > 0);
                if has_required_input {
                    sys_tests.push(build_operation_error_test("", &op.name, op_def, &None));
                }
                sys_tests.extend(build_operation_scope_test("", &op.name, op_def, &None));
            }

            test_groups.push(TestGroup {
                resource_type: format!("$${}", op.name),
                profile_url: None,
                tests: sys_tests,
            });
        }

        // System-level interaction tests (batch, transaction, history-system, search-system)
        let system_interaction_codes: Vec<String> =
            rest.interaction.iter().map(|i| i.code.clone()).collect();
        if !system_interaction_codes.is_empty() {
            let mut sys_interaction_tests = Vec::new();
            for code in &system_interaction_codes {
                sys_interaction_tests.push(build_system_interaction_test(code));
            }
            // Also add a system-level search test if search-system is declared
            if system_interaction_codes.contains(&"search-system".to_string()) {
                sys_interaction_tests.push(build_system_search_test());
            }
            test_groups.push(TestGroup {
                resource_type: "$$system_interactions".to_string(),
                profile_url: None,
                tests: sys_interaction_tests,
            });
        }
    }

    let name = cs.name.clone().unwrap_or_else(|| "Unnamed IG".to_string());

    TestPlan {
        name,
        ig_url: ig_url.map(|s| s.to_string()),
        test_groups,
        creation_order: Vec::new(),
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

    // --- Conditional operation tests ---
    // If the server declares conditionalCreate, generate a test with If-None-Exist header.
    if resource.conditional_create == Some(true) {
        tests.push(build_conditional_create_test(
            &resource.resource_type,
            profile_url,
        ));
    }
    // If the server declares conditionalUpdate, generate a test with If-Match header.
    if resource.conditional_update == Some(true) {
        tests.push(build_conditional_update_test(
            &resource.resource_type,
            profile_url,
        ));
    }
    // If the server declares conditionalRead (not "not-supported"), generate tests
    // with If-Modified-Since and If-None-Match headers.
    if let Some(ref cr) = resource.conditional_read
        && cr != "not-supported"
    {
        tests.extend(build_conditional_read_test(
            &resource.resource_type,
            profile_url,
        ));
    }
    // If the server declares conditionalDelete (single or multiple), generate a
    // conditional delete test.
    if let Some(ref cd) = resource.conditional_delete
        && cd != "not-supported"
    {
        tests.push(build_conditional_delete_test(
            &resource.resource_type,
            profile_url,
        ));
    }
    // If the server declares updateCreate, generate an update-as-create test.
    if resource.update_create == Some(true) {
        tests.push(build_update_create_test(
            &resource.resource_type,
            profile_url,
        ));
    }

    // --- History parameter tests ---
    // Generate history param tests when history-instance or history-type is declared.
    let has_history_instance = resource
        .interaction
        .iter()
        .any(|i| i.code == "history-instance");
    let has_history_type = resource
        .interaction
        .iter()
        .any(|i| i.code == "history-type");
    if has_history_instance || has_history_type {
        tests.extend(build_history_param_test(
            &resource.resource_type,
            profile_url,
        ));
    }

    // --- Read parameter tests ---
    // Generate read param tests (_elements, _summary) when read is declared.
    if has_read {
        tests.extend(build_read_param_test(&resource.resource_type, profile_url));
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

        // --- Composite search param tests ---
        // For each SearchParameter with type=composite, generate a test
        // with a composite value (two values joined by $).
        for sp in &resource_search_params {
            if sp.param_type == "composite" {
                tests.push(build_search_composite_test(
                    &resource.resource_type,
                    &sp.code,
                    profile_url,
                ));
            }
        }

        // --- Declared comparator/modifier validation ---
        // When a SearchParameter declares specific comparators or modifiers,
        // generate tests only for those. If not declared, test all applicable
        // (current behavior).
        for sp in &resource_search_params {
            // Declared comparator tests
            if !sp.comparator.is_empty() {
                for comp_str in &sp.comparator {
                    if let Some(prefix) = SearchPrefix::parse_prefix(comp_str) {
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
                }
            }
            // Declared modifier tests
            if !sp.modifier.is_empty() {
                for mod_str in &sp.modifier {
                    if let Some(modifier) = SearchModifier::parse_modifier(mod_str) {
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
                }
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
                if let Some(target_type) =
                    resolve_reference_target(&resource.resource_type, &sp.name, Some(search_params))
                {
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

                        // Chained search with modifier (:exact, :contains)
                        for modifier in &[SearchModifier::Exact, SearchModifier::Contains] {
                            tests.push(build_chained_search_modifier_test(
                                &resource.resource_type,
                                &sp.name,
                                &target_sp.code,
                                modifier,
                                profile_url,
                                field_values,
                                created_ids,
                            ));
                        }
                    }

                    // Multi-hop chained search: try to find a second hop
                    // through the target resource's reference params
                    let second_hop_params: Vec<&SearchParameter> = search_params
                        .iter()
                        .filter(|tsp| {
                            tsp.base.contains(&target_type) && tsp.param_type == "reference"
                        })
                        .take(1) // limit to 1 to avoid explosion
                        .collect();

                    for second_sp in &second_hop_params {
                        if let Some(second_target) = resolve_reference_target(
                            &target_type,
                            &second_sp.code,
                            Some(search_params),
                        ) {
                            let third_params: Vec<&SearchParameter> = search_params
                                .iter()
                                .filter(|tsp| {
                                    tsp.base.contains(&second_target) && tsp.param_type == "string"
                                })
                                .take(1)
                                .collect();

                            for third_sp in &third_params {
                                tests.push(build_chained_search_multi_hop_test(
                                    &resource.resource_type,
                                    &[sp.name.clone(), second_sp.code.clone()],
                                    &third_sp.code,
                                    profile_url,
                                    field_values,
                                    created_ids,
                                ));
                            }
                        }
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
                let expected_include_type = resolve_reference_target(
                    &resource.resource_type,
                    &param.to_lowercase(),
                    Some(search_params),
                );
                tests.push(build_include_test(
                    &resource.resource_type,
                    &param.to_lowercase(),
                    false,
                    None,
                    expected_include_type.clone(),
                    profile_url,
                ));

                // _include:recurse variant
                tests.push(build_include_recurse_test(
                    &resource.resource_type,
                    &param.to_lowercase(),
                    profile_url,
                ));

                // _include:iterate variant
                tests.push(build_include_iterate_test(
                    &resource.resource_type,
                    &param.to_lowercase(),
                    profile_url,
                ));

                // _include:recurse:iterate combined variant
                tests.push(build_include_recurse_iterate_test(
                    &resource.resource_type,
                    &param.to_lowercase(),
                    profile_url,
                ));

                // _include with :targetType variant (when we can resolve the target)
                if let Some(target_type) = &expected_include_type {
                    tests.push(build_include_target_type_test(
                        &resource.resource_type,
                        &param.to_lowercase(),
                        target_type,
                        profile_url,
                    ));
                }
            }
        }

        // _include=* wildcard test
        if !resource.search_include.is_empty() {
            tests.push(build_include_wildcard_test(
                &resource.resource_type,
                profile_url,
            ));
        }

        // Multiple _include params in one request (when 2+ declared)
        if resource.search_include.len() >= 2 {
            let params: Vec<String> = resource.search_include[..2]
                .iter()
                .filter_map(|s| s.split_once(':').map(|(_, p)| p.to_lowercase()))
                .collect();
            if params.len() >= 2 {
                tests.push(build_include_multiple_test(
                    &resource.resource_type,
                    &params,
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

                // _revinclude:recurse variant
                tests.push(build_revinclude_recurse_test(
                    &resource.resource_type,
                    res,
                    &param.to_lowercase(),
                    profile_url,
                ));

                // _revinclude:iterate variant
                tests.push(build_revinclude_iterate_test(
                    &resource.resource_type,
                    res,
                    &param.to_lowercase(),
                    profile_url,
                ));
            }
        }

        // _revinclude=* wildcard test
        if !resource.search_revinclude.is_empty() {
            tests.push(build_revinclude_wildcard_test(
                &resource.resource_type,
                profile_url,
            ));
        }

        // _include + _revinclude combined test (when both declared)
        if let (Some(first_include), Some(first_revinclude)) = (
            resource.search_include.first(),
            resource.search_revinclude.first(),
        ) && let (Some((_inc_res, inc_param)), Some((rev_res, rev_param))) = (
            first_include.split_once(':'),
            first_revinclude.split_once(':'),
        ) {
            tests.push(build_include_revinclude_combined_test(
                &resource.resource_type,
                &inc_param.to_lowercase(),
                rev_res,
                &rev_param.to_lowercase(),
                profile_url,
            ));
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
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_elements",
            "id,meta,name",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _summary variants ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_summary",
            "count",
            profile_url,
            &inline_params,
            created_ids,
        ));
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_summary",
            "text",
            profile_url,
            &inline_params,
            created_ids,
        ));
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_summary",
            "data",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _total variants ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_total",
            "none",
            profile_url,
            &inline_params,
            created_ids,
        ));
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_total",
            "accurate",
            profile_url,
            &inline_params,
            created_ids,
        ));
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_total",
            "estimate",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _count=0 (count-only search) ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_count",
            "0",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _elements:exclude ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_elements:exclude",
            "text,contained",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- Multi-field _sort ---
        // Use two declared params if available, otherwise skip
        if inline_params.len() >= 2 {
            let sort_value = format!("{},{}", inline_params[0].name, inline_params[1].name);
            tests.extend(build_result_param_test(
                &resource.resource_type,
                "_sort",
                &sort_value,
                profile_url,
                &inline_params,
                created_ids,
            ));
        }

        // --- _filter (advanced filtering DSL) ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_filter",
            "name+eq+Smith",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _source ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_source",
            "urn:source:test",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _language ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_language",
            "en",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _contained / _containedType ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_contained",
            "true",
            profile_url,
            &inline_params,
            created_ids,
        ));
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_containedType",
            "container",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _getpagesoffset ---
        tests.extend(build_result_param_test(
            &resource.resource_type,
            "_getpagesoffset",
            "0",
            profile_url,
            &inline_params,
            created_ids,
        ));

        // --- _format parameter tests ---
        if has_search_type {
            tests.push(build_format_test(
                &resource.resource_type,
                "json",
                profile_url,
            ));
            tests.push(build_format_test(
                &resource.resource_type,
                "application/fhir+json",
                profile_url,
            ));
            tests.push(build_format_test(
                &resource.resource_type,
                "xml",
                profile_url,
            ));
        }

        // --- _pretty parameter tests ---
        if has_search_type {
            tests.push(build_pretty_test(
                &resource.resource_type,
                true,
                profile_url,
            ));
            tests.push(build_pretty_test(
                &resource.resource_type,
                false,
                profile_url,
            ));
        }

        // --- _has (reverse chaining) tests ---
        // For each reference param on this resource, find resources that
        // reference this type and chain into their search params.
        for sp in &inline_params {
            if sp.param_type == "reference"
                && let Some(target_type) =
                    resolve_reference_target(&resource.resource_type, &sp.name, Some(search_params))
            {
                // Find search params on the target resource to chain into
                let target_params: Vec<&SearchParameter> = search_params
                    .iter()
                    .filter(|tsp| tsp.base.contains(&target_type) && tsp.param_type == "string")
                    .take(1) // limit to 1 to avoid explosion
                    .collect();

                for target_sp in target_params {
                    let value = resolve_param_value(
                        &resource.resource_type,
                        &target_sp.code,
                        "string",
                        field_values,
                        created_ids,
                    );
                    let url = format!(
                        "/{}?_has:{}:{}:{}={}",
                        resource.resource_type, target_type, sp.name, target_sp.code, value
                    );
                    tests.push(TestCase {
                        name: format!(
                            "{}_result_has_{}_{}_{}",
                            resource.resource_type.to_lowercase(),
                            target_type.to_lowercase(),
                            sp.name.replace('-', "_"),
                            target_sp.code.replace('-', "_"),
                        ),
                        kind: TestCaseKind::ResultParam {
                            param: "_has".to_string(),
                        },
                        interaction: Interaction::SearchType,
                        resource_type: resource.resource_type.to_string(),
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
                    });
                }
            }
        }

        // --- _list test ---
        // Tests the _list result parameter. Since we don't know if the server
        // has a list with this ID, we accept either 200 (valid Bundle) or 404.
        tests.push(TestCase {
            name: format!("{}_result_list", resource.resource_type.to_lowercase()),
            kind: TestCaseKind::ResultParam {
                param: "_list".to_string(),
            },
            interaction: Interaction::SearchType,
            resource_type: resource.resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/{}?_list=test-list-id-99999", resource.resource_type),
                headers: HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 0, // 0 = accept any status
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion: None,
            },
        });

        // --- _query test ---
        // Tests the _query result parameter with a non-existent query name.
        // Per FHIR spec, unknown named queries may be rejected (404/400) or
        // ignored (200 with Bundle), so we accept any status.
        tests.push(TestCase {
            name: format!("{}_result_query", resource.resource_type.to_lowercase()),
            kind: TestCaseKind::ResultParam {
                param: "_query".to_string(),
            },
            interaction: Interaction::SearchType,
            resource_type: resource.resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/{}?_query=nonexistent-query", resource.resource_type),
                headers: HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 0, // 0 = accept any status
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion: None,
            },
        });
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

        // --- Operation conformance tests ---
        if let Some(def) = op_def {
            // 1. Output parameter validation is already handled inside build_operation_test
            //    via operation_output_params in the ResponseAssertion.

            // 2. affectsState=false: verify the operation is safe to call (no side effects)
            if def.affects_state == Some(false) {
                tests.push(build_operation_affects_state_test(
                    &resource.resource_type,
                    &op.name,
                    op_def,
                    profile_url,
                    field_values,
                    created_ids,
                ));
            }

            // 3. idempotent=true: verify the operation returns the same result when called twice
            if def.idempotent == Some(true) {
                tests.push(build_operation_idempotent_test(
                    &resource.resource_type,
                    &op.name,
                    op_def,
                    profile_url,
                    field_values,
                    created_ids,
                ));
            }

            // 4. Error handling: operations with required input params should reject
            //    requests that omit those params
            let has_required_input = def
                .parameter
                .iter()
                .any(|p| p.use_.as_deref() == Some("in") && p.min.unwrap_or(0) > 0);
            if has_required_input {
                tests.push(build_operation_error_test(
                    &resource.resource_type,
                    &op.name,
                    op_def,
                    profile_url,
                ));
            }

            // 5. Scope validation: operations should reject requests at undeclared scopes
            tests.extend(build_operation_scope_test(
                &resource.resource_type,
                &op.name,
                op_def,
                profile_url,
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_capability_statement() -> CapabilityStatement {
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
                    versioning: None,
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
                security: None,
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
            target: vec![],
            comparator: vec![],
            modifier: vec![],
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
            affects_state: None,
            idempotent: None,
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
        // + 2 read param tests (_elements, _summary) = 5 total
        assert_eq!(interactions, 5);
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
        // 2 negative tests (read_nonexistent, search_invalid_param) + 1 scope test (instance-only → system level)
        assert_eq!(negatives, 3);
        // 4 result params (_summary, _count, _sort, _elements) × 2 variants each (real ID + empty) = 8
        // + 1 _list test + 1 _query test = 10 total
        // + _summary count/text/data variants: 3 × 2 = 6
        // + _total none/accurate/estimate: 3 × 2 = 6
        // + _count=0: 1 × 2 = 2
        // + _elements:exclude: 1 × 2 = 2
        // + multi-field _sort: 1 × 2 = 2
        // + _filter: 1 × 2 = 2
        // + _source: 1 × 2 = 2
        // + _language: 1 × 2 = 2
        // + _contained: 1 × 2 = 2
        // + _containedType: 1 × 2 = 2
        // + _getpagesoffset: 1 × 2 = 2
        // + _format (json, application/fhir+json, xml): 3 × 1 = 3 (no empty-ID variant)
        // + _pretty (true, false): 2 × 1 = 2 (no empty-ID variant)
        // Total result param tests = 10 + 6 + 6 + 2 + 2 + 2 + 2 + 2 + 2 + 2 + 2 + 2 + 3 + 2 = 45
        assert_eq!(result_params, 45);

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
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
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
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
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
            software: None,
            implementation: None,
            messaging: vec![],
            document: vec![],
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
        };

        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
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
    fn assertion_for_kind_include_variant() {
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

    // ── Additional assertion_for_kind tests ────────────────────────────

    #[test]
    fn assertion_for_kind_search_modifier() {
        let kind = TestCaseKind::SearchModifier {
            param_name: "name".to_string(),
            modifier: SearchModifier::Exact,
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
        assert_eq!(assertion.min_entries, Some(0));
    }

    #[test]
    fn assertion_for_kind_search_prefix() {
        let kind = TestCaseKind::SearchPrefix {
            param_name: "birthdate".to_string(),
            prefix: SearchPrefix::Gt,
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_search_near() {
        let kind = TestCaseKind::SearchNear {
            param_name: "near".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Location").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_search_combo() {
        let kind = TestCaseKind::SearchCombo {
            params: vec!["name".to_string(), "birthdate".to_string()],
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_search_chained() {
        let kind = TestCaseKind::SearchChained {
            chain_param: "organization".to_string(),
            target_param: "name".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_search_chained_modifier() {
        let kind = TestCaseKind::SearchChainedModifier {
            chain_param: "organization".to_string(),
            target_param: "name".to_string(),
            modifier: SearchModifier::Exact,
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_search_chained_multi_hop() {
        let kind = TestCaseKind::SearchChainedMultiHop {
            chain_params: vec!["subject".to_string(), "managingOrganization".to_string()],
            target_param: "name".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Observation").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_search_composite() {
        let kind = TestCaseKind::SearchComposite {
            param_name: "custom-composite".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_include() {
        let kind = TestCaseKind::Include {
            param: "organization".to_string(),
            revinclude: false,
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_result_param_summary() {
        let kind = TestCaseKind::ResultParam {
            param: "_summary".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
        assert!(!assertion.absent_fields.is_empty());
        assert!(assertion.required_fields.contains_key("Patient"));
    }

    #[test]
    fn assertion_for_kind_result_param_count() {
        let kind = TestCaseKind::ResultParam {
            param: "_count".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
        assert_eq!(assertion.max_entries, Some(1));
    }

    #[test]
    fn assertion_for_kind_result_param_total() {
        let kind = TestCaseKind::ResultParam {
            param: "_total".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_result_param_filter() {
        let kind = TestCaseKind::ResultParam {
            param: "_filter".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_result_param_unknown() {
        let kind = TestCaseKind::ResultParam {
            param: "_unknown".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(assertion.bundle_type, Some("searchset".to_string()));
    }

    #[test]
    fn assertion_for_kind_operation() {
        let kind = TestCaseKind::Operation {
            code: "everything".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient").unwrap();
        assert_eq!(
            assertion.response_contains_key,
            Some("resourceType".to_string())
        );
    }

    #[test]
    fn assertion_for_kind_conformance() {
        let kind = TestCaseKind::Conformance {
            description: "test".to_string(),
        };
        let assertion = assertion_for_kind(&kind, "Patient");
        assert!(assertion.is_none());
    }

    // ── generate_test_plan edge case tests ──────────────────────────────

    #[test]
    fn generate_test_plan_skips_non_server_mode() {
        let cs = CapabilityStatement {
            resource_type: "CapabilityStatement".to_string(),
            url: Some("http://example.org/CapabilityStatement/test".to_string()),
            name: Some("TestCS".to_string()),
            status: Some("active".to_string()),
            software: None,
            implementation: None,
            messaging: vec![],
            document: vec![],
            rest: vec![Rest {
                mode: "client".to_string(), // not "server"
                resource: vec![RestResource {
                    resource_type: "Patient".to_string(),
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![RestInteraction {
                        code: "read".to_string(),
                    }],
                    search_param: vec![],
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
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        // No test groups because mode is "client", not "server"
        assert_eq!(plan.test_groups.len(), 0);
    }

    #[test]
    fn generate_test_plan_skips_non_resource_types() {
        let cs = CapabilityStatement {
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
                    resource_type: "Parameters".to_string(), // non-resource type
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Parameters".to_string()),
                    supported_profile: vec![],
                    interaction: vec![RestInteraction {
                        code: "read".to_string(),
                    }],
                    search_param: vec![],
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
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        // Parameters should be skipped (no test groups)
        assert_eq!(plan.test_groups.len(), 0);
    }

    #[test]
    fn generate_test_plan_system_interactions() {
        let cs = CapabilityStatement {
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
                resource: vec![],
                interaction: vec![
                    RestInteraction {
                        code: "batch".to_string(),
                    },
                    RestInteraction {
                        code: "transaction".to_string(),
                    },
                    RestInteraction {
                        code: "search-system".to_string(),
                    },
                    RestInteraction {
                        code: "history-system".to_string(),
                    },
                ],
                operation: vec![],
                security: None,
            }],
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        // Should have a system_interactions group
        let sys_group = plan
            .test_groups
            .iter()
            .find(|g| g.resource_type == "$$system_interactions");
        assert!(sys_group.is_some(), "Should have system interactions group");
        let sys_group = sys_group.unwrap();
        // batch, transaction, search-system, history-system = 4 interactions + 1 search-system test = 5
        assert_eq!(sys_group.tests.len(), 5);
    }

    #[test]
    fn generate_test_plan_system_operations() {
        let cs = CapabilityStatement {
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
                resource: vec![],
                interaction: vec![],
                operation: vec![RestOperation {
                    name: "export".to_string(),
                    definition: Some(
                        "http://hl7.org/fhir/uv/bulkdata/OperationDefinition/export".to_string(),
                    ),
                }],
                security: None,
            }],
        };
        let ops = vec![OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/uv/bulkdata/OperationDefinition/export".to_string(),
            name: "export".to_string(),
            code: "export".to_string(),
            system: Some(true),
            type_: Some(false),
            instance: Some(false),
            parameter: vec![],
            affects_state: Some(false),
            idempotent: Some(true),
        }];
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], Some(&ops), None, &empty_fv, &empty_ids);
        // Should have a system $export group
        let sys_op_group = plan
            .test_groups
            .iter()
            .find(|g| g.resource_type == "$$export");
        assert!(
            sys_op_group.is_some(),
            "Should have system operation group for $export"
        );
        let sys_op_group = sys_op_group.unwrap();
        // Base operation + affects_state=false test + idempotent=true test + scope tests
        assert!(sys_op_group.tests.len() >= 3);
    }

    #[test]
    fn generate_test_plan_conditional_operations() {
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![
                        RestInteraction {
                            code: "read".to_string(),
                        },
                        RestInteraction {
                            code: "create".to_string(),
                        },
                        RestInteraction {
                            code: "update".to_string(),
                        },
                        RestInteraction {
                            code: "delete".to_string(),
                        },
                    ],
                    search_param: vec![],
                    operation: vec![],
                    read_history: None,
                    update_create: Some(true),
                    versioning: None,
                    conditional_create: Some(true),
                    conditional_read: Some("if-modified-since".to_string()),
                    conditional_update: Some(true),
                    conditional_delete: Some("single".to_string()),
                    search_include: vec![],
                    search_revinclude: vec![],
                }],
                interaction: vec![],
                operation: vec![],
                security: None,
            }],
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];
        // Should have conditional tests
        let test_names: Vec<&str> = group.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(
            test_names.iter().any(|n| n.contains("conditional_create")),
            "Should have conditional create test"
        );
        assert!(
            test_names.iter().any(|n| n.contains("conditional_update")),
            "Should have conditional update test"
        );
        assert!(
            test_names.iter().any(|n| n.contains("conditional_read")),
            "Should have conditional read test"
        );
        assert!(
            test_names.iter().any(|n| n.contains("conditional_delete")),
            "Should have conditional delete test"
        );
        assert!(
            test_names.iter().any(|n| n.contains("update_create")),
            "Should have update-create test"
        );
    }

    #[test]
    fn generate_test_plan_history_and_read_params() {
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![
                        RestInteraction {
                            code: "read".to_string(),
                        },
                        RestInteraction {
                            code: "history-instance".to_string(),
                        },
                        RestInteraction {
                            code: "history-type".to_string(),
                        },
                    ],
                    search_param: vec![],
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
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];
        let test_names: Vec<&str> = group.tests.iter().map(|t| t.name.as_str()).collect();
        // Should have history param tests
        assert!(
            test_names.iter().any(|n| n.contains("history")),
            "Should have history param tests"
        );
        // Should have read param tests (_elements, _summary)
        assert!(
            test_names.iter().any(|n| n.contains("_elements")),
            "Should have _elements read param test"
        );
        assert!(
            test_names.iter().any(|n| n.contains("_summary")),
            "Should have _summary read param test"
        );
    }

    #[test]
    fn generate_test_plan_ig_url_passthrough() {
        let cs = sample_capability_statement();
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(
            &cs,
            &[],
            None,
            Some("http://example.org/ig"),
            &empty_fv,
            &empty_ids,
        );
        assert_eq!(plan.ig_url, Some("http://example.org/ig".to_string()));
    }

    #[test]
    fn generate_test_plan_unnamed_cs_fallback() {
        let cs = CapabilityStatement {
            resource_type: "CapabilityStatement".to_string(),
            url: Some("http://example.org/CapabilityStatement/test".to_string()),
            name: None, // no name
            status: Some("active".to_string()),
            software: None,
            implementation: None,
            messaging: vec![],
            document: vec![],
            rest: vec![],
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        assert_eq!(plan.name, "Unnamed IG");
    }

    #[test]
    fn generate_test_plan_composite_search_params() {
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![RestInteraction {
                        code: "search-type".to_string(),
                    }],
                    search_param: vec![],
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
        };
        let search_params = vec![SearchParameter {
            resource_type: "SearchParameter".to_string(),
            url: "http://hl7.org/fhir/SearchParameter/Patient-composite-test".to_string(),
            name: "composite-test".to_string(),
            code: "composite-test".to_string(),
            base: vec!["Patient".to_string()],
            param_type: "composite".to_string(),
            expression: None,
            description: None,
            target: vec![],
            comparator: vec![],
            modifier: vec![],
        }];
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &search_params, None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];
        assert!(
            group
                .tests
                .iter()
                .any(|t| matches!(t.kind, TestCaseKind::SearchComposite { .. })),
            "Should have composite search test"
        );
    }

    #[test]
    fn generate_test_plan_declared_comparators_and_modifiers() {
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![RestInteraction {
                        code: "search-type".to_string(),
                    }],
                    search_param: vec![],
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
        };
        let search_params = vec![SearchParameter {
            resource_type: "SearchParameter".to_string(),
            url: "http://hl7.org/fhir/SearchParameter/individual-birthdate".to_string(),
            name: "birthdate".to_string(),
            code: "birthdate".to_string(),
            base: vec!["Patient".to_string()],
            param_type: "date".to_string(),
            expression: Some("Patient.birthDate".to_string()),
            description: None,
            target: vec![],
            comparator: vec!["gt".to_string(), "lt".to_string()],
            modifier: vec!["missing".to_string(), "exact".to_string()],
        }];
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &search_params, None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];
        // Should have declared comparator tests (gt, lt)
        let prefix_tests: Vec<&TestCase> = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::SearchPrefix { .. }))
            .collect();
        assert!(
            prefix_tests.len() >= 2,
            "Should have at least 2 declared comparator tests"
        );
        // Should have declared modifier tests (missing, exact)
        let modifier_tests: Vec<&TestCase> = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::SearchModifier { .. }))
            .collect();
        assert!(
            modifier_tests.len() >= 2,
            "Should have at least 2 declared modifier tests"
        );
    }

    #[test]
    fn generate_test_plan_include_revinclude_combined() {
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![RestInteraction {
                        code: "search-type".to_string(),
                    }],
                    search_param: vec![],
                    operation: vec![],
                    read_history: None,
                    update_create: None,
                    versioning: None,
                    conditional_create: None,
                    conditional_read: None,
                    conditional_update: None,
                    conditional_delete: None,
                    search_include: vec!["Patient:organization".to_string()],
                    search_revinclude: vec!["Observation:subject".to_string()],
                }],
                interaction: vec![],
                operation: vec![],
                security: None,
            }],
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];
        // Should have _include + _revinclude combined test
        assert!(
            group
                .tests
                .iter()
                .any(|t| t.name.contains("include_revinclude")),
            "Should have _include + _revinclude combined test"
        );
    }

    #[test]
    fn generate_test_plan_operation_affects_state_idempotent_error_scope() {
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![],
                    search_param: vec![],
                    operation: vec![RestOperation {
                        name: "validate".to_string(),
                        definition: Some(
                            "http://hl7.org/fhir/OperationDefinition/Resource-validate".to_string(),
                        ),
                    }],
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
        };
        let ops = vec![OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/OperationDefinition/Resource-validate".to_string(),
            name: "validate".to_string(),
            code: "validate".to_string(),
            system: Some(false),
            type_: Some(false),
            instance: Some(true), // instance-only — scope tests will try system level
            parameter: vec![OperationParameter {
                name: "resource".to_string(),
                use_: Some("in".to_string()),
                min: Some(1),
                max: Some("1".to_string()),
                param_type: Some("Resource".to_string()),
            }],
            affects_state: Some(false),
            idempotent: Some(true),
        }];
        let empty_fv = HashMap::new();
        let mut created_ids = HashMap::new();
        created_ids.insert("Patient".to_string(), "patient-123".to_string());
        let plan = generate_test_plan(&cs, &[], Some(&ops), None, &empty_fv, &created_ids);
        let group = &plan.test_groups[0];
        let test_names: Vec<&str> = group.tests.iter().map(|t| t.name.as_str()).collect();
        // Should have affects_state=false test
        assert!(
            test_names.iter().any(|n| n.contains("affects_state")),
            "Should have affects_state=false test"
        );
        // Should have idempotent test
        assert!(
            test_names.iter().any(|n| n.contains("idempotent")),
            "Should have idempotent test"
        );
        // Should have error test (has required input param)
        assert!(
            test_names
                .iter()
                .any(|n| n.contains("missing_required_params")),
            "Should have error test for required input"
        );
        // Should have scope tests
        assert!(
            test_names.iter().any(|n| n.contains("scope")),
            "Should have scope tests"
        );
    }

    #[test]
    fn generate_test_plan_field_values_in_urls() {
        let cs = sample_capability_statement();
        let mut field_values = HashMap::new();
        let mut patient_fields = HashMap::new();
        patient_fields.insert("Patient.name[0].family".to_string(), "Smith".to_string());
        patient_fields.insert("Patient.birthDate".to_string(), "1990-01-01".to_string());
        field_values.insert("Patient".to_string(), patient_fields);
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(
            &cs,
            &sample_search_params(),
            None,
            None,
            &field_values,
            &empty_ids,
        );
        let group = &plan.test_groups[0];
        // Search single tests should use field values in URLs
        let single_tests: Vec<&TestCase> = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::SearchSingle { .. }))
            .collect();
        assert!(!single_tests.is_empty(), "Should have single search tests");
        // At least one should have a URL with the field value
        let has_field_value = single_tests
            .iter()
            .any(|t| t.request.url.contains("Smith") || t.request.url.contains("1990"));
        assert!(has_field_value, "Search URLs should contain field values");
    }

    #[test]
    fn generate_test_plan_created_ids_in_urls() {
        let cs = sample_capability_statement();
        let empty_fv = HashMap::new();
        let mut created_ids = HashMap::new();
        created_ids.insert("Patient".to_string(), "patient-123".to_string());
        let plan = generate_test_plan(
            &cs,
            &sample_search_params(),
            None,
            None,
            &empty_fv,
            &created_ids,
        );
        let group = &plan.test_groups[0];
        // Interaction tests use {id} placeholder which gets resolved later
        // by the orchestrator. Verify the URL pattern is correct.
        let interaction_tests: Vec<&TestCase> = group
            .tests
            .iter()
            .filter(|t| matches!(t.kind, TestCaseKind::Interaction))
            .collect();
        assert!(
            !interaction_tests.is_empty(),
            "Should have interaction tests"
        );
        // At least one should have a URL with the {id} placeholder
        let has_id_placeholder = interaction_tests
            .iter()
            .any(|t| t.request.url.contains("{id}"));
        assert!(
            has_id_placeholder,
            "Interaction URLs should contain {{id}} placeholder"
        );
    }

    #[test]
    fn generate_test_plan_conditional_read_not_supported() {
        // When conditional_read is "not-supported", no conditional read tests should be generated
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![RestInteraction {
                        code: "read".to_string(),
                    }],
                    search_param: vec![],
                    operation: vec![],
                    read_history: None,
                    update_create: None,
                    versioning: None,
                    conditional_create: None,
                    conditional_read: Some("not-supported".to_string()),
                    conditional_update: None,
                    conditional_delete: None,
                    search_include: vec![],
                    search_revinclude: vec![],
                }],
                interaction: vec![],
                operation: vec![],
                security: None,
            }],
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];
        let test_names: Vec<&str> = group.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(
            !test_names.iter().any(|n| n.contains("conditional_read")),
            "Should NOT have conditional read tests when not-supported"
        );
    }

    #[test]
    fn generate_test_plan_conditional_delete_not_supported() {
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![],
                    interaction: vec![RestInteraction {
                        code: "delete".to_string(),
                    }],
                    search_param: vec![],
                    operation: vec![],
                    read_history: None,
                    update_create: None,
                    versioning: None,
                    conditional_create: None,
                    conditional_read: None,
                    conditional_update: None,
                    conditional_delete: Some("not-supported".to_string()),
                    search_include: vec![],
                    search_revinclude: vec![],
                }],
                interaction: vec![],
                operation: vec![],
                security: None,
            }],
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];
        let test_names: Vec<&str> = group.tests.iter().map(|t| t.name.as_str()).collect();
        assert!(
            !test_names.iter().any(|n| n.contains("conditional_delete")),
            "Should NOT have conditional delete tests when not-supported"
        );
    }

    #[test]
    fn generate_test_plan_supported_profile_preferred() {
        // When supported_profile is set, it should be preferred over profile
        let cs = CapabilityStatement {
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
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
                    supported_profile: vec![
                        "http://example.org/StructureDefinition/SupportedProfile".to_string(),
                    ],
                    interaction: vec![RestInteraction {
                        code: "read".to_string(),
                    }],
                    search_param: vec![],
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
        };
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let plan = generate_test_plan(&cs, &[], None, None, &empty_fv, &empty_ids);
        let group = &plan.test_groups[0];
        // The profile_url should be the supported_profile, not the profile
        assert_eq!(
            group.profile_url,
            Some("http://example.org/StructureDefinition/SupportedProfile".to_string())
        );
    }
}
