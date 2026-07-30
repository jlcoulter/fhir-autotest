use crate::generate::model::*;
use std::collections::HashMap;

pub(crate) fn build_negative_test(
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
