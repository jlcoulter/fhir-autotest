use crate::generate::model::*;
use crate::generate::value_resolver::resolve_search_value;
use crate::model::SearchParameter;
use std::collections::HashMap;

/// Resolve a search parameter value from generated resources, falling back
/// to a sentinel value if no real value is available.
pub(crate) fn resolve_param_value(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> String {
    let rt_values = field_values.get(resource_type);
    let empty = HashMap::new();
    let rt_values = rt_values.unwrap_or(&empty);
    let value = resolve_search_value(
        resource_type,
        param_name,
        param_type,
        rt_values,
        created_ids,
    );
    value.unwrap_or_else(|| sample_value(param_type).to_string())
}

/// Sample value for a search param based on its type.
/// Used as fallback when no generated resource value is available.
pub(crate) fn sample_value(param_type: &str) -> &'static str {
    match param_type {
        "string" => "test-value",
        "exact" => "test-value",
        "token" => "test-code",
        "reference" => "Patient/test-id",
        "number" => "1",
        "date" => "2024-01-01",
        "dateTime" => "2024-01-01T00:00:00Z",
        "quantity" => "5.0||http://unitsofmeasure.org|kg",
        "uri" => "http://example.org",
        "composite" => "test-value",
        "special" => "-25.0%7C133.0%7C3000%7Ckm", // near format: lat%7Clon%7Cdistance%7Cunits (pipes must be %-encoded for HAPI)
        _ => "test-value",
    }
}

pub(crate) fn build_search_single_test(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let value = resolve_param_value(
        resource_type,
        param_name,
        param_type,
        field_values,
        created_ids,
    );
    let url = format!("/{resource_type}?{param_name}={value}&_id={{id}}");

    TestCase {
        name: format!(
            "{}_search_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_")
        ),
        kind: TestCaseKind::SearchSingle {
            param_name: param_name.to_string(),
            param_type: param_type.to_string(),
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

pub(crate) fn build_search_single_from_sp(
    resource_type: &str,
    sp: &SearchParameter,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    build_search_single_test(
        resource_type,
        &sp.code,
        &sp.param_type,
        profile_url,
        field_values,
        created_ids,
    )
}

pub(crate) fn build_search_modifier_test(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    modifier: &SearchModifier,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let value = resolve_param_value(
        resource_type,
        param_name,
        param_type,
        field_values,
        created_ids,
    );
    let url = if matches!(modifier, SearchModifier::Missing) {
        // :missing takes true/false
        format!("/{resource_type}?{param_name}:missing=true")
    } else {
        format!("/{resource_type}?{param_name}{}={value}", modifier.suffix())
    };

    TestCase {
        name: format!(
            "{}_search_{}_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_"),
            format!("{:?}", modifier).to_lowercase()
        ),
        kind: TestCaseKind::SearchModifier {
            param_name: param_name.to_string(),
            modifier: modifier.clone(),
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

pub(crate) fn build_search_prefix_test(
    resource_type: &str,
    param_name: &str,
    param_type: &str,
    prefix: &SearchPrefix,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let value = resolve_param_value(
        resource_type,
        param_name,
        param_type,
        field_values,
        created_ids,
    );
    let url = format!(
        "/{resource_type}?{param_name}={prefix}{value}",
        prefix = prefix.prefix_str()
    );

    TestCase {
        name: format!(
            "{}_search_{}_{}",
            resource_type.to_lowercase(),
            param_name.replace('-', "_"),
            prefix.prefix_str()
        ),
        kind: TestCaseKind::SearchPrefix {
            param_name: param_name.to_string(),
            prefix: prefix.clone(),
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

/// Build a proximity/near search test for FHIR special-type params (e.g. Location?near).
///
/// FHIR near format: `?near=lat|lon[|distance[|units]]`
/// Use a conservative lat|lon payload for broader server compatibility.
pub(crate) fn build_search_near_test(
    resource_type: &str,
    param_name: &str,
    profile_url: &Option<String>,
) -> TestCase {
    // Include distance|units — HAPI FHIR R4 requires all four components.
    // Pipes must be %-encoded (%7C) because HAPI's servlet rejects bare | in query strings.
    let url = format!("/{resource_type}?{param_name}=-25.0%7C133.0%7C3000%7Ckm");

    TestCase {
        name: format!(
            "{}_search_{}_near_10km",
            resource_type.to_lowercase(),
            param_name.replace('-', "_"),
        ),
        kind: TestCaseKind::SearchNear {
            param_name: param_name.to_string(),
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

/// Build a composite search param test.
///
/// Composite search params combine two values with a `$` separator.
/// Uses `expected_status: 0` since composite search is complex and
/// may not be fully supported by all servers.
pub(crate) fn build_search_composite_test(
    resource_type: &str,
    param_name: &str,
    profile_url: &Option<String>,
) -> TestCase {
    let url = format!("/{resource_type}?{param_name}=test-value1$test-value2&_id={{id}}");

    TestCase {
        name: format!(
            "{}_search_{}_composite",
            resource_type.to_lowercase(),
            param_name.replace('-', "_"),
        ),
        kind: TestCaseKind::SearchComposite {
            param_name: param_name.to_string(),
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
            expected_status: 0, // 0 = accept any status (composite is complex)
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build a system-level search test (`GET /?_type=...`).
/// System-level search searches across multiple resource types.
pub(crate) fn build_system_search_test() -> TestCase {
    TestCase {
        name: "system_search".to_string(),
        kind: TestCaseKind::SearchSingle {
            param_name: "_type".to_string(),
            param_type: "string".to_string(),
        },
        interaction: Interaction::SearchType,
        resource_type: String::new(),
        profile_url: None,
        request: HttpRequest {
            method: "GET".to_string(),
            url: "/?_type=Patient,Observation&_count=1".to_string(),
            headers: HashMap::new(),
            body: None,
        },
        validation: ValidationSpec {
            expected_status: 0,
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
    fn build_search_single_test_produces_correct_url_and_kind() {
        let test = build_search_single_test(
            "Patient",
            "name",
            "string",
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("?name="),
            "URL should contain ?name=, got {}",
            test.request.url
        );
        assert!(
            test.request.url.contains("&_id={id}"),
            "URL should contain &_id={{id}}, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_name");
        assert!(
            matches!(test.kind, TestCaseKind::SearchSingle { ref param_name, .. } if param_name == "name")
        );
        assert_eq!(test.interaction, Interaction::SearchType);
        assert_eq!(test.validation.expected_status, 200);
        assert_eq!(
            test.validation
                .response_assertion
                .as_ref()
                .unwrap()
                .bundle_type,
            Some("searchset".to_string())
        );
    }

    #[test]
    fn build_search_single_test_with_profile() {
        let profile = Some("http://example.org/Profile".to_string());
        let test = build_search_single_test(
            "Patient",
            "birthdate",
            "date",
            &profile,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.profile_url, profile);
        assert!(test.request.url.contains("?birthdate="));
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchSingle { ref param_name, ref param_type }
                if param_name == "birthdate" && param_type == "date"
        ));
    }

    #[test]
    fn build_search_modifier_test_exact() {
        let test = build_search_modifier_test(
            "Patient",
            "name",
            "string",
            &SearchModifier::Exact,
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("name:exact="),
            "URL should contain name:exact=, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_name_exact");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchModifier { ref param_name, ref modifier }
                if param_name == "name" && modifier == &SearchModifier::Exact
        ));
    }

    #[test]
    fn build_search_modifier_test_missing() {
        let test = build_search_modifier_test(
            "Patient",
            "birthdate",
            "date",
            &SearchModifier::Missing,
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert_eq!(test.request.url, "/Patient?birthdate:missing=true");
        assert_eq!(test.name, "patient_search_birthdate_missing");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchModifier { ref modifier, .. }
                if modifier == &SearchModifier::Missing
        ));
    }

    #[test]
    fn build_search_prefix_test_eq() {
        let test = build_search_prefix_test(
            "Patient",
            "birthdate",
            "date",
            &SearchPrefix::Eq,
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("birthdate=eq"),
            "URL should contain birthdate=eq, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_birthdate_eq");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchPrefix { ref param_name, ref prefix }
                if param_name == "birthdate" && prefix == &SearchPrefix::Eq
        ));
    }

    #[test]
    fn build_search_prefix_test_gt() {
        let test = build_search_prefix_test(
            "Observation",
            "value",
            "quantity",
            &SearchPrefix::Gt,
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            test.request.url.contains("value=gt"),
            "URL should contain value=gt, got {}",
            test.request.url
        );
        assert_eq!(test.name, "observation_search_value_gt");
    }

    #[test]
    fn build_search_near_test_produces_coordinate_url() {
        let test = build_search_near_test("Location", "near", &None);
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("near="),
            "URL should contain near=, got {}",
            test.request.url
        );
        assert!(
            test.request.url.contains("%7C"),
            "URL should contain %7C (pipe encoding), got {}",
            test.request.url
        );
        assert_eq!(test.name, "location_search_near_near_10km");
        assert!(
            matches!(test.kind, TestCaseKind::SearchNear { ref param_name } if param_name == "near")
        );
        assert_eq!(test.validation.expected_status, 200);
    }

    #[test]
    fn sample_value_returns_correct_defaults() {
        assert_eq!(sample_value("string"), "test-value");
        assert_eq!(sample_value("token"), "test-code");
        assert_eq!(sample_value("reference"), "Patient/test-id");
        assert_eq!(sample_value("number"), "1");
        assert_eq!(sample_value("date"), "2024-01-01");
        assert_eq!(sample_value("dateTime"), "2024-01-01T00:00:00Z");
        assert_eq!(sample_value("uri"), "http://example.org");
        assert_eq!(
            sample_value("quantity"),
            "5.0||http://unitsofmeasure.org|kg"
        );
        assert!(
            sample_value("special").contains("%7C"),
            "special should contain pipe encoding"
        );
        assert_eq!(sample_value("unknown"), "test-value");
    }

    #[test]
    fn resolve_param_value_falls_back_to_sample_value() {
        let empty_fv = HashMap::new();
        let empty_ids = HashMap::new();
        let value = resolve_param_value("Patient", "name", "string", &empty_fv, &empty_ids);
        assert_eq!(value, "test-value");
    }

    #[test]
    fn resolve_param_value_uses_field_values() {
        let mut field_values = HashMap::new();
        let mut patient_values = HashMap::new();
        patient_values.insert("Patient.name".to_string(), "John".to_string());
        field_values.insert("Patient".to_string(), patient_values);
        let empty_ids = HashMap::new();
        let value = resolve_param_value("Patient", "name", "string", &field_values, &empty_ids);
        assert_eq!(value, "John");
    }

    #[test]
    fn build_search_composite_test_produces_correct_url() {
        let test = build_search_composite_test("Patient", "name", &None);
        assert_eq!(test.request.method, "GET");
        assert!(
            test.request.url.contains("name=test-value1$test-value2"),
            "URL should contain composite value, got {}",
            test.request.url
        );
        assert_eq!(test.name, "patient_search_name_composite");
        assert!(matches!(
            test.kind,
            TestCaseKind::SearchComposite { ref param_name } if param_name == "name"
        ));
        assert_eq!(test.validation.expected_status, 0);
    }
}
