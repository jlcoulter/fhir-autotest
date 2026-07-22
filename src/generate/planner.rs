use crate::model::*;
use crate::generate::model::*;
use std::collections::HashMap;

/// Generate a test plan from a CapabilityStatement's rest resources and their
/// supported interactions.
pub fn generate_test_plan(
    cs: &CapabilityStatement,
    profiles: &[StructureDefinition],
    search_params: &[SearchParameter],
    ig_url: Option<&str>,
) -> TestPlan {
    let mut test_groups = Vec::new();

    for rest in &cs.rest {
        if rest.mode != "server" {
            continue;
        }

        for resource in &rest.resource {
            let profile_url = resource
                .supported_profile
                .first()
                .cloned()
                .or_else(|| resource.profile.clone());

            let group = build_test_group(resource, &profile_url, profiles, search_params);
            test_groups.push(group);
        }
    }

    let name = cs
        .name
        .clone()
        .unwrap_or_else(|| "Unnamed IG".to_string());

    TestPlan {
        name,
        ig_url: ig_url.map(|s| s.to_string()),
        test_groups,
        creation_order: Vec::new(),
        setup_resources: HashMap::new(),
    }
}

fn build_test_group(
    resource: &RestResource,
    profile_url: &Option<String>,
    _profiles: &[StructureDefinition],
    _search_params: &[SearchParameter],
) -> TestGroup {
    let mut tests = Vec::new();

    for interaction in &resource.interaction {
        let interaction_type = match Interaction::from_code(&interaction.code) {
            Some(i) => i,
            None => continue,
        };

        let test_case = build_test_case(&resource.resource_type, &interaction_type, profile_url);
        tests.push(test_case);
    }

    // Add search tests for each search parameter
    for sp in &resource.search_param {
        let test_case = build_search_test(&resource.resource_type, sp, profile_url);
        tests.push(test_case);
    }

    TestGroup {
        resource_type: resource.resource_type.clone(),
        profile_url: profile_url.clone(),
        tests,
    }
}

fn build_test_case(
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
        Interaction::Operation(name) => ("POST", format!("${name}"), 200),
    };

    let required_elements = if matches!(interaction, Interaction::Read | Interaction::Create) {
        vec![format!("{resource_type}.id")]
    } else {
        Vec::new()
    };

    TestCase {
        name: format!(
            "{}_{}",
            resource_type.to_lowercase(),
            interaction.label()
        ),
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
            profile_url: if matches!(
                interaction,
                Interaction::Read | Interaction::Create | Interaction::Update
            ) {
                profile_url.clone()
            } else {
                None
            },
            required_elements,
            forbidden_elements: Vec::new(),
        },
    }
}

fn build_search_test(
    resource_type: &str,
    sp: &RestSearchParam,
    profile_url: &Option<String>,
) -> TestCase {
    let url = format!("/{resource_type}?{name}=_test_value", name = sp.name);

    TestCase {
        name: format!(
            "{}_search_{}",
            resource_type.to_lowercase(),
            sp.name.replace('-', "_")
        ),
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
        },
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
                        RestInteraction { code: "read".to_string() },
                        RestInteraction { code: "search-type".to_string() },
                        RestInteraction { code: "create".to_string() },
                        RestInteraction { code: "update".to_string() },
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
                    operation: vec![],
                    read_history: None,
                    update_create: None,
                    conditional_create: None,
                    conditional_read: None,
                    conditional_update: None,
                    conditional_delete: None,
                }],
                interaction: vec![],
            }],
        }
    }

    #[test]
    fn generate_test_plan_from_capability_statement() {
        let cs = sample_capability_statement();
        let plan = generate_test_plan(&cs, &[], &[], None);

        assert_eq!(plan.test_groups.len(), 1);
        assert_eq!(plan.test_groups[0].resource_type, "Patient");

        let interactions: Vec<&Interaction> = plan.test_groups[0]
            .tests
            .iter()
            .map(|t| &t.interaction)
            .collect();

        assert!(interactions.iter().any(|i| **i == Interaction::Read));
        assert!(interactions.iter().any(|i| **i == Interaction::SearchType));
        assert!(interactions.iter().any(|i| **i == Interaction::Create));
        assert!(interactions.iter().any(|i| **i == Interaction::Update));

        // Should also have search tests for "name" and "birthdate"
        let search_tests: Vec<&TestCase> = plan.test_groups[0]
            .tests
            .iter()
            .filter(|t| t.name.contains("search_"))
            .collect();
        assert_eq!(search_tests.len(), 2);
    }

    #[test]
    fn test_case_urls_contain_resource_type() {
        let cs = sample_capability_statement();
        let plan = generate_test_plan(&cs, &[], &[], None);

        for test in &plan.test_groups[0].tests {
            assert!(
                test.request.url.contains("Patient"),
                "URL '{}' should contain 'Patient'",
                test.request.url
            );
        }
    }
}