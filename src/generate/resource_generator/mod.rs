pub mod fields;
pub mod slices;
pub mod types;
pub mod valuesets;

pub use valuesets::{build_code_system_first_code_map, build_value_set_system_map};

use crate::model::*;
use anyhow::Result;
use std::collections::HashMap;

/// Generate a synthetic FHIR resource that conforms to a StructureDefinition profile.
///
/// Walks the snapshot elements, fills in required fields (min > 0) with appropriate
/// sentinel values, and applies any fixed/pattern constraints. Also stamps
/// `meta.profile` with the profile's canonical URL.
pub fn generate_resource(
    profile: &StructureDefinition,
    all_profiles: &[StructureDefinition],
) -> Result<serde_json::Value> {
    generate_resource_with_value_sets(profile, all_profiles, &HashMap::new())
}

pub fn generate_resource_with_value_sets(
    profile: &StructureDefinition,
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
) -> Result<serde_json::Value> {
    let mut resource = serde_json::json!({
        "resourceType": profile.base_type
    });

    // Stamp the profile URL so servers know which profile this conforms to
    resource["meta"] = serde_json::json!({
        "profile": [profile.url]
    });

    let empty = vec![];
    let elements = match &profile.snapshot {
        Some(snapshot) => &snapshot.element,
        None => profile
            .differential
            .as_ref()
            .map(|d| &d.element)
            .unwrap_or(&empty),
    };

    fields::populate_required_fields(
        &mut resource,
        elements,
        &profile.base_type,
        all_profiles,
        value_set_systems,
    )?;

    // Second pass: populate required slices (e.g., identifier:abn with patternUri)
    slices::populate_required_slices(
        &mut resource,
        elements,
        &profile.base_type,
        all_profiles,
        value_set_systems,
    )?;

    // Third pass: populate extension slices defined by the profile
    // (e.g., HealthcareService.extension:active-period, Location.extension:amenity)
    slices::populate_extension_slices(
        &mut resource,
        elements,
        &profile.base_type,
        all_profiles,
        value_set_systems,
    );

    // Fourth pass: populate mustSupport BackboneElement fields that were skipped
    // in pass 1 because min=0. A backbone with min=0 but mustSupport=true should
    // be generated when at least one of its children has min=1 — this ensures
    // the conformance must_support checker can verify the field is present.
    fields::populate_must_support_backbones(
        &mut resource,
        elements,
        &profile.base_type,
        all_profiles,
        value_set_systems,
    );

    Ok(resource)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn minimal_patient_profile() -> StructureDefinition {
        StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestPatient".to_string(),
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
                        id: "Patient.identifier".to_string(),
                        path: "Patient.identifier".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Identifier".to_string(),
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
    fn generate_patient_from_profile() {
        let profile = minimal_patient_profile();
        let resource = generate_resource(&profile, &[]).unwrap();

        assert_eq!(resource["resourceType"], "Patient");
        assert!(resource.get("name").is_some(), "name is required (min=1)");
        assert!(
            resource.get("identifier").is_some(),
            "identifier is required (min=1)"
        );
        // gender is optional (min=0), should not be present
        assert!(
            resource.get("gender").is_none(),
            "gender is optional (min=0), should not be generated"
        );
    }

    #[test]
    fn generate_resource_with_fixed_values() {
        let mut profile = minimal_patient_profile();
        // Add a fixed gender
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.gender".to_string(),
                path: "Patient.gender".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "code".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_code: Some("male".to_string()),
                fixed_string: None,
                fixed_uri: None,
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
            });
        }

        let resource = generate_resource(&profile, &[]).unwrap();
        assert_eq!(resource["gender"], "male");
    }

    #[test]
    fn generate_reference_with_target_profile() {
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestObservation".to_string(),
            base_type: "Observation".to_string(),
            name: "TestObservation".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Observation".to_string(),
                        path: "Observation".to_string(),
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
                        id: "Observation.subject".to_string(),
                        path: "Observation.subject".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Reference".to_string(),
                            target_profile: vec![
                                "http://hl7.org/fhir/StructureDefinition/Patient".to_string()
                            ],
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
                ],
            }),
            differential: None,
        };

        let resource = generate_resource(&profile, &[]).unwrap();
        let subject = &resource["subject"];
        let reference = subject["reference"].as_str().unwrap_or("");
        assert!(
            reference.starts_with("Patient/"),
            "Reference should use target_profile to determine resource type"
        );
    }

    #[test]
    fn backbone_element_gets_required_subfields() {
        // Practitioner profile with required qualification (BackboneElement)
        // that has required sub-fields: identifier and code
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestPractitioner".to_string(),
            base_type: "Practitioner".to_string(),
            name: "TestPractitioner".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Practitioner".to_string(),
                        path: "Practitioner".to_string(),
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
                        id: "Practitioner.qualification".to_string(),
                        path: "Practitioner.qualification".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "BackboneElement".to_string(),
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
                    ElementDefinition {
                        id: "Practitioner.qualification.identifier".to_string(),
                        path: "Practitioner.qualification.identifier".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Identifier".to_string(),
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
                    ElementDefinition {
                        id: "Practitioner.qualification.code".to_string(),
                        path: "Practitioner.qualification.code".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "CodeableConcept".to_string(),
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
        };

        let resource = generate_resource(&profile, &[]).unwrap();

        // qualification should be an array (max="*") with required sub-fields
        let qualification = resource
            .get("qualification")
            .expect("qualification is required (min=1)");
        let qual_array = qualification
            .as_array()
            .expect("qualification should be an array");
        assert!(
            !qual_array.is_empty(),
            "qualification array should not be empty"
        );

        let qual = &qual_array[0];
        assert!(
            qual.get("identifier").is_some(),
            "qualification.identifier should be populated (required)"
        );
        assert!(
            qual.get("code").is_some(),
            "qualification.code should be populated (required)"
        );
    }

    #[test]
    fn extension_type_fields_are_skipped() {
        // Extension fields with min > 0 should be skipped since we can't
        // generate valid extensions without knowing the URL
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestPatient".to_string(),
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
                        id: "Patient.extension".to_string(),
                        path: "Patient.extension".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Extension".to_string(),
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
                    ElementDefinition {
                        id: "Patient.identifier".to_string(),
                        path: "Patient.identifier".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Identifier".to_string(),
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
        };

        let resource = generate_resource(&profile, &[]).unwrap();

        // Extension should NOT be present (empty extensions are invalid)
        assert!(
            resource.get("extension").is_none(),
            "Extension fields should be skipped since empty extensions are invalid"
        );
        // But identifier should be present
        assert!(
            resource.get("identifier").is_some(),
            "identifier should still be populated even when extension is skipped"
        );
    }

    #[test]
    fn always_array_fields_are_wrapped_in_arrays() {
        // Organization profile with address (min=1, max=1) and identifier (min=1, max=*)
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestOrg".to_string(),
            base_type: "Organization".to_string(),
            name: "TestOrg".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Organization".to_string(),
                        path: "Organization".to_string(),
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
                        id: "Organization.identifier".to_string(),
                        path: "Organization.identifier".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Identifier".to_string(),
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
                    ElementDefinition {
                        id: "Organization.name".to_string(),
                        path: "Organization.name".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "string".to_string(),
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
                    ElementDefinition {
                        id: "Organization.address".to_string(),
                        path: "Organization.address".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Address".to_string(),
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
        };

        let resource = generate_resource(&profile, &[]).unwrap();

        // identifier (max="*") should be an array
        let identifier = resource.get("identifier").unwrap();
        assert!(
            identifier.is_array(),
            "identifier should be an array (max=*)"
        );

        // name (max="1", Organization.name is a string 0..1) should be a string
        let name = resource.get("name").unwrap();
        assert!(
            name.is_string(),
            "name should be a string (Organization.name is 0..1 in base spec)"
        );

        // address (max="1" but Organization.address is 0..* in base spec) should be an array
        let address = resource.get("address").unwrap();
        assert!(
            address.is_array(),
            "address should be an array (Organization.address is 0..* in base spec)"
        );
    }

    #[test]
    fn nested_required_fields_at_depth_2() {
        // Practitioner with qualification.code.text (min=1) — 2-level nesting
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestPractitioner".to_string(),
            base_type: "Practitioner".to_string(),
            name: "TestPractitioner".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Practitioner".to_string(),
                        path: "Practitioner".to_string(),
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
                        id: "Practitioner.qualification".to_string(),
                        path: "Practitioner.qualification".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "BackboneElement".to_string(),
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
                    ElementDefinition {
                        id: "Practitioner.qualification.code".to_string(),
                        path: "Practitioner.qualification.code".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "CodeableConcept".to_string(),
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
                    ElementDefinition {
                        id: "Practitioner.qualification.code.text".to_string(),
                        path: "Practitioner.qualification.code.text".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "string".to_string(),
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
        };

        let resource = generate_resource(&profile, &[]).unwrap();

        let qualification = resource.get("qualification").unwrap();
        let qual_array = qualification.as_array().unwrap();
        assert!(
            !qual_array.is_empty(),
            "qualification array should not be empty"
        );

        let qual = &qual_array[0];
        let code = qual.get("code").unwrap();
        assert!(
            code.is_object(),
            "code should be an object (CodeableConcept)"
        );
        let text = code.get("text").unwrap();
        assert!(
            text.as_str().is_some(),
            "code.text should be a non-null string (required at depth 2)"
        );
    }

    #[test]
    fn uses_valueset_system_for_bound_codeable_concept() {
        let profile_json = serde_json::json!({
            "resourceType": "StructureDefinition",
            "url": "http://example.org/StructureDefinition/TestService",
            "name": "TestService",
            "type": "HealthcareService",
            "kind": "resource",
            "derivation": "constraint",
            "snapshot": {
                "element": [
                    { "id": "HealthcareService", "path": "HealthcareService", "min": 0, "max": "*" },
                    {
                        "id": "HealthcareService.type",
                        "path": "HealthcareService.type",
                        "min": 1,
                        "max": "*",
                        "type": [{ "code": "CodeableConcept" }],
                        "binding": {
                            "strength": "required",
                            "valueSet": "http://example.org/fhir/ValueSet/service-type|1.0.0"
                        }
                    }
                ]
            }
        });
        let profile: StructureDefinition = serde_json::from_value(profile_json).unwrap();

        let mut value_set_systems = std::collections::HashMap::new();
        value_set_systems.insert(
            "http://example.org/fhir/ValueSet/service-type".to_string(),
            "http://example.org/fhir/CodeSystem/service-type".to_string(),
        );

        let resource =
            generate_resource_with_value_sets(&profile, &[], &value_set_systems).unwrap();

        let coding = resource["type"][0]["coding"][0].clone();
        assert_eq!(
            coding["system"].as_str().unwrap(),
            "http://example.org/fhir/CodeSystem/service-type"
        );
    }

    #[test]
    fn honors_pattern_codeable_concept_before_default_generation() {
        let profile_json = serde_json::json!({
            "resourceType": "StructureDefinition",
            "url": "http://example.org/StructureDefinition/TestPractitionerRole",
            "name": "TestPractitionerRole",
            "type": "PractitionerRole",
            "kind": "resource",
            "derivation": "constraint",
            "snapshot": {
                "element": [
                    { "id": "PractitionerRole", "path": "PractitionerRole", "min": 0, "max": "*" },
                    {
                        "id": "PractitionerRole.code",
                        "path": "PractitionerRole.code",
                        "min": 1,
                        "max": "*",
                        "type": [{ "code": "CodeableConcept" }],
                        "patternCodeableConcept": {
                            "coding": [{
                                "system": "http://example.org/fhir/CodeSystem/practitioner-role",
                                "code": "a-specialty"
                            }]
                        }
                    }
                ]
            }
        });
        let profile: StructureDefinition = serde_json::from_value(profile_json).unwrap();

        let resource = generate_resource(&profile, &[]).unwrap();
        let coding = resource["code"][0]["coding"][0].clone();

        assert_eq!(
            coding["system"].as_str().unwrap(),
            "http://example.org/fhir/CodeSystem/practitioner-role"
        );
        assert_eq!(coding["code"].as_str().unwrap(), "a-specialty");
    }
}
