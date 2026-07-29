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
