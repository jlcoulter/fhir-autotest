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

    // Test A: with real resource ID (uses {id} placeholder resolved at runtime)
    // This exercises the result param behaviour on actual data.
    // The {id} placeholder is resolved at runtime by the orchestrator; if no
    // resource was created for this type, the test is skipped gracefully.
    {
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
                absent_fields: summary_absent_fields(),
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
}
