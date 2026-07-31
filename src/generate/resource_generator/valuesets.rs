use std::collections::HashMap;

/// Build a map from ValueSet URL → the system URL used by that ValueSet.
///
/// Extracts the system from `ValueSet.compose.include[].system` (preferred)
/// or falls back to `ValueSet.expansion.contains[].system`.
pub fn build_value_set_system_map(
    raw_resources: &HashMap<String, serde_json::Value>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for resource in raw_resources.values() {
        if resource.get("resourceType").and_then(|v| v.as_str()) != Some("ValueSet") {
            continue;
        }

        let Some(url) = resource.get("url").and_then(|v| v.as_str()) else {
            continue;
        };

        if let Some(system) = extract_valueset_system(resource) {
            map.insert(url.to_string(), system);
        }
    }

    map
}

/// Build a map from CodeSystem URL → first concept code in that system.
///
/// Used as a fallback when generating CodeableConcept values for elements with
/// a required binding: if no fixedCoding is specified, we pick the first valid
/// code from the bound CodeSystem.
pub fn build_code_system_first_code_map(
    raw_resources: &HashMap<String, serde_json::Value>,
) -> HashMap<String, (String, Option<String>)> {
    let mut map: HashMap<String, (String, Option<String>)> = HashMap::new();

    for resource in raw_resources.values() {
        match resource.get("resourceType").and_then(|v| v.as_str()) {
            Some("CodeSystem") => {
                let Some(url) = resource.get("url").and_then(|v| v.as_str()) else {
                    continue;
                };
                let first_concept = resource
                    .get("concept")
                    .and_then(|c| c.as_array())
                    .and_then(|a| a.first());
                if let Some(concept) = first_concept {
                    let code = concept
                        .get("code")
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string());
                    let display = concept
                        .get("display")
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string());
                    if let Some(code) = code {
                        map.insert(url.to_string(), (code, display));
                    }
                }
            }
            _ => continue,
        }
    }

    map
}

/// Look up the system URL bound to an element via its binding.valueSet reference.
pub fn bound_system_for_element(
    element: &crate::model::ElementDefinition,
    value_set_systems: &HashMap<String, String>,
) -> Option<String> {
    let binding = element.binding.as_ref()?;
    let value_set_url = binding.value_set.as_ref()?.split('|').next()?;
    value_set_systems.get(value_set_url).cloned()
}

fn extract_valueset_system(resource: &serde_json::Value) -> Option<String> {
    // Prefer compose.include.system because it is canonical terminology metadata.
    if let Some(include) = resource
        .get("compose")
        .and_then(|v| v.get("include"))
        .and_then(|v| v.as_array())
    {
        for item in include {
            if let Some(system) = item.get("system").and_then(|v| v.as_str()) {
                return Some(system.to_string());
            }
        }
    }

    // Fallback to expansion.contains[*].system when compose is unavailable.
    if let Some(contains) = resource
        .get("expansion")
        .and_then(|v| v.get("contains"))
        .and_then(|v| v.as_array())
    {
        for item in contains {
            if let Some(system) = item.get("system").and_then(|v| v.as_str()) {
                return Some(system.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── build_value_set_system_map ──────────────────────────────────────

    #[test]
    fn test_build_value_set_system_map_from_compose() {
        let mut resources = HashMap::new();
        resources.insert(
            "vs1".to_string(),
            json!({
                "resourceType": "ValueSet",
                "url": "http://example.org/ValueSet/test",
                "compose": {
                    "include": [{"system": "http://example.org/CodeSystem/test"}]
                }
            }),
        );
        let map = build_value_set_system_map(&resources);
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get("http://example.org/ValueSet/test"),
            Some(&"http://example.org/CodeSystem/test".to_string())
        );
    }

    #[test]
    fn test_build_value_set_system_map_from_expansion() {
        let mut resources = HashMap::new();
        resources.insert(
            "vs1".to_string(),
            json!({
                "resourceType": "ValueSet",
                "url": "http://example.org/ValueSet/test",
                "expansion": {
                    "contains": [{"system": "http://example.org/CodeSystem/expanded"}]
                }
            }),
        );
        let map = build_value_set_system_map(&resources);
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get("http://example.org/ValueSet/test"),
            Some(&"http://example.org/CodeSystem/expanded".to_string())
        );
    }

    #[test]
    fn test_build_value_set_system_map_skips_non_valuesets() {
        let mut resources = HashMap::new();
        resources.insert(
            "cs1".to_string(),
            json!({
                "resourceType": "CodeSystem",
                "url": "http://example.org/CodeSystem/test",
            }),
        );
        let map = build_value_set_system_map(&resources);
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_value_set_system_map_skips_no_url() {
        let mut resources = HashMap::new();
        resources.insert(
            "vs1".to_string(),
            json!({
                "resourceType": "ValueSet",
                "compose": {
                    "include": [{"system": "http://example.org/CodeSystem/test"}]
                }
            }),
        );
        let map = build_value_set_system_map(&resources);
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_value_set_system_map_no_system() {
        let mut resources = HashMap::new();
        resources.insert(
            "vs1".to_string(),
            json!({
                "resourceType": "ValueSet",
                "url": "http://example.org/ValueSet/test",
            }),
        );
        let map = build_value_set_system_map(&resources);
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_value_set_system_map_multiple() {
        let mut resources = HashMap::new();
        resources.insert(
            "vs1".to_string(),
            json!({
                "resourceType": "ValueSet",
                "url": "http://example.org/ValueSet/vs1",
                "compose": {
                    "include": [{"system": "http://example.org/CodeSystem/cs1"}]
                }
            }),
        );
        resources.insert(
            "vs2".to_string(),
            json!({
                "resourceType": "ValueSet",
                "url": "http://example.org/ValueSet/vs2",
                "compose": {
                    "include": [{"system": "http://example.org/CodeSystem/cs2"}]
                }
            }),
        );
        let map = build_value_set_system_map(&resources);
        assert_eq!(map.len(), 2);
    }

    // ── build_code_system_first_code_map ───────────────────────────────

    #[test]
    fn test_build_code_system_first_code_map_with_display() {
        let mut resources = HashMap::new();
        resources.insert(
            "cs1".to_string(),
            json!({
                "resourceType": "CodeSystem",
                "url": "http://example.org/CodeSystem/test",
                "concept": [
                    {"code": "active", "display": "Active"},
                    {"code": "inactive", "display": "Inactive"}
                ]
            }),
        );
        let map = build_code_system_first_code_map(&resources);
        assert_eq!(map.len(), 1);
        let (code, display) = map.get("http://example.org/CodeSystem/test").unwrap();
        assert_eq!(code, "active");
        assert_eq!(display.as_deref(), Some("Active"));
    }

    #[test]
    fn test_build_code_system_first_code_map_no_display() {
        let mut resources = HashMap::new();
        resources.insert(
            "cs1".to_string(),
            json!({
                "resourceType": "CodeSystem",
                "url": "http://example.org/CodeSystem/test",
                "concept": [{"code": "active"}]
            }),
        );
        let map = build_code_system_first_code_map(&resources);
        let (code, display) = map.get("http://example.org/CodeSystem/test").unwrap();
        assert_eq!(code, "active");
        assert!(display.is_none());
    }

    #[test]
    fn test_build_code_system_first_code_map_skips_non_codesystems() {
        let mut resources = HashMap::new();
        resources.insert(
            "vs1".to_string(),
            json!({
                "resourceType": "ValueSet",
                "url": "http://example.org/ValueSet/test",
            }),
        );
        let map = build_code_system_first_code_map(&resources);
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_code_system_first_code_map_skips_no_url() {
        let mut resources = HashMap::new();
        resources.insert(
            "cs1".to_string(),
            json!({
                "resourceType": "CodeSystem",
                "concept": [{"code": "active"}]
            }),
        );
        let map = build_code_system_first_code_map(&resources);
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_code_system_first_code_map_empty_concepts() {
        let mut resources = HashMap::new();
        resources.insert(
            "cs1".to_string(),
            json!({
                "resourceType": "CodeSystem",
                "url": "http://example.org/CodeSystem/test",
                "concept": []
            }),
        );
        let map = build_code_system_first_code_map(&resources);
        assert!(map.is_empty());
    }

    // ── bound_system_for_element ────────────────────────────────────────

    #[test]
    fn test_bound_system_for_element_found() {
        let mut value_set_systems = HashMap::new();
        value_set_systems.insert(
            "http://hl7.org/fhir/ValueSet/administrative-gender".to_string(),
            "http://hl7.org/fhir/administrative-gender".to_string(),
        );
        let element = crate::model::ElementDefinition {
            id: "Patient.gender".to_string(),
            path: "Patient.gender".to_string(),
            min: Some(0),
            max: Some("1".to_string()),
            type_: vec![],
            fixed_string: None,
            fixed_uri: None,
            fixed_code: None,
            fixed_boolean: None,
            fixed_integer: None,
            fixed_decimal: None,
            pattern_string: None,
            pattern_uri: None,
            pattern_code: None,
            pattern_boolean: None,
            must_support: false,
            short: None,
            definition: None,
            binding: Some(crate::model::ElementBinding {
                strength: "required".to_string(),
                value_set: Some("http://hl7.org/fhir/ValueSet/administrative-gender".to_string()),
                description: None,
            }),
            content_reference: None,
            fixed_quantity: None,
            pattern_quantity: None,
            fixed_coding: None,
            pattern_coding: None,
            fixed_codeable_concept: None,
            pattern_codeable_concept: None,
            constraint: vec![],
            is_modifier: false,
            is_summary: false,
            slice_name: None,
            slicing: None,
        };
        let result = bound_system_for_element(&element, &value_set_systems);
        assert_eq!(
            result,
            Some("http://hl7.org/fhir/administrative-gender".to_string())
        );
    }

    #[test]
    fn test_bound_system_for_element_no_binding() {
        let element = crate::model::ElementDefinition {
            id: "Patient.name".to_string(),
            path: "Patient.name".to_string(),
            min: Some(0),
            max: Some("1".to_string()),
            type_: vec![],
            fixed_string: None,
            fixed_uri: None,
            fixed_code: None,
            fixed_boolean: None,
            fixed_integer: None,
            fixed_decimal: None,
            pattern_string: None,
            pattern_uri: None,
            pattern_code: None,
            pattern_boolean: None,
            must_support: false,
            short: None,
            definition: None,
            binding: None,
            content_reference: None,
            fixed_quantity: None,
            pattern_quantity: None,
            fixed_coding: None,
            pattern_coding: None,
            fixed_codeable_concept: None,
            pattern_codeable_concept: None,
            constraint: vec![],
            is_modifier: false,
            is_summary: false,
            slice_name: None,
            slicing: None,
        };
        let result = bound_system_for_element(&element, &HashMap::new());
        assert!(result.is_none());
    }

    #[test]
    fn test_bound_system_for_element_no_value_set() {
        let element = crate::model::ElementDefinition {
            id: "Patient.gender".to_string(),
            path: "Patient.gender".to_string(),
            min: Some(0),
            max: Some("1".to_string()),
            type_: vec![],
            fixed_string: None,
            fixed_uri: None,
            fixed_code: None,
            fixed_boolean: None,
            fixed_integer: None,
            fixed_decimal: None,
            pattern_string: None,
            pattern_uri: None,
            pattern_code: None,
            pattern_boolean: None,
            must_support: false,
            short: None,
            definition: None,
            binding: Some(crate::model::ElementBinding {
                strength: "required".to_string(),
                value_set: None,
                description: None,
            }),
            content_reference: None,
            fixed_quantity: None,
            pattern_quantity: None,
            fixed_coding: None,
            pattern_coding: None,
            fixed_codeable_concept: None,
            pattern_codeable_concept: None,
            constraint: vec![],
            is_modifier: false,
            is_summary: false,
            slice_name: None,
            slicing: None,
        };
        let result = bound_system_for_element(&element, &HashMap::new());
        assert!(result.is_none());
    }

    #[test]
    fn test_bound_system_for_element_strips_version() {
        let mut value_set_systems = HashMap::new();
        value_set_systems.insert(
            "http://hl7.org/fhir/ValueSet/administrative-gender".to_string(),
            "http://hl7.org/fhir/administrative-gender".to_string(),
        );
        let element = crate::model::ElementDefinition {
            id: "Patient.gender".to_string(),
            path: "Patient.gender".to_string(),
            min: Some(0),
            max: Some("1".to_string()),
            type_: vec![],
            fixed_string: None,
            fixed_uri: None,
            fixed_code: None,
            fixed_boolean: None,
            fixed_integer: None,
            fixed_decimal: None,
            pattern_string: None,
            pattern_uri: None,
            pattern_code: None,
            pattern_boolean: None,
            must_support: false,
            short: None,
            definition: None,
            binding: Some(crate::model::ElementBinding {
                strength: "required".to_string(),
                value_set: Some(
                    "http://hl7.org/fhir/ValueSet/administrative-gender|4.0.1".to_string(),
                ),
                description: None,
            }),
            content_reference: None,
            fixed_quantity: None,
            pattern_quantity: None,
            fixed_coding: None,
            pattern_coding: None,
            fixed_codeable_concept: None,
            pattern_codeable_concept: None,
            constraint: vec![],
            is_modifier: false,
            is_summary: false,
            slice_name: None,
            slicing: None,
        };
        let result = bound_system_for_element(&element, &value_set_systems);
        assert_eq!(
            result,
            Some("http://hl7.org/fhir/administrative-gender".to_string())
        );
    }

    // ── extract_valueset_system ──────────────────────────────────────────

    #[test]
    fn test_extract_valueset_system_from_compose() {
        let resource = json!({
            "compose": {
                "include": [{"system": "http://example.org/CodeSystem/test"}]
            }
        });
        assert_eq!(
            extract_valueset_system(&resource),
            Some("http://example.org/CodeSystem/test".to_string())
        );
    }

    #[test]
    fn test_extract_valueset_system_from_expansion() {
        let resource = json!({
            "expansion": {
                "contains": [{"system": "http://example.org/CodeSystem/expanded"}]
            }
        });
        assert_eq!(
            extract_valueset_system(&resource),
            Some("http://example.org/CodeSystem/expanded".to_string())
        );
    }

    #[test]
    fn test_extract_valueset_system_compose_preferred() {
        let resource = json!({
            "compose": {
                "include": [{"system": "http://example.org/CodeSystem/compose"}]
            },
            "expansion": {
                "contains": [{"system": "http://example.org/CodeSystem/expansion"}]
            }
        });
        // compose is preferred over expansion
        assert_eq!(
            extract_valueset_system(&resource),
            Some("http://example.org/CodeSystem/compose".to_string())
        );
    }

    #[test]
    fn test_extract_valueset_system_none() {
        let resource = json!({});
        assert_eq!(extract_valueset_system(&resource), None);
    }
}
