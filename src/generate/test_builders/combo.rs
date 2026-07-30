use crate::generate::model::*;
use crate::generate::test_builders::search::resolve_param_value;
use std::collections::HashMap;

pub(crate) fn build_search_combo_test(
    resource_type: &str,
    params: &[(&str, &str)],
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let query: Vec<String> = params
        .iter()
        .map(|(name, ptype)| {
            let value = resolve_param_value(resource_type, name, ptype, field_values, created_ids);
            format!("{}={}", name, value)
        })
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

pub(crate) fn build_chained_search_test(
    resource_type: &str,
    chain_param: &str,
    target_param: &str,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    // For chained searches, resolve the target param value from the target resource type.
    // We don't know the target resource type at this point, so use the chain param's
    // reference target as a hint.
    let value = resolve_param_value(
        resource_type,
        target_param,
        "string",
        field_values,
        created_ids,
    );
    let url = format!("/{resource_type}?{chain_param}.{target_param}={value}");

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_search_combo_test_combines_params() {
        let params: [(&str, &str); 2] = [("name", "string"), ("birthdate", "date")];
        let test =
            build_search_combo_test("Patient", &params, &None, &HashMap::new(), &HashMap::new());
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("?name="),
            "URL should contain ?name=, got {}",
            test.request.url
        );
        assert!(
            test.request.url.contains("&birthdate="),
            "URL should contain &birthdate=, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_combo_name_and_birthdate");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchCombo { ref params } if params == &vec!["name".to_string(), "birthdate".to_string()]
        ));
    }

    #[test]
    fn build_chained_search_test_produces_chain_url() {
        let test = build_chained_search_test(
            "Patient",
            "organization",
            "name",
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("?organization.name="),
            "URL should contain ?organization.name=, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_chain_organization_name");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchChained { ref chain_param, ref target_param }
                if chain_param == "organization" && target_param == "name"
        ));
    }
}
