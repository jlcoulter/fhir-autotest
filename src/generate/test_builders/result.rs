use crate::generate::model::*;
use crate::model::RestSearchParam;
use std::collections::HashMap;

/// Fields that are never summary in FHIR R4 and should be absent when `_summary=true`.
///
/// Per the FHIR specification, `_summary=true` returns only elements with
/// `isSummary=true`. The following elements are never summary across all
/// resource types in the base FHIR R4 specification.
pub(crate) fn summary_absent_fields() -> Vec<String> {
    vec![
        "text".to_string(),
        "contained".to_string(),
        "extension".to_string(),
        "modifierExtension".to_string(),
    ]
}

/// Fields that should be absent when `_elements=id,meta,name` is used.
/// These are common top-level fields that are not in the requested set
/// and should be filtered out by the server's _elements handling.
fn elements_forbidden_fields() -> Vec<String> {
    vec![
        "text".to_string(),
        "contained".to_string(),
        "extension".to_string(),
        "modifierExtension".to_string(),
    ]
}

pub(crate) fn build_result_param_test(
    resource_type: &str,
    param: &str,
    value: &str,
    profile_url: &Option<String>,
    declared_params: &[RestSearchParam],
    _created_ids: &HashMap<String, String>,
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

    // Determine expected status: 0 (accept any) for params servers may not support
    let expected_status: u16 = match param {
        "_filter" | "_source" | "_language" | "_contained" | "_containedType"
        | "_getpagesoffset" => 0,
        _ => 200,
    };

    // Test A: with real resource ID (uses {id} placeholder resolved at runtime)
    {
        let url = format!("/{resource_type}?{actual_param}={actual_value}&_id={{id}}");

        let response_assertion = match param {
            "_count" => {
                if value == "0" {
                    // _count=0: Bundle with total but no entries
                    Some(ResponseAssertion {
                        bundle_type: Some("searchset".to_string()),
                        bundle_total_present: Some(true),
                        max_entries: Some(0),
                        ..ResponseAssertion::none()
                    })
                } else {
                    Some(ResponseAssertion {
                        bundle_type: Some("searchset".to_string()),
                        min_entries: Some(1),
                        max_entries: Some(1),
                        ..ResponseAssertion::none()
                    })
                }
            }
            "_summary" => {
                if value == "count" {
                    Some(ResponseAssertion {
                        bundle_type: Some("searchset".to_string()),
                        bundle_total_present: Some(true),
                        max_entries: Some(0),
                        summary_mode: Some("count".to_string()),
                        ..ResponseAssertion::none()
                    })
                } else if value == "text" {
                    // _summary=text: servers may or may not add a Narrative
                    // to resources that lack one. Accept any status.
                    Some(ResponseAssertion {
                        bundle_type: Some("searchset".to_string()),
                        min_entries: Some(0),
                        ..ResponseAssertion::none()
                    })
                } else if value == "data" {
                    // _summary=data: servers may or may not strip text.
                    // Accept any status.
                    Some(ResponseAssertion {
                        bundle_type: Some("searchset".to_string()),
                        min_entries: Some(0),
                        ..ResponseAssertion::none()
                    })
                } else {
                    // _summary=true (original behaviour)
                    Some(ResponseAssertion {
                        bundle_type: Some("searchset".to_string()),
                        min_entries: Some(1),
                        absent_fields: summary_absent_fields(),
                        ..ResponseAssertion::none()
                    })
                }
            }
            "_sort" => {
                // For multi-field sort, check if value contains commas
                let fields: Vec<&str> = value.split(',').collect();
                let primary = fields.first().unwrap_or(&"_lastUpdated").to_string();
                let additional: Vec<String> =
                    fields.iter().skip(1).map(|s| s.to_string()).collect();
                Some(ResponseAssertion {
                    bundle_type: Some("searchset".to_string()),
                    sort_by: Some(SortAssertion {
                        field: sort_field.clone().unwrap_or(primary),
                        direction: "asc".to_string(),
                        additional_fields: additional,
                    }),
                    ..ResponseAssertion::none()
                })
            }
            "_elements" => Some(ResponseAssertion {
                bundle_type: Some("searchset".to_string()),
                min_entries: Some(1),
                ..ResponseAssertion::none()
            }),
            "_total" => {
                if value == "none" {
                    Some(ResponseAssertion {
                        bundle_type: Some("searchset".to_string()),
                        bundle_total_present: Some(false),
                        ..ResponseAssertion::none()
                    })
                } else {
                    Some(ResponseAssertion {
                        bundle_type: Some("searchset".to_string()),
                        bundle_total_present: Some(true),
                        ..ResponseAssertion::none()
                    })
                }
            }
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
                expected_status,
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: if param == "_elements" {
                    elements_forbidden_fields()
                } else if param == "_elements:exclude" {
                    vec!["text".to_string(), "contained".to_string()]
                } else {
                    Vec::new()
                },
                response_assertion,
            },
        });
    }

    // Test B: with nonexistent ID — always returns empty Bundle
    let url_empty =
        format!("/{resource_type}?{actual_param}={actual_value}&_id=nonexistent-id-99999");

    let response_assertion_empty = if param == "_sort" {
        let fields: Vec<&str> = value.split(',').collect();
        let primary = fields.first().unwrap_or(&"_lastUpdated").to_string();
        let additional: Vec<String> = fields.iter().skip(1).map(|s| s.to_string()).collect();
        Some(ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            sort_by: Some(SortAssertion {
                field: sort_field.unwrap_or(primary),
                direction: "asc".to_string(),
                additional_fields: additional,
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
            expected_status,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: response_assertion_empty,
        },
    });

    tests
}

/// Build a test for the `_format` parameter.
/// Tests that the server respects the `_format` parameter for response format negotiation.
pub(crate) fn build_format_test(
    resource_type: &str,
    format: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let expected_status = match format {
        "json" => 0u16,
        "application/fhir+json" => 0u16,
        "xml" => 0u16,
        _ => 0u16,
    };

    TestCase {
        name: format!(
            "{}_format_{}",
            resource_type.to_lowercase(),
            format.replace(['/', '+'], "_")
        ),
        kind: TestCaseKind::ResultParam {
            param: "_format".to_string(),
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url: format!("/{}?_format={}&_id={{id}}", resource_type, format),
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status,
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

/// Build a test for the `_pretty` parameter.
/// Tests that the server respects the `_pretty` parameter for pretty-printing.
pub(crate) fn build_pretty_test(
    resource_type: &str,
    pretty: bool,
    profile_url: &Option<String>,
) -> TestCase {
    let value = if pretty { "true" } else { "false" };

    TestCase {
        name: format!("{}_pretty_{}", resource_type.to_lowercase(), value),
        kind: TestCaseKind::ResultParam {
            param: "_pretty".to_string(),
        },
        interaction: Interaction::SearchType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url: format!("/{}?_pretty={}&_id={{id}}", resource_type, value),
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 0, // pretty-printing is best-effort
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_result_param_test_count() {
        let tests = build_result_param_test("Patient", "_count", "1", &None, &[], &HashMap::new());
        assert_eq!(
            tests.len(),
            2,
            "Should produce both real-ID and empty-ID variants"
        );

        // Test A: real-ID variant
        let real_test = tests.iter().find(|t| !t.name.contains("empty")).unwrap();
        assert_eq!(real_test.request.method, "GET");
        assert!(
            real_test.request.url.contains("?_count=1"),
            "URL: {}",
            real_test.request.url
        );
        assert!(
            real_test.request.url.contains("&_id={id}"),
            "Real URL should have {{id}} placeholder: {}",
            real_test.request.url
        );
        assert_eq!(real_test.name, "patient_result_count");
        assert!(
            matches!(real_test.kind, TestCaseKind::ResultParam { ref param } if param == "_count")
        );

        // Test B: empty-ID variant
        let empty_test = tests.iter().find(|t| t.name.contains("empty")).unwrap();
        assert!(empty_test.request.url.contains("&_id=nonexistent-id-99999"));
        assert_eq!(empty_test.name, "patient_result_count_empty");
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
    fn build_result_param_test_elements() {
        let tests = build_result_param_test(
            "Patient",
            "_elements",
            "id,meta,name",
            &None,
            &[],
            &HashMap::new(),
        );
        assert_eq!(
            tests.len(),
            2,
            "Should produce both real-ID and empty-ID variants"
        );

        // Test A: real-ID variant
        let real_test = tests.iter().find(|t| !t.name.contains("empty")).unwrap();
        assert_eq!(real_test.request.method, "GET");
        assert!(
            real_test.request.url.contains("?_elements=id,meta,name"),
            "URL: {}",
            real_test.request.url
        );
        assert!(
            real_test.request.url.contains("&_id={id}"),
            "Real URL should have {{id}} placeholder: {}",
            real_test.request.url
        );
        assert_eq!(real_test.name, "patient_result_elements");
        assert!(
            matches!(real_test.kind, TestCaseKind::ResultParam { ref param } if param == "_elements")
        );
        assert!(
            !real_test.validation.forbidden_elements.is_empty(),
            "Should have forbidden elements for _elements test"
        );
        assert!(
            real_test
                .validation
                .forbidden_elements
                .contains(&"text".to_string()),
            "text should be forbidden"
        );

        // Test B: empty-ID variant
        let empty_test = tests.iter().find(|t| t.name.contains("empty")).unwrap();
        assert!(empty_test.request.url.contains("&_id=nonexistent-id-99999"));
        assert_eq!(empty_test.name, "patient_result_elements_empty");
    }
}
