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
        // 3 result params (_summary, _count, _sort) × 2 variants each (real ID + empty) = 6
        // + 1 _list test + 1 _query test = 8 total
        assert_eq!(result_params, 8);

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
}
