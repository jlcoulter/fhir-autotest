use crate::generate::model::*;
use std::collections::HashMap;

pub(crate) fn build_include_test(
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

/// Build a test for `_include=*` (wildcard include).
/// Includes all referenced resources regardless of type.
pub(crate) fn build_include_wildcard_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url = format!("/{resource_type}?_include=*&_id={{id}}");

    TestCase {
        name: format!("{}_include_wildcard", resource_type.to_lowercase()),
        kind: TestCaseKind::Include {
            param: "*".to_string(),
            revinclude: false,
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
                include_requires_distinct_from: Some(resource_type.to_string()),
                ..ResponseAssertion::none()
            }),
        },
    }
}

/// Build a test for `_revinclude=*` (wildcard reverse include).
/// Includes all resources that reference the matched resources.
pub(crate) fn build_revinclude_wildcard_test(
    resource_type: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url = format!("/{resource_type}?_revinclude=*&_id={{id}}");

    TestCase {
        name: format!("{}_revinclude_wildcard", resource_type.to_lowercase()),
        kind: TestCaseKind::Include {
            param: "*".to_string(),
            revinclude: true,
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

/// Build a test for `_include:recurse=ResourceType:param` (recursive include).
pub(crate) fn build_include_recurse_test(
    resource_type: &str,
    param_name: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url = format!("/{resource_type}?_include:recurse={resource_type}:{param_name}&_id={{id}}");

    TestCase {
        name: format!(
            "{}_include_recurse_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_")
        ),
        kind: TestCaseKind::Include {
            param: param_name.to_string(),
            revinclude: false,
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
            expected_status: 0, // servers may not support :recurse
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

/// Build a test for `_include:iterate=ResourceType:param` (iterative include).
pub(crate) fn build_include_iterate_test(
    resource_type: &str,
    param_name: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url = format!("/{resource_type}?_include:iterate={resource_type}:{param_name}&_id={{id}}");

    TestCase {
        name: format!(
            "{}_include_iterate_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_")
        ),
        kind: TestCaseKind::Include {
            param: param_name.to_string(),
            revinclude: false,
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
            expected_status: 0, // servers may not support :iterate
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

/// Build a test for `_include=ResourceType:param:TargetType` (target type specification).
pub(crate) fn build_include_target_type_test(
    resource_type: &str,
    param_name: &str,
    target_type: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url =
        format!("/{resource_type}?_include={resource_type}:{param_name}:{target_type}&_id={{id}}");

    let mut include_types = HashMap::new();
    include_types.insert(target_type.to_string(), param_name.to_string());

    TestCase {
        name: format!(
            "{}_include_{}_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_"),
            target_type.to_lowercase()
        ),
        kind: TestCaseKind::Include {
            param: param_name.to_string(),
            revinclude: false,
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
                ..ResponseAssertion::none()
            }),
        },
    }
}

/// Build a test with multiple `_include` parameters in one request.
/// Uses the first two declared includes for the resource.
pub(crate) fn build_include_multiple_test(
    resource_type: &str,
    param_names: &[String],
    profile_url: &Option<String>,
) -> TestCase {
    let params: Vec<String> = param_names
        .iter()
        .map(|p| format!("_include={resource_type}:{p}"))
        .collect();
    let url = format!("/{resource_type}?{}&_id={{id}}", params.join("&"));

    TestCase {
        name: format!(
            "{}_include_multiple_{}",
            resource_type.to_lowercase(),
            param_names
                .iter()
                .map(|p| p.replace('-', "_"))
                .collect::<Vec<_>>()
                .join("_")
        ),
        kind: TestCaseKind::Include {
            param: param_names.join("+"),
            revinclude: false,
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

/// Build a test with `_include` + `_revinclude` combined in one request.
pub(crate) fn build_include_revinclude_combined_test(
    resource_type: &str,
    include_param: &str,
    revinclude_source: &str,
    revinclude_param: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url = format!(
        "/{resource_type}?_include={resource_type}:{include_param}&_revinclude={revinclude_source}:{revinclude_param}&_id={{id}}"
    );

    TestCase {
        name: format!(
            "{}_include_revinclude_combined_{}_{}",
            resource_type.to_lowercase(),
            include_param.replace('-', "_"),
            revinclude_param.replace('-', "_")
        ),
        kind: TestCaseKind::Include {
            param: format!("{}+rev:{}", include_param, revinclude_param),
            revinclude: false,
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

/// Build a test for `_include:recurse:iterate=ResourceType:param` (combined modifiers).
pub(crate) fn build_include_recurse_iterate_test(
    resource_type: &str,
    param_name: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url = format!(
        "/{resource_type}?_include:recurse:iterate={resource_type}:{param_name}&_id={{id}}"
    );

    TestCase {
        name: format!(
            "{}_include_recurse_iterate_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_")
        ),
        kind: TestCaseKind::Include {
            param: param_name.to_string(),
            revinclude: false,
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
            expected_status: 0, // servers may not support combined modifiers
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

/// Build a test for `_revinclude:recurse=SourceType:param` (recursive reverse include).
pub(crate) fn build_revinclude_recurse_test(
    resource_type: &str,
    source_resource: &str,
    param_name: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url =
        format!("/{resource_type}?_revinclude:recurse={source_resource}:{param_name}&_id={{id}}");

    TestCase {
        name: format!(
            "{}_revinclude_recurse_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_")
        ),
        kind: TestCaseKind::Include {
            param: param_name.to_string(),
            revinclude: true,
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
            expected_status: 0, // servers may not support :recurse
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

/// Build a test for `_revinclude:iterate=SourceType:param` (iterative reverse include).
pub(crate) fn build_revinclude_iterate_test(
    resource_type: &str,
    source_resource: &str,
    param_name: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url =
        format!("/{resource_type}?_revinclude:iterate={source_resource}:{param_name}&_id={{id}}");

    TestCase {
        name: format!(
            "{}_revinclude_iterate_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_")
        ),
        kind: TestCaseKind::Include {
            param: param_name.to_string(),
            revinclude: true,
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
            expected_status: 0, // servers may not support :iterate
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
    fn build_include_wildcard_test_creates_correct_url() {
        let test = build_include_wildcard_test("Patient", &None);
        assert!(
            test.request.url.contains("?_include=*"),
            "URL should contain ?_include=*, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_include_wildcard");
        assert_eq!(test.validation.expected_status, 200);
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
    fn build_revinclude_wildcard_test_creates_correct_url() {
        let test = build_revinclude_wildcard_test("Patient", &None);
        assert!(
            test.request.url.contains("?_revinclude=*"),
            "URL should contain ?_revinclude=*, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_revinclude_wildcard");
        assert_eq!(test.validation.expected_status, 200);
    }

    #[test]
    fn build_include_recurse_test_creates_correct_url() {
        let test = build_include_recurse_test("Patient", "organization", &None);
        assert!(
            test.request
                .url
                .contains("?_include:recurse=Patient:organization"),
            "URL should contain ?_include:recurse=Patient:organization, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_include_recurse_organization");
        assert_eq!(test.validation.expected_status, 0);
    }

    #[test]
    fn build_include_iterate_test_creates_correct_url() {
        let test = build_include_iterate_test("Patient", "organization", &None);
        assert!(
            test.request
                .url
                .contains("?_include:iterate=Patient:organization"),
            "URL should contain ?_include:iterate=Patient:organization, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_include_iterate_organization");
        assert_eq!(test.validation.expected_status, 0);
    }

    #[test]
    fn build_include_target_type_test_creates_correct_url() {
        let test = build_include_target_type_test("Patient", "organization", "Organization", &None);
        assert!(
            test.request
                .url
                .contains("?_include=Patient:organization:Organization"),
            "URL should contain ?_include=Patient:organization:Organization, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_include_organization_organization");
        let assertion = test.validation.response_assertion.unwrap();
        assert_eq!(
            assertion.include_types.get("Organization"),
            Some(&"organization".to_string())
        );
    }

    #[test]
    fn build_include_multiple_test_creates_correct_url() {
        let params = vec![
            "organization".to_string(),
            "general-practitioner".to_string(),
        ];
        let test = build_include_multiple_test("Patient", &params, &None);
        assert!(
            test.request.url.contains("_include=Patient:organization"),
            "URL should contain _include=Patient:organization, got {}",
            test.request.url
        );
        assert!(
            test.request
                .url
                .contains("_include=Patient:general-practitioner"),
            "URL should contain _include=Patient:general-practitioner, got {}",
            test.request.url
        );
        assert_eq!(
            test.name,
            "patient_include_multiple_organization_general_practitioner"
        );
    }

    #[test]
    fn build_include_revinclude_combined_test_creates_correct_url() {
        let test = build_include_revinclude_combined_test(
            "Patient",
            "organization",
            "Observation",
            "subject",
            &None,
        );
        assert!(
            test.request.url.contains("_include=Patient:organization"),
            "URL should contain _include=Patient:organization, got {}",
            test.request.url
        );
        assert!(
            test.request.url.contains("_revinclude=Observation:subject"),
            "URL should contain _revinclude=Observation:subject, got {}",
            test.request.url
        );
        assert_eq!(
            test.name,
            "patient_include_revinclude_combined_organization_subject"
        );
    }

    #[test]
    fn build_include_recurse_iterate_test_creates_correct_url() {
        let test = build_include_recurse_iterate_test("Patient", "organization", &None);
        assert!(
            test.request
                .url
                .contains("?_include:recurse:iterate=Patient:organization"),
            "URL should contain ?_include:recurse:iterate=Patient:organization, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_include_recurse_iterate_organization");
        assert_eq!(test.validation.expected_status, 0);
    }

    #[test]
    fn build_revinclude_recurse_test_creates_correct_url() {
        let test = build_revinclude_recurse_test("Patient", "Observation", "subject", &None);
        assert!(
            test.request
                .url
                .contains("?_revinclude:recurse=Observation:subject"),
            "URL should contain ?_revinclude:recurse=Observation:subject, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_revinclude_recurse_subject");
        assert_eq!(test.validation.expected_status, 0);
    }

    #[test]
    fn build_revinclude_iterate_test_creates_correct_url() {
        let test = build_revinclude_iterate_test("Patient", "Observation", "subject", &None);
        assert!(
            test.request
                .url
                .contains("?_revinclude:iterate=Observation:subject"),
            "URL should contain ?_revinclude:iterate=Observation:subject, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_revinclude_iterate_subject");
        assert_eq!(test.validation.expected_status, 0);
    }
}
