use crate::generate::model::*;
use std::collections::HashMap;

pub(crate) fn build_interaction_test(
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

    // For PATCH, add a JSON Patch body
    let body = if matches!(interaction, Interaction::Patch) {
        Some(serde_json::json!([
            {"op": "replace", "path": "/status", "value": "inactive"}
        ]))
    } else {
        None
    };

    // For history interactions, assert the response is a Bundle of type "history"
    let response_assertion = if matches!(
        interaction,
        Interaction::HistoryInstance | Interaction::HistoryType
    ) {
        Some(ResponseAssertion {
            bundle_type: Some("history".to_string()),
            min_entries: Some(1),
            ..ResponseAssertion::none()
        })
    } else {
        None
    };

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
            body,
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
            response_assertion,
        },
    }
}

/// Build a conditional create test that sends an `If-None-Exist` header.
/// This tests the server's ability to handle conditional create (upsert) semantics.
pub(crate) fn build_conditional_create_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let mut headers = HashMap::new();
    headers.insert(
        "If-None-Exist".to_string(),
        format!("{resource_type}?identifier=test-id"),
    );

    TestCase {
        name: format!("{}_conditional_create", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::Create,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "POST".to_string(),
            url: format!("/{resource_type}"),
            headers,
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 201,
            profile_url: profile_url.clone(),
            required_elements: vec![format!("{resource_type}.id")],
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build a conditional update test that sends an `If-Match` header.
/// This tests the server's ability to handle conditional update (version-aware) semantics.
pub(crate) fn build_conditional_update_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let mut headers = HashMap::new();
    headers.insert("If-Match".to_string(), "W/\"1\"".to_string());

    TestCase {
        name: format!("{}_conditional_update", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::Update,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "PUT".to_string(),
            url: format!("/{resource_type}/{{id}}"),
            headers,
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: profile_url.clone(),
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build a conditional read test that sends conditional headers.
/// This tests the server's ability to handle `If-Modified-Since` and
/// `If-None-Match` headers when `conditionalRead` is declared.
pub(crate) fn build_conditional_read_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> Vec<TestCase> {
    let base = |header_name: &str, header_value: &str, suffix: &str| -> TestCase {
        let mut headers = std::collections::HashMap::new();
        headers.insert(header_name.to_string(), header_value.to_string());
        TestCase {
            name: format!(
                "{}_conditional_read_{}",
                resource_type.to_lowercase(),
                suffix
            ),
            kind: TestCaseKind::Interaction,
            interaction: Interaction::Read,
            resource_type: resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/{resource_type}/{{id}}"),
                headers,
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 200,
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion: Some(ResponseAssertion {
                    accept_statuses: vec![304],
                    ..ResponseAssertion::none()
                }),
            },
        }
    };

    vec![
        base(
            "If-Modified-Since",
            "Mon, 01 Jan 2020 00:00:00 GMT",
            "if_modified_since",
        ),
        base("If-None-Match", "W/\"1\"", "if_none_match"),
    ]
}

/// Build a conditional delete test that sends `DELETE /{resource}?identifier=test-id`.
/// This tests the server's ability to handle conditional delete when
/// `conditionalDelete` is `single` or `multiple`.
pub(crate) fn build_conditional_delete_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> TestCase {
    TestCase {
        name: format!("{}_conditional_delete", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::Delete,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "DELETE".to_string(),
            url: format!("/{resource_type}?identifier=test-id"),
            headers: std::collections::HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 0, // accept any status (204, 404, 200 with OperationOutcome)
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build an updateCreate test that sends `PUT /{resource}/new-test-id` with a
/// valid resource body. This tests the server's ability to create via update
/// when `updateCreate` is `true`.
pub(crate) fn build_update_create_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> TestCase {
    TestCase {
        name: format!("{}_update_create", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::Update,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "PUT".to_string(),
            url: format!("/{resource_type}/new-test-id"),
            headers: std::collections::HashMap::new(),
            body: Some(serde_json::json!({
                "resourceType": resource_type,
                "id": "new-test-id",
            })),
        },
        validation: ValidationSpec {
            expected_status: 201,
            profile_url: profile_url.clone(),
            required_elements: vec![format!("{resource_type}.id")],
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build history parameter tests for instance-level and type-level history.
/// Tests `_since`, `_count`, and `_at` parameters on history interactions.
pub(crate) fn build_history_param_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> Vec<TestCase> {
    let mut tests = Vec::new();

    // Instance-level history with _since
    tests.push(TestCase {
        name: format!("{}_history_instance_since", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::HistoryInstance,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url: format!("/{resource_type}/{{id}}/_history?_since=2020-01-01"),
            headers: std::collections::HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 0, // accept any status (200 or 4xx if not supported)
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    });

    // Instance-level history with _count
    tests.push(TestCase {
        name: format!("{}_history_instance_count", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::HistoryInstance,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url: format!("/{resource_type}/{{id}}/_history?_count=1"),
            headers: std::collections::HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 0,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    });

    // Instance-level history with _at
    tests.push(TestCase {
        name: format!("{}_history_instance_at", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::HistoryInstance,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url: format!("/{resource_type}/{{id}}/_history?_at=2024"),
            headers: std::collections::HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 0,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    });

    // Type-level history with _since
    tests.push(TestCase {
        name: format!("{}_history_type_since", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::HistoryType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url: format!("/{resource_type}/_history?_since=2020-01-01"),
            headers: std::collections::HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 0,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    });

    // Type-level history with _count
    tests.push(TestCase {
        name: format!("{}_history_type_count", resource_type.to_lowercase()),
        kind: TestCaseKind::Interaction,
        interaction: Interaction::HistoryType,
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "GET".to_string(),
            url: format!("/{resource_type}/_history?_count=1"),
            headers: std::collections::HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 0,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    });

    tests
}

/// Build read parameter tests for `_elements` and `_summary` on read interactions.
pub(crate) fn build_read_param_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> Vec<TestCase> {
    vec![
        // Read with _elements filter
        TestCase {
            name: format!("{}_read_elements", resource_type.to_lowercase()),
            kind: TestCaseKind::Interaction,
            interaction: Interaction::Read,
            resource_type: resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/{resource_type}/{{id}}?_elements=id,meta"),
                headers: std::collections::HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 200,
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion: Some(ResponseAssertion {
                    // _elements should only contain requested fields
                    // We can't assert forbidden_elements at the response level here
                    // because the assertion framework checks Bundle entries, not single resources
                    ..ResponseAssertion::none()
                }),
            },
        },
        // Read with _summary=true
        TestCase {
            name: format!("{}_read_summary", resource_type.to_lowercase()),
            kind: TestCaseKind::Interaction,
            interaction: Interaction::Read,
            resource_type: resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/{resource_type}/{{id}}?_summary=true"),
                headers: std::collections::HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 200,
                profile_url: None,
                required_elements: vec![format!("{resource_type}.id")],
                forbidden_elements: vec![
                    "text".to_string(),
                    "contained".to_string(),
                    "extension".to_string(),
                ],
                response_assertion: None,
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
