use crate::mutators::Mutator;
use fhir_autotest::model::profile::StructureDefinition;

/// Cardinality violation mutator: adds/removes fields to violate min/max constraints.
///
/// For each field in the profile's element definitions:
/// - Remove required fields (min > 0)
/// - Add duplicate instances of max=1 fields
/// - Add many instances of max=* fields
/// - Add unexpected fields not in the profile
pub struct CardinalityMutator;

impl Mutator for CardinalityMutator {
    fn name(&self) -> &'static str {
        "cardinality"
    }

    fn mutate(
        &self,
        base_resource: &serde_json::Value,
        profile: &StructureDefinition,
        seed: u64,
    ) -> serde_json::Value {
        let mut resource = base_resource.clone();
        apply_cardinality_mutations(&mut resource, profile, seed);
        resource
    }
}

fn field_name_from_path(path: &str) -> &str {
    path.split('.').next_back().unwrap_or(path)
}

fn apply_cardinality_mutations(
    resource: &mut serde_json::Value,
    profile: &StructureDefinition,
    seed: u64,
) {
    let strategy = (seed % 4) as u8;

    match strategy {
        // Remove required fields
        0 => {
            if let Some(snapshot) = &profile.snapshot {
                for element in &snapshot.element {
                    if element.min.unwrap_or(0) > 0
                        && let Some(obj) = resource.as_object_mut()
                    {
                        let field_name = field_name_from_path(&element.path);
                        if field_name != profile.base_type && !field_name.contains(':') {
                            obj.remove(field_name);
                        }
                    }
                }
            }
        }
        // Add duplicate instances of max=1 fields
        1 => {
            if let Some(snapshot) = &profile.snapshot {
                for element in &snapshot.element {
                    if element.max.as_deref() == Some("1")
                        && let Some(obj) = resource.as_object_mut()
                    {
                        let field_name = field_name_from_path(&element.path);
                        if field_name != profile.base_type
                            && !field_name.contains(':')
                            && obj.contains_key(field_name)
                        {
                            let existing = obj[field_name].clone();
                            obj[field_name] = serde_json::json!([existing, existing]);
                        }
                    }
                }
            }
        }
        // Add unexpected fields
        2 => {
            if let Some(obj) = resource.as_object_mut() {
                obj.insert(
                    "x_fhir_unknown_field".to_string(),
                    serde_json::json!("unexpected"),
                );
                obj.insert(
                    "x_fhir_nested_unknown".to_string(),
                    serde_json::json!({
                        "nested": "value",
                        "deep": {"deeper": "value"}
                    }),
                );
            }
        }
        // Remove all optional fields (leave only required)
        3 => {
            if let Some(snapshot) = &profile.snapshot {
                let required: std::collections::HashSet<&str> = snapshot
                    .element
                    .iter()
                    .filter(|e| e.min.unwrap_or(0) > 0)
                    .filter_map(|e| {
                        let name = field_name_from_path(&e.path);
                        if name != profile.base_type && !name.contains(':') {
                            Some(name)
                        } else {
                            None
                        }
                    })
                    .collect();

                if let Some(obj) = resource.as_object_mut() {
                    obj.retain(|key, _| required.contains(key.as_str()));
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fhir_autotest::model::profile::*;

    fn test_profile() -> StructureDefinition {
        StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Test".to_string(),
            base_type: "Patient".to_string(),
            name: "TestPatient".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Patient".to_string(),
                        path: "Patient".to_string(),
                        min: Some(0),
                        max: Some("*".to_string()),
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
                    },
                    ElementDefinition {
                        id: "Patient.name".to_string(),
                        path: "Patient.name".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "HumanName".to_string(),
                            target_profile: vec![],
                            profile: vec![],
                            versioning: None,
                        }],
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
                        must_support: true,
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
                    },
                    ElementDefinition {
                        id: "Patient.gender".to_string(),
                        path: "Patient.gender".to_string(),
                        min: Some(0),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "code".to_string(),
                            target_profile: vec![],
                            profile: vec![],
                            versioning: None,
                        }],
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
                    },
                ],
            }),
            differential: None,
        }
    }

    #[test]
    fn remove_required_field() {
        let profile = test_profile();
        let mut resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith"}],
            "gender": "male"
        });
        apply_cardinality_mutations(&mut resource, &profile, 0);
        assert!(
            resource.get("name").is_none(),
            "Required field 'name' should be removed"
        );
        assert!(
            resource.get("gender").is_some(),
            "Optional field 'gender' should remain"
        );
    }

    #[test]
    fn duplicate_max_one_field() {
        let profile = test_profile();
        let mut resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith"}],
            "gender": "male"
        });
        apply_cardinality_mutations(&mut resource, &profile, 1);
        assert!(
            resource["gender"].is_array(),
            "gender (max=1) should be wrapped in array"
        );
    }
}
