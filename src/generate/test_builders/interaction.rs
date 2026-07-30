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
