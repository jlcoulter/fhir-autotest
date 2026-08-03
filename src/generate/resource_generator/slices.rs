use crate::generate::resource_generator::types::{
    capitalize_fhir_type, find_human_name_use, find_identifier_system, find_identifier_type,
    find_slice_system, generate_typed_value, is_generic_identifier_system, resolve_slice_type_code,
};
use crate::model::*;
use anyhow::Result;
use std::collections::HashMap;

/// Generate slice-aware elements for fields that have required slices.
///
/// For each field with slicing, check if any slice has min > 0.
/// If so, generate an additional element that applies the slice's
/// pattern values (e.g., patternUri on system for identifier slices).
///
/// If no slice is individually required but the field itself is required
/// and has slicing, replace the generic value with one matching the
/// first slice's discriminator pattern so the resource passes validation.
pub fn populate_required_slices(
    resource: &mut serde_json::Value,
    elements: &[ElementDefinition],
    resource_type: &str,
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
) -> Result<()> {
    // Collect all fields that have slice definitions
    let mut slice_fields: HashMap<String, Vec<&ElementDefinition>> = HashMap::new();

    for element in elements {
        if let Some(ref _slice_name) = element.slice_name {
            let field_name = match super::types::get_field_name(&element.path, resource_type) {
                Some(name) => name,
                None => continue,
            };
            slice_fields.entry(field_name).or_default().push(element);
        }
    }

    for (field_name, slices) in &slice_fields {
        // Find the discriminator path from the slicing element
        let discriminator_path: Option<String> = elements
            .iter()
            .find(|e| {
                let fname = super::types::get_field_name(&e.path, resource_type);
                fname.as_deref() == Some(field_name) && e.slicing.is_some()
            })
            .and_then(|e| e.slicing.as_ref())
            .and_then(|s| s.discriminator.first().map(|d| d.path.clone()));

        // Check if any slice has min > 0 (required slice)
        let required_slices: Vec<&&ElementDefinition> =
            slices.iter().filter(|s| s.min.unwrap_or(0) > 0).collect();

        // Get the field value — if the field doesn't exist yet,
        // it will be handled by the main populate_required_fields loop
        let field_value = match resource.get(field_name) {
            Some(val) => val.clone(),
            None => continue,
        };

        let mut slice_values = Vec::new();

        // Start with the existing value(s)
        if let Some(arr) = field_value.as_array() {
            slice_values.extend(arr.iter().cloned());
        } else {
            slice_values.push(field_value);
        }

        if !required_slices.is_empty() {
            // Replace the first generic value with a value matching the first
            // required slice, then add values for remaining required slices.
            // This avoids having a non-slice-matching generic value in the array.
            if let Some(first_slice) = required_slices.first()
                && let Some(val) = generate_slice_value(
                    first_slice,
                    resource_type,
                    discriminator_path.as_deref(),
                    all_profiles,
                    elements,
                    value_set_systems,
                )
            {
                // Replace the first generic value
                if slice_values.is_empty() {
                    slice_values.push(val);
                } else {
                    slice_values[0] = val;
                }
            }
            // Add values for remaining required slices
            for slice in required_slices.iter().skip(1) {
                if let Some(val) = generate_slice_value(
                    slice,
                    resource_type,
                    discriminator_path.as_deref(),
                    all_profiles,
                    elements,
                    value_set_systems,
                ) {
                    slice_values.push(val);
                }
            }
        } else if slice_values.len() == 1 {
            // No slice is individually required, but the field has slicing.
            // Replace the generic value with one matching the first slice's
            // discriminator pattern so the resource passes validation.
            if let Some(val) = generate_slice_value(
                slices[0],
                resource_type,
                discriminator_path.as_deref(),
                all_profiles,
                elements,
                value_set_systems,
            ) {
                slice_values = vec![val];
            }
        }

        resource[field_name] = serde_json::json!(slice_values);
    }

    Ok(())
}

/// Generate a value that matches a slice's discriminator pattern.
///
/// Examines the slice element for pattern values (patternUri, patternCode, etc.)
/// and creates a value that satisfies the discriminator.
///
/// If no pattern values are present but the slice has a type profile reference,
/// scans the full elements list for the profiled type's sub-elements to find
/// fixedUri/patternCodeableConcept values that satisfy the discriminator.
fn generate_slice_value(
    slice: &ElementDefinition,
    _resource_type: &str,
    discriminator_path: Option<&str>,
    all_profiles: &[StructureDefinition],
    elements: &[ElementDefinition],
    value_set_systems: &HashMap<String, String>,
) -> Option<serde_json::Value> {
    // Determine the base type from the slice's own type, or from the
    // unsliced base element when the slice relies on inherited typing.
    let type_code = resolve_slice_type_code(slice, elements)?;

    // Start with a base value for the type
    let mut value = generate_typed_value(&type_code, &[], slice, value_set_systems);

    // HumanName slice support
    if type_code == "HumanName"
        && let Some(slice_name) = &slice.slice_name
        && let Some(use_code) = find_human_name_use(slice_name, elements)
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("use".to_string(), serde_json::json!(use_code));
    }

    // Apply pattern values from the slice definition
    if let Some(val) = &slice.pattern_uri
        && let Some(obj) = value.as_object_mut()
    {
        if type_code == "Identifier" {
            obj.insert("system".to_string(), serde_json::json!(val));
        } else {
            obj.insert("value".to_string(), serde_json::json!(val));
        }
    }

    if let Some(val) = &slice.pattern_code
        && let Some(obj) = value.as_object_mut()
    {
        match type_code.as_str() {
            "HumanName" => {
                obj.insert("use".to_string(), serde_json::json!(val));
            }
            "Address" => {
                obj.insert("type".to_string(), serde_json::json!(val));
            }
            _ => {}
        }
    }

    if let Some(val) = &slice.pattern_string
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("value".to_string(), serde_json::json!(val));
    }

    if let Some(val) = &slice.pattern_coding
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("coding".to_string(), val.clone());
    }

    if let Some(val) = &slice.pattern_codeable_concept
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("coding".to_string(), val.clone());
    }

    // Identifier slice handling
    if type_code == "Identifier"
        && let Some(obj) = value.as_object_mut()
    {
        let profile_url = slice
            .type_
            .first()
            .and_then(|t| {
                t.profile
                    .first()
                    .or_else(|| t.target_profile.first())
                    .map(|s| s.as_str())
            })
            .unwrap_or("");

        if let Some(system) = find_identifier_system(profile_url, all_profiles)
            && (!obj.contains_key("system")
                || obj
                    .get("system")
                    .and_then(|v| v.as_str())
                    .is_some_and(is_generic_identifier_system))
        {
            obj.insert("system".to_string(), serde_json::json!(system));
        }

        // Some IG slices define Identifier.system at nested paths without
        // repeating a discriminator. Use slice-specific nested constraints
        // as a direct source of truth when available.
        if let Some(slice_name) = &slice.slice_name
            && let Some(system) = find_slice_system(slice_name, elements)
        {
            obj.insert("system".to_string(), serde_json::json!(system));
        }

        if let Some(identifier_type) = find_identifier_type(profile_url, all_profiles)
            && !obj.contains_key("type")
        {
            obj.insert("type".to_string(), identifier_type);
        }

        match discriminator_path {
            Some("system") => {
                if let Some(slice_name) = &slice.slice_name
                    && let Some(system) = find_slice_system(slice_name, elements)
                {
                    obj.insert("system".to_string(), serde_json::json!(system));
                    return Some(value);
                }
            }

            Some(path) if path.starts_with("type") && !obj.contains_key("type") => {
                if let Some(identifier_type) = find_identifier_type(profile_url, all_profiles) {
                    obj.insert("type".to_string(), identifier_type);
                }

                if !obj.contains_key("type") {
                    obj.insert(
                        "type".to_string(),
                        serde_json::json!({
                            "coding": [{
                                "system": "http://terminology.hl7.org/CodeSystem/v2-0203",
                                "code": "XX"
                            }]
                        }),
                    );
                }
            }

            _ => {}
        }
    }

    Some(value)
}

/// Apply slice values for a given field path, wrapping the result in an array.
pub fn apply_slices_for_path(
    value: serde_json::Value,
    field_path: &str,
    elements: &[ElementDefinition],
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
) -> serde_json::Value {
    let slices: Vec<&ElementDefinition> = elements
        .iter()
        .filter(|e| e.path == field_path && e.slice_name.is_some())
        .collect();

    if slices.is_empty() {
        return value;
    }

    let discriminator_path = elements
        .iter()
        .find(|e| e.path == field_path && e.slicing.is_some())
        .and_then(|e| e.slicing.as_ref())
        .and_then(|s| s.discriminator.first().map(|d| d.path.clone()));

    let required_slices: Vec<&&ElementDefinition> =
        slices.iter().filter(|s| s.min.unwrap_or(0) > 0).collect();

    let mut slice_values = Vec::new();
    if let Some(arr) = value.as_array() {
        slice_values.extend(arr.iter().cloned());
    } else {
        slice_values.push(value);
    }

    if !required_slices.is_empty() {
        if let Some(first_slice) = required_slices.first()
            && let Some(v) = generate_slice_value(
                first_slice,
                "",
                discriminator_path.as_deref(),
                all_profiles,
                elements,
                value_set_systems,
            )
        {
            if slice_values.is_empty() {
                slice_values.push(v);
            } else {
                slice_values[0] = v;
            }
        }

        for slice in required_slices.iter().skip(1) {
            if let Some(v) = generate_slice_value(
                slice,
                "",
                discriminator_path.as_deref(),
                all_profiles,
                elements,
                value_set_systems,
            ) {
                slice_values.push(v);
            }
        }
    } else if let Some(v) = generate_slice_value(
        slices[0],
        "",
        discriminator_path.as_deref(),
        all_profiles,
        elements,
        value_set_systems,
    ) {
        slice_values = vec![v];
    }

    serde_json::json!(slice_values)
}

/// Populate extension slices defined by a profile.
///
/// Profiles can define slices on the `extension` field (e.g.
/// `HealthcareService.extension:active-period`). Each slice references a
/// StructureDefinition of type `Extension` that defines the extension URL
/// (via `fixedUri` on `Extension.url`) and either a direct value type (via
/// `Extension.value[x]`) or nested sub-extensions (when `value[x]` is
/// prohibited with `max=0`).
///
/// This function scans the profile's snapshot for extension slice elements,
/// looks up the referenced extension definitions, and generates valid
/// extension entries with the correct URL and a generated value.
///
/// Only top-level extension slices (e.g. `ResourceType.extension:sliceName`)
/// are handled — nested extensions on sub-fields are not covered here.
pub fn populate_extension_slices(
    resource: &mut serde_json::Value,
    elements: &[ElementDefinition],
    resource_type: &str,
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
) {
    // Collect extension slice elements (sliceName set on ResourceType.extension)
    let extension_slices: Vec<&ElementDefinition> = elements
        .iter()
        .filter(|e| {
            e.slice_name.is_some()
                && e.path == format!("{}.extension", resource_type)
                && !e.type_.is_empty()
                && e.type_[0].code == "Extension"
        })
        .collect();

    if extension_slices.is_empty() {
        return;
    }

    // Build a URL → StructureDefinition map for quick lookup of extension definitions
    let ext_def_map: HashMap<&str, &StructureDefinition> = all_profiles
        .iter()
        .filter(|p| p.base_type == "Extension")
        .map(|p| (p.url.as_str(), p))
        .collect();

    let mut extensions: Vec<serde_json::Value> = Vec::new();

    for slice in &extension_slices {
        // Get the profile URL from the slice's type reference
        let profile_url = slice.type_[0]
            .profile
            .first()
            .map(|s| s.split('|').next().unwrap_or(s))
            .or_else(|| {
                slice.type_[0]
                    .target_profile
                    .first()
                    .map(|s| s.split('|').next().unwrap_or(s))
            });

        let Some(profile_url) = profile_url else {
            continue;
        };

        // Look up the extension definition
        let Some(ext_def) = ext_def_map.get(profile_url) else {
            continue;
        };

        // Extract the fixed URL from the extension definition's snapshot
        let ext_elements = match &ext_def.snapshot {
            Some(s) => &s.element,
            None => continue,
        };

        // Find the fixedUri on Extension.url
        let ext_url = ext_elements
            .iter()
            .find(|e| e.id == "Extension.url" || e.path == "Extension.url")
            .and_then(|e| e.fixed_uri.as_deref())
            .unwrap_or(profile_url);

        // Find the value type from Extension.value[x]
        let value_x_elem = ext_elements
            .iter()
            .find(|e| e.id == "Extension.value[x]" || e.path == "Extension.value[x]");

        // Determine if this is a complex extension (value[x] prohibited, uses nested extensions)
        let is_complex = value_x_elem
            .and_then(|e| e.max.as_deref())
            .map(|m| m == "0")
            .unwrap_or(false);

        let mut ext_entry = serde_json::json!({
            "url": ext_url
        });

        if is_complex {
            // Complex extension: generate nested sub-extensions from the extension
            // definition's Extension.extension slices (e.g. suppressedBy, includeSelf).
            let sub_ext_slices: Vec<&ElementDefinition> = ext_elements
                .iter()
                .filter(|e| {
                    e.slice_name.is_some()
                        && e.path == "Extension.extension"
                        && !e.type_.is_empty()
                        && e.type_[0].code == "Extension"
                })
                .collect();

            let mut sub_extensions: Vec<serde_json::Value> = Vec::new();

            for sub_slice in &sub_ext_slices {
                let min = sub_slice.min.unwrap_or(0);
                if min == 0 {
                    continue; // Optional sub-extension, skip
                }

                // Find the fixed URL for this sub-extension (e.g. "suppressedBy")
                let sub_url = ext_elements
                    .iter()
                    .find(|e| e.id == format!("{}.url", sub_slice.id))
                    .and_then(|e| e.fixed_uri.as_deref())
                    .unwrap_or("");

                // Find the value[x] element for this sub-extension to get type + binding
                let sub_value_elem = ext_elements
                    .iter()
                    .find(|e| e.id == format!("{}.value[x]", sub_slice.id));

                let sub_value_type = sub_value_elem
                    .and_then(|e| e.type_.first())
                    .map(|t| t.code.as_str());

                let mut sub_entry = serde_json::json!({
                    "url": sub_url
                });

                if let (Some(vt), Some(value_elem)) = (sub_value_type, sub_value_elem) {
                    let mut value = generate_typed_value(vt, &[], value_elem, value_set_systems);

                    // If the value type is CodeableConcept, the profile may constrain the
                    // coding with a fixedCoding on the value[x].coding child element.
                    // Check the profile's own elements (not the base extension definition's)
                    // to find any fixed/pattern coding that must be used.
                    if vt == "CodeableConcept" {
                        let sub_name = sub_slice.slice_name.as_deref().unwrap_or("");
                        let slice_name_str = slice.slice_name.as_deref().unwrap_or("");
                        let fixed_coding = elements
                            .iter()
                            .find(|e| {
                                e.id.contains(&format!(":{}", slice_name_str))
                                    && e.id.contains(&format!(":{}", sub_name))
                                    && (e.id.ends_with(".value[x].coding")
                                        || e.path.ends_with("value[x].coding"))
                            })
                            .and_then(|e| e.fixed_coding.as_ref().or(e.pattern_coding.as_ref()));

                        if let Some(coding) = fixed_coding {
                            value = serde_json::json!({
                                "coding": [coding],
                                "text": coding.get("display").and_then(|d| d.as_str()).unwrap_or("General practice")
                            });
                        }
                    }

                    // FHIR requires PascalCase suffix: valueMarkdown, valueCodeableConcept, etc.
                    let type_name = capitalize_fhir_type(vt);
                    let value_key = format!("value{}", type_name);
                    sub_entry[value_key] = value;
                }

                sub_extensions.push(sub_entry);
            }

            if !sub_extensions.is_empty() {
                ext_entry["extension"] = serde_json::json!(sub_extensions);
            }
        } else {
            // Simple extension: generate a direct value[x] entry
            let value_type = value_x_elem
                .and_then(|e| e.type_.first())
                .map(|t| t.code.as_str());

            if let Some(vt) = value_type {
                let value = generate_typed_value(vt, &[], slice, value_set_systems);
                // FHIR requires PascalCase suffix: valueMarkdown, valueCodeableConcept, etc.
                let type_name = capitalize_fhir_type(vt);
                let value_key = format!("value{}", type_name);
                ext_entry[value_key] = value;
            }
        }

        extensions.push(ext_entry);
    }

    if !extensions.is_empty() {
        resource["extension"] = serde_json::json!(extensions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_element(
        id: &str,
        path: &str,
        slice_name: Option<&str>,
        min: u32,
        type_code: &str,
    ) -> ElementDefinition {
        ElementDefinition {
            id: id.into(),
            path: path.into(),
            slice_name: slice_name.map(|s| s.into()),
            min: Some(min),
            type_: if type_code.is_empty() {
                vec![]
            } else {
                vec![ElementDefinitionType {
                    code: type_code.into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }]
            },
            ..Default::default()
        }
    }

    fn make_element_with_slicing(
        id: &str,
        path: &str,
        slice_name: Option<&str>,
        min: u32,
        type_code: &str,
        discriminator_path: &str,
        discriminator_type: &str,
    ) -> ElementDefinition {
        ElementDefinition {
            id: id.into(),
            path: path.into(),
            slice_name: slice_name.map(|s| s.into()),
            min: Some(min),
            type_: if type_code.is_empty() {
                vec![]
            } else {
                vec![ElementDefinitionType {
                    code: type_code.into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }]
            },
            slicing: Some(ElementSlicing {
                discriminator: vec![SlicingDiscriminator {
                    discriminator_type: discriminator_type.into(),
                    path: discriminator_path.into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // -- populate_required_slices ----------------------------------------

    #[test]
    fn test_populate_required_slices_no_slices() {
        let mut resource = json!({"resourceType": "Patient", "identifier": [{"value": "test"}]});
        let elements = vec![make_element("Patient", "Patient", None, 0, "")];
        let result =
            populate_required_slices(&mut resource, &elements, "Patient", &[], &HashMap::new());
        assert!(result.is_ok());
        assert_eq!(resource["identifier"][0]["value"], "test");
    }

    #[test]
    fn test_populate_required_slices_with_required_slice() {
        let mut resource = json!({"resourceType": "Patient", "identifier": [{"value": "test"}]});
        let elements = vec![
            make_element("Patient", "Patient", None, 0, ""),
            make_element(
                "Patient.identifier",
                "Patient.identifier",
                None,
                0,
                "Identifier",
            ),
            ElementDefinition {
                id: "Patient.identifier:ABN".into(),
                path: "Patient.identifier".into(),
                slice_name: Some("ABN".into()),
                min: Some(1),
                type_: vec![ElementDefinitionType {
                    code: "Identifier".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                slicing: Some(ElementSlicing {
                    discriminator: vec![SlicingDiscriminator {
                        discriminator_type: "value".into(),
                        path: "system".into(),
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];
        let result =
            populate_required_slices(&mut resource, &elements, "Patient", &[], &HashMap::new());
        assert!(result.is_ok());
        // Should have at least one identifier entry
        assert!(!resource["identifier"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_populate_required_slices_no_field_value() {
        let mut resource = json!({"resourceType": "Patient"});
        let elements = vec![
            make_element("Patient", "Patient", None, 0, ""),
            make_element_with_slicing(
                "Patient.identifier:ABN",
                "Patient.identifier",
                Some("ABN"),
                1,
                "Identifier",
                "system",
                "value",
            ),
        ];
        let result =
            populate_required_slices(&mut resource, &elements, "Patient", &[], &HashMap::new());
        assert!(result.is_ok());
        // Field doesn't exist yet, so nothing happens
        assert!(resource.get("identifier").is_none());
    }

    #[test]
    fn test_populate_required_slices_multiple_required() {
        let mut resource = json!({"resourceType": "Patient", "identifier": [{"value": "test"}]});
        let elements = vec![
            make_element("Patient", "Patient", None, 0, ""),
            make_element(
                "Patient.identifier",
                "Patient.identifier",
                None,
                0,
                "Identifier",
            ),
            ElementDefinition {
                id: "Patient.identifier:ABN".into(),
                path: "Patient.identifier".into(),
                slice_name: Some("ABN".into()),
                min: Some(1),
                type_: vec![ElementDefinitionType {
                    code: "Identifier".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                slicing: Some(ElementSlicing {
                    discriminator: vec![SlicingDiscriminator {
                        discriminator_type: "value".into(),
                        path: "system".into(),
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ElementDefinition {
                id: "Patient.identifier:HPII".into(),
                path: "Patient.identifier".into(),
                slice_name: Some("HPII".into()),
                min: Some(1),
                type_: vec![ElementDefinitionType {
                    code: "Identifier".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
        ];
        let result =
            populate_required_slices(&mut resource, &elements, "Patient", &[], &HashMap::new());
        assert!(result.is_ok());
        let identifiers = resource["identifier"].as_array().unwrap();
        // Should have at least 2 entries (first replaced + second added)
        assert!(identifiers.len() >= 2);
    }

    #[test]
    fn test_populate_required_slices_no_required_but_slicing() {
        let mut resource = json!({"resourceType": "Patient", "identifier": [{"value": "test"}]});
        let elements = vec![
            make_element("Patient", "Patient", None, 0, ""),
            make_element_with_slicing(
                "Patient.identifier:ABN",
                "Patient.identifier",
                Some("ABN"),
                0,
                "Identifier",
                "system",
                "value",
            ),
        ];
        let result =
            populate_required_slices(&mut resource, &elements, "Patient", &[], &HashMap::new());
        assert!(result.is_ok());
        // No required slices, but slicing exists -- should replace with slice value
        let identifiers = resource["identifier"].as_array().unwrap();
        assert_eq!(identifiers.len(), 1);
    }

    // -- apply_slices_for_path --------------------------------------------

    #[test]
    fn test_apply_slices_for_path_no_slices() {
        let value = json!({"value": "test"});
        let result = apply_slices_for_path(
            value.clone(),
            "Patient.identifier",
            &[],
            &[],
            &HashMap::new(),
        );
        // When no slices exist, the value is returned as-is (not wrapped in array)
        assert_eq!(result, json!({"value": "test"}));
    }

    #[test]
    fn test_apply_slices_for_path_with_slices() {
        let value = json!({"value": "test"});
        let elements = vec![
            make_element(
                "Patient.identifier:ABN",
                "Patient.identifier",
                Some("ABN"),
                0,
                "Identifier",
            ),
            ElementDefinition {
                id: "Patient.identifier".into(),
                path: "Patient.identifier".into(),
                slicing: Some(ElementSlicing {
                    discriminator: vec![SlicingDiscriminator {
                        discriminator_type: "value".into(),
                        path: "system".into(),
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];
        let result =
            apply_slices_for_path(value, "Patient.identifier", &elements, &[], &HashMap::new());
        let arr = result.as_array().unwrap();
        assert!(!arr.is_empty());
    }

    #[test]
    fn test_apply_slices_for_path_with_required_slice() {
        let value = json!({"value": "test"});
        let elements = vec![make_element_with_slicing(
            "Patient.identifier:ABN",
            "Patient.identifier",
            Some("ABN"),
            1,
            "Identifier",
            "system",
            "value",
        )];
        let result =
            apply_slices_for_path(value, "Patient.identifier", &elements, &[], &HashMap::new());
        let arr = result.as_array().unwrap();
        assert!(!arr.is_empty());
    }

    #[test]
    fn test_apply_slices_for_path_empty_value() {
        let value = json!([]);
        let elements = vec![make_element_with_slicing(
            "Patient.identifier:ABN",
            "Patient.identifier",
            Some("ABN"),
            1,
            "Identifier",
            "system",
            "value",
        )];
        let result =
            apply_slices_for_path(value, "Patient.identifier", &elements, &[], &HashMap::new());
        let arr = result.as_array().unwrap();
        assert!(!arr.is_empty());
    }

    // -- populate_extension_slices ----------------------------------------

    #[test]
    fn test_populate_extension_slices_no_slices() {
        let mut resource = json!({"resourceType": "Patient"});
        populate_extension_slices(&mut resource, &[], "Patient", &[], &HashMap::new());
        assert!(resource.get("extension").is_none());
    }

    #[test]
    fn test_populate_extension_slices_with_slice_but_no_def() {
        let mut resource = json!({"resourceType": "Patient"});
        let elements = vec![ElementDefinition {
            id: "Patient.extension:testExt".into(),
            path: "Patient.extension".into(),
            slice_name: Some("testExt".into()),
            type_: vec![ElementDefinitionType {
                code: "Extension".into(),
                profile: vec!["http://example.org/Extension/TestExt".into()],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        }];
        // No matching extension definition in all_profiles, so nothing should happen
        populate_extension_slices(&mut resource, &elements, "Patient", &[], &HashMap::new());
        assert!(resource.get("extension").is_none());
    }

    #[test]
    fn test_populate_extension_slices_with_extension_def() {
        let mut resource = json!({"resourceType": "Patient"});
        let elements = vec![ElementDefinition {
            id: "Patient.extension:testExt".into(),
            path: "Patient.extension".into(),
            slice_name: Some("testExt".into()),
            type_: vec![ElementDefinitionType {
                code: "Extension".into(),
                profile: vec!["http://example.org/Extension/SimpleExt".into()],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        }];

        // Create an extension definition with a value[x] element
        let ext_def = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Extension/SimpleExt".to_string(),
            name: "SimpleExt".to_string(),
            base_type: "Extension".to_string(),
            kind: "complex-type".to_string(),
            derivation: None,
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Extension".into(),
                        path: "Extension".into(),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.url".into(),
                        path: "Extension.url".into(),
                        fixed_uri: Some("http://example.org/ext/simple".into()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.value[x]".into(),
                        path: "Extension.value[x]".into(),
                        type_: vec![ElementDefinitionType {
                            code: "string".into(),
                            profile: vec![],
                            target_profile: vec![],
                            versioning: None,
                        }],
                        ..Default::default()
                    },
                ],
            }),
            differential: None,
        };

        populate_extension_slices(
            &mut resource,
            &elements,
            "Patient",
            &[ext_def],
            &HashMap::new(),
        );
        assert!(resource.get("extension").is_some());
        let exts = resource["extension"].as_array().unwrap();
        assert_eq!(exts.len(), 1);
        assert_eq!(exts[0]["url"], "http://example.org/ext/simple");
        assert!(exts[0].get("valueString").is_some());
    }

    #[test]
    fn test_populate_extension_slices_complex_extension() {
        let mut resource = json!({"resourceType": "Patient"});
        let elements = vec![ElementDefinition {
            id: "Patient.extension:complexExt".into(),
            path: "Patient.extension".into(),
            slice_name: Some("complexExt".into()),
            type_: vec![ElementDefinitionType {
                code: "Extension".into(),
                profile: vec!["http://example.org/Extension/ComplexExt".into()],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        }];

        // Create a complex extension definition with value[x] max=0 and sub-extensions
        let ext_def = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Extension/ComplexExt".to_string(),
            name: "ComplexExt".to_string(),
            base_type: "Extension".to_string(),
            kind: "complex-type".to_string(),
            derivation: None,
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Extension".into(),
                        path: "Extension".into(),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.url".into(),
                        path: "Extension.url".into(),
                        fixed_uri: Some("http://example.org/ext/complex".into()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.value[x]".into(),
                        path: "Extension.value[x]".into(),
                        max: Some("0".into()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.extension:subExt".into(),
                        path: "Extension.extension".into(),
                        slice_name: Some("subExt".into()),
                        min: Some(1),
                        type_: vec![ElementDefinitionType {
                            code: "Extension".into(),
                            profile: vec![],
                            target_profile: vec![],
                            versioning: None,
                        }],
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.extension:subExt.url".into(),
                        path: "Extension.extension.url".into(),
                        fixed_uri: Some("sub-value".into()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.extension:subExt.value[x]".into(),
                        path: "Extension.extension.value[x]".into(),
                        type_: vec![ElementDefinitionType {
                            code: "string".into(),
                            profile: vec![],
                            target_profile: vec![],
                            versioning: None,
                        }],
                        ..Default::default()
                    },
                ],
            }),
            differential: None,
        };

        populate_extension_slices(
            &mut resource,
            &elements,
            "Patient",
            &[ext_def],
            &HashMap::new(),
        );
        assert!(resource.get("extension").is_some());
        let exts = resource["extension"].as_array().unwrap();
        assert_eq!(exts.len(), 1);
        assert_eq!(exts[0]["url"], "http://example.org/ext/complex");
        // Should have sub-extensions
        assert!(exts[0].get("extension").is_some());
    }

    #[test]
    fn test_populate_extension_slices_skips_optional_sub_extensions() {
        let mut resource = json!({"resourceType": "Patient"});
        let elements = vec![ElementDefinition {
            id: "Patient.extension:complexExt".into(),
            path: "Patient.extension".into(),
            slice_name: Some("complexExt".into()),
            type_: vec![ElementDefinitionType {
                code: "Extension".into(),
                profile: vec!["http://example.org/Extension/ComplexExt".into()],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        }];

        let ext_def = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Extension/ComplexExt".to_string(),
            name: "ComplexExt".to_string(),
            base_type: "Extension".to_string(),
            kind: "complex-type".to_string(),
            derivation: None,
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Extension".into(),
                        path: "Extension".into(),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.url".into(),
                        path: "Extension.url".into(),
                        fixed_uri: Some("http://example.org/ext/complex".into()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.value[x]".into(),
                        path: "Extension.value[x]".into(),
                        max: Some("0".into()),
                        ..Default::default()
                    },
                    // Optional sub-extension (min=0) -- should be skipped
                    ElementDefinition {
                        id: "Extension.extension:optionalSub".into(),
                        path: "Extension.extension".into(),
                        slice_name: Some("optionalSub".into()),
                        min: Some(0),
                        type_: vec![ElementDefinitionType {
                            code: "Extension".into(),
                            profile: vec![],
                            target_profile: vec![],
                            versioning: None,
                        }],
                        ..Default::default()
                    },
                ],
            }),
            differential: None,
        };

        populate_extension_slices(
            &mut resource,
            &elements,
            "Patient",
            &[ext_def],
            &HashMap::new(),
        );
        assert!(resource.get("extension").is_some());
        let exts = resource["extension"].as_array().unwrap();
        // No sub-extensions since the only one is optional
        assert!(exts[0].get("extension").is_none());
    }

    #[test]
    fn test_populate_extension_slices_no_profile_url() {
        let mut resource = json!({"resourceType": "Patient"});
        let elements = vec![ElementDefinition {
            id: "Patient.extension:noProfile".into(),
            path: "Patient.extension".into(),
            slice_name: Some("noProfile".into()),
            type_: vec![ElementDefinitionType {
                code: "Extension".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        }];
        // No profile URL in the type reference -- should skip
        populate_extension_slices(&mut resource, &elements, "Patient", &[], &HashMap::new());
        assert!(resource.get("extension").is_none());
    }

    #[test]
    fn test_populate_extension_slices_no_snapshot() {
        let mut resource = json!({"resourceType": "Patient"});
        let elements = vec![ElementDefinition {
            id: "Patient.extension:noSnapshot".into(),
            path: "Patient.extension".into(),
            slice_name: Some("noSnapshot".into()),
            type_: vec![ElementDefinitionType {
                code: "Extension".into(),
                profile: vec!["http://example.org/Extension/NoSnapshot".into()],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        }];

        let ext_def = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Extension/NoSnapshot".to_string(),
            name: "NoSnapshot".to_string(),
            base_type: "Extension".to_string(),
            kind: "complex-type".to_string(),
            derivation: None,
            base_definition: None,
            snapshot: None,
            differential: None,
        };

        populate_extension_slices(
            &mut resource,
            &elements,
            "Patient",
            &[ext_def],
            &HashMap::new(),
        );
        assert!(resource.get("extension").is_none());
    }

    // -- generate_slice_value tests ----------------------------------------

    #[test]
    fn test_generate_slice_value_with_pattern_coding() {
        let slice = ElementDefinition {
            id: "Patient.identifier:ABN".into(),
            path: "Patient.identifier".into(),
            slice_name: Some("ABN".into()),
            min: Some(1),
            type_: vec![ElementDefinitionType {
                code: "Identifier".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            pattern_coding: Some(serde_json::json!({
                "system": "http://hl7.org.au/id/abn",
                "code": "some-code"
            })),
            ..Default::default()
        };
        let elements = vec![
            ElementDefinition {
                id: "Patient.identifier".into(),
                path: "Patient.identifier".into(),
                type_: vec![ElementDefinitionType {
                    code: "Identifier".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
            slice.clone(),
        ];
        let result = generate_slice_value(
            &slice,
            "Patient",
            Some("system"),
            &[],
            &elements,
            &HashMap::new(),
        );
        assert!(result.is_some());
        let val = result.unwrap();
        // Should have a coding field from pattern_coding
        assert!(
            val.get("coding").is_some(),
            "Should have coding from pattern_coding"
        );
    }

    #[test]
    fn test_generate_slice_value_with_pattern_codeable_concept() {
        let slice = ElementDefinition {
            id: "Patient.identifier:ABN".into(),
            path: "Patient.identifier".into(),
            slice_name: Some("ABN".into()),
            min: Some(1),
            type_: vec![ElementDefinitionType {
                code: "Identifier".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            pattern_codeable_concept: Some(serde_json::json!({
                "coding": [{
                    "system": "http://hl7.org.au/id/abn",
                    "code": "some-code"
                }]
            })),
            ..Default::default()
        };
        let elements = vec![
            ElementDefinition {
                id: "Patient.identifier".into(),
                path: "Patient.identifier".into(),
                type_: vec![ElementDefinitionType {
                    code: "Identifier".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
            slice.clone(),
        ];
        let result = generate_slice_value(
            &slice,
            "Patient",
            Some("system"),
            &[],
            &elements,
            &HashMap::new(),
        );
        assert!(result.is_some());
        let val = result.unwrap();
        // Should have a coding field from pattern_codeable_concept
        assert!(
            val.get("coding").is_some(),
            "Should have coding from pattern_codeable_concept"
        );
    }

    #[test]
    fn test_generate_slice_value_with_discriminator_type_path() {
        let slice = ElementDefinition {
            id: "Patient.identifier:ABN".into(),
            path: "Patient.identifier".into(),
            slice_name: Some("ABN".into()),
            min: Some(1),
            type_: vec![ElementDefinitionType {
                code: "Identifier".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        };
        let elements = vec![
            ElementDefinition {
                id: "Patient.identifier".into(),
                path: "Patient.identifier".into(),
                type_: vec![ElementDefinitionType {
                    code: "Identifier".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
            slice.clone(),
        ];
        // discriminator_path starting with "type" should trigger type handling
        let result = generate_slice_value(
            &slice,
            "Patient",
            Some("type.coding.code"),
            &[],
            &elements,
            &HashMap::new(),
        );
        assert!(result.is_some());
        let val = result.unwrap();
        // Should have a type field (fallback CodeableConcept with XX code)
        assert!(
            val.get("type").is_some(),
            "Should have type field when discriminator path starts with 'type'"
        );
    }

    #[test]
    fn test_generate_slice_value_with_human_name() {
        let slice = ElementDefinition {
            id: "Patient.name:official".into(),
            path: "Patient.name".into(),
            slice_name: Some("official".into()),
            min: Some(1),
            type_: vec![ElementDefinitionType {
                code: "HumanName".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        };
        let elements = vec![
            ElementDefinition {
                id: "Patient.name".into(),
                path: "Patient.name".into(),
                type_: vec![ElementDefinitionType {
                    code: "HumanName".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
            slice.clone(),
            // The .use element that find_human_name_use looks for
            ElementDefinition {
                id: "Patient.name:official.use".into(),
                path: "Patient.name.use".into(),
                fixed_code: Some("official".into()),
                ..Default::default()
            },
        ];
        let result = generate_slice_value(
            &slice,
            "Patient",
            Some("use"),
            &[],
            &elements,
            &HashMap::new(),
        );
        assert!(result.is_some());
        let val = result.unwrap();
        // HumanName should have a "use" field
        assert!(val.get("use").is_some(), "HumanName should have use field");
    }

    #[test]
    fn test_generate_slice_value_with_pattern_uri() {
        let slice = ElementDefinition {
            id: "Patient.identifier:ABN".into(),
            path: "Patient.identifier".into(),
            slice_name: Some("ABN".into()),
            min: Some(1),
            type_: vec![ElementDefinitionType {
                code: "Identifier".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            pattern_uri: Some("http://hl7.org.au/id/abn".into()),
            ..Default::default()
        };
        let elements = vec![
            ElementDefinition {
                id: "Patient.identifier".into(),
                path: "Patient.identifier".into(),
                type_: vec![ElementDefinitionType {
                    code: "Identifier".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
            slice.clone(),
        ];
        let result = generate_slice_value(
            &slice,
            "Patient",
            Some("system"),
            &[],
            &elements,
            &HashMap::new(),
        );
        assert!(result.is_some());
        let val = result.unwrap();
        // Identifier with pattern_uri should have system set
        assert_eq!(
            val.get("system").and_then(|v| v.as_str()),
            Some("http://hl7.org.au/id/abn")
        );
    }

    #[test]
    fn test_generate_slice_value_with_pattern_string() {
        let slice = ElementDefinition {
            id: "Patient.name:official".into(),
            path: "Patient.name".into(),
            slice_name: Some("official".into()),
            min: Some(1),
            type_: vec![ElementDefinitionType {
                code: "HumanName".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            pattern_string: Some("Official Name".into()),
            ..Default::default()
        };
        let elements = vec![
            ElementDefinition {
                id: "Patient.name".into(),
                path: "Patient.name".into(),
                type_: vec![ElementDefinitionType {
                    code: "HumanName".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
            slice.clone(),
        ];
        let result = generate_slice_value(
            &slice,
            "Patient",
            Some("use"),
            &[],
            &elements,
            &HashMap::new(),
        );
        assert!(result.is_some());
        let val = result.unwrap();
        // Should have value set from pattern_string
        assert_eq!(
            val.get("value").and_then(|v| v.as_str()),
            Some("Official Name")
        );
    }

    #[test]
    fn test_generate_slice_value_with_pattern_code_human_name() {
        let slice = ElementDefinition {
            id: "Patient.name:official".into(),
            path: "Patient.name".into(),
            slice_name: Some("official".into()),
            min: Some(1),
            type_: vec![ElementDefinitionType {
                code: "HumanName".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            pattern_code: Some("official".into()),
            ..Default::default()
        };
        let elements = vec![
            ElementDefinition {
                id: "Patient.name".into(),
                path: "Patient.name".into(),
                type_: vec![ElementDefinitionType {
                    code: "HumanName".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
            slice.clone(),
        ];
        let result = generate_slice_value(
            &slice,
            "Patient",
            Some("use"),
            &[],
            &elements,
            &HashMap::new(),
        );
        assert!(result.is_some());
        let val = result.unwrap();
        // HumanName with pattern_code should have use set
        assert_eq!(val.get("use").and_then(|v| v.as_str()), Some("official"));
    }

    #[test]
    fn test_generate_slice_value_with_pattern_code_address() {
        let slice = ElementDefinition {
            id: "Patient.address:home".into(),
            path: "Patient.address".into(),
            slice_name: Some("home".into()),
            min: Some(1),
            type_: vec![ElementDefinitionType {
                code: "Address".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            pattern_code: Some("home".into()),
            ..Default::default()
        };
        let elements = vec![
            ElementDefinition {
                id: "Patient.address".into(),
                path: "Patient.address".into(),
                type_: vec![ElementDefinitionType {
                    code: "Address".into(),
                    profile: vec![],
                    target_profile: vec![],
                    versioning: None,
                }],
                ..Default::default()
            },
            slice.clone(),
        ];
        let result = generate_slice_value(
            &slice,
            "Patient",
            Some("type"),
            &[],
            &elements,
            &HashMap::new(),
        );
        assert!(result.is_some());
        let val = result.unwrap();
        // Address with pattern_code should have type set
        assert_eq!(val.get("type").and_then(|v| v.as_str()), Some("home"));
    }

    #[test]
    fn test_populate_extension_slices_complex_with_sub_extensions() {
        let mut resource = json!({"resourceType": "Patient"});
        let elements = vec![ElementDefinition {
            id: "Patient.extension:complexExt".into(),
            path: "Patient.extension".into(),
            slice_name: Some("complexExt".into()),
            type_: vec![ElementDefinitionType {
                code: "Extension".into(),
                profile: vec!["http://example.org/Extension/ComplexExt".into()],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        }];

        let ext_def = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Extension/ComplexExt".to_string(),
            name: "ComplexExt".to_string(),
            base_type: "Extension".to_string(),
            kind: "complex-type".to_string(),
            derivation: None,
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Extension".into(),
                        path: "Extension".into(),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.url".into(),
                        path: "Extension.url".into(),
                        fixed_uri: Some("http://example.org/ext/complex".into()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.value[x]".into(),
                        path: "Extension.value[x]".into(),
                        max: Some("0".into()),
                        ..Default::default()
                    },
                    // Required sub-extension
                    ElementDefinition {
                        id: "Extension.extension:requiredSub".into(),
                        path: "Extension.extension".into(),
                        slice_name: Some("requiredSub".into()),
                        min: Some(1),
                        type_: vec![ElementDefinitionType {
                            code: "Extension".into(),
                            profile: vec![],
                            target_profile: vec![],
                            versioning: None,
                        }],
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.extension:requiredSub.url".into(),
                        path: "Extension.extension.url".into(),
                        fixed_uri: Some("required-value".into()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Extension.extension:requiredSub.value[x]".into(),
                        path: "Extension.extension.value[x]".into(),
                        type_: vec![ElementDefinitionType {
                            code: "string".into(),
                            profile: vec![],
                            target_profile: vec![],
                            versioning: None,
                        }],
                        ..Default::default()
                    },
                    // Optional sub-extension (min=0) -- should be skipped
                    ElementDefinition {
                        id: "Extension.extension:optionalSub".into(),
                        path: "Extension.extension".into(),
                        slice_name: Some("optionalSub".into()),
                        min: Some(0),
                        type_: vec![ElementDefinitionType {
                            code: "Extension".into(),
                            profile: vec![],
                            target_profile: vec![],
                            versioning: None,
                        }],
                        ..Default::default()
                    },
                ],
            }),
            differential: None,
        };

        populate_extension_slices(
            &mut resource,
            &elements,
            "Patient",
            &[ext_def],
            &HashMap::new(),
        );
        assert!(resource.get("extension").is_some());
        let exts = resource["extension"].as_array().unwrap();
        assert_eq!(exts.len(), 1);
        assert_eq!(exts[0]["url"], "http://example.org/ext/complex");
        // Should have sub-extensions (only the required one)
        assert!(exts[0].get("extension").is_some());
        let sub_exts = exts[0]["extension"].as_array().unwrap();
        assert_eq!(sub_exts.len(), 1);
        assert_eq!(sub_exts[0]["url"], "required-value");
        assert!(sub_exts[0].get("valueString").is_some());
    }
}
