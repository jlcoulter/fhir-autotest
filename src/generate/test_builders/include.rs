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
}
