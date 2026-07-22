use crate::model::*;
use anyhow::Result;

/// Generate a synthetic FHIR resource that conforms to a StructureDefinition profile.
///
/// Walks the snapshot elements, fills in required fields (min > 0) with appropriate
/// sentinel values, and applies any fixed/pattern constraints.
pub fn generate_resource(profile: &StructureDefinition) -> Result<serde_json::Value> {
    let mut resource = serde_json::json!({
        "resourceType": profile.base_type
    });

    let empty = vec![];
    let elements = match &profile.snapshot {
        Some(snapshot) => &snapshot.element,
        None => profile.differential.as_ref().map(|d| &d.element).unwrap_or(&empty),
    };

    populate_required_fields(&mut resource, elements, &profile.base_type)?;

    Ok(resource)
}

fn populate_required_fields(
    resource: &mut serde_json::Value,
    elements: &[ElementDefinition],
    resource_type: &str,
) -> Result<()> {
    for element in elements {
        // Skip the root element (e.g., "Patient")
        let field_name = match get_field_name(&element.path, resource_type) {
            Some(name) => name,
            None => continue,
        };

        if field_name == resource_type {
            continue;
        }

        let min = element.min.unwrap_or(0);
        if min == 0 {
            continue; // Optional field, skip
        }

        // Check if field already populated (by a parent element handler)
        if resource.get(&field_name).is_some() {
            continue;
        }

        // Apply fixed values first
        if let Some(val) = &element.fixed_string {
            resource[field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.fixed_code {
            resource[field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.fixed_uri {
            resource[field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.fixed_boolean {
            resource[field_name] = serde_json::Value::Bool(*val);
            continue;
        }
        if let Some(val) = &element.fixed_integer {
            resource[field_name] = serde_json::Value::Number((*val).into());
            continue;
        }

        // Apply pattern values
        if let Some(val) = &element.pattern_string {
            resource[field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.pattern_code {
            resource[field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.pattern_uri {
            resource[field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.pattern_boolean {
            resource[field_name] = serde_json::Value::Bool(*val);
            continue;
        }

        // Generate based on type
        if element.type_.is_empty() {
            continue;
        }

        let type_code = &element.type_[0].code;
        let value = generate_typed_value(type_code, &element.type_[0].target_profile);

        // Handle cardinality: if max is "*" or > 1, wrap in array
        let max = element.max.as_deref().unwrap_or("1");
        if max != "1" {
            resource[field_name] = serde_json::json!([value]);
        } else {
            resource[field_name] = value;
        }
    }

    Ok(())
}

/// Extract the field name from a FHIR path like "Patient.name" → "name".
/// Returns None for paths that don't belong to this resource type or are nested.
fn get_field_name(path: &str, resource_type: &str) -> Option<String> {
    if !path.starts_with(resource_type) {
        return None;
    }

    let remainder = path.strip_prefix(resource_type)?;
    if remainder.is_empty() {
        return Some(resource_type.to_string());
    }

    if !remainder.starts_with('.') {
        return None;
    }

    let field_part = remainder.strip_prefix('.')?;

    // Only handle direct children (no nested paths like "Patient.name.family")
    if field_part.contains('.') {
        return None;
    }

    // Strip slice notation (e.g., "identifier:type" → "identifier")
    let field_name = field_part.split(':').next().unwrap_or(field_part);

    Some(field_name.to_string())
}

/// Generate a minimal valid value for a given FHIR type code.
fn generate_typed_value(
    type_code: &str,
    target_profiles: &[String],
) -> serde_json::Value {
    match type_code {
        // Primitive types
        "string" => serde_json::json!("generated-string"),
        "uri" => serde_json::json!("http://example.org/generated-uri"),
        "url" => serde_json::json!("http://example.org/generated-url"),
        "canonical" => serde_json::json!("http://example.org/generated-canonical"),
        "code" => serde_json::json!("generated-code"),
        "id" => serde_json::Value::String(uuid::Uuid::new_v4().to_string()),
        "boolean" => serde_json::json!(true),
        "integer" => serde_json::json!(1),
        "decimal" => serde_json::json!(1.0),
        "date" => serde_json::json!("2024-01-01"),
        "dateTime" => serde_json::json!("2024-01-01T00:00:00Z"),
        "instant" => serde_json::json!("2024-01-01T00:00:00Z"),
        "time" => serde_json::json!("00:00:00"),
        "unsignedInt" => serde_json::json!(1),
        "positiveInt" => serde_json::json!(1),
        "base64Binary" => serde_json::json!(""),
        "markdown" => serde_json::json!("generated-markdown"),
        "oid" => serde_json::json!("urn:oid:2.16.840.1.113883.19.5"),
        "uuid" => serde_json::Value::String(format!("urn:uuid:{}", uuid::Uuid::new_v4())),

        // Complex types
        "Identifier" => serde_json::json!({
            "system": "http://example.org/identifier",
            "value": format!("generated-{}", uuid::Uuid::new_v4())
        }),
        "HumanName" => serde_json::json!({
            "family": "GeneratedFamily",
            "given": ["GeneratedGiven"]
        }),
        "Address" => serde_json::json!({
            "line": ["123 Generated St"],
            "city": "GeneratedCity",
            "state": "GC",
            "postalCode": "00000",
            "country": "US"
        }),
        "ContactPoint" => serde_json::json!({
            "system": "phone",
            "value": "555-0000"
        }),
        "CodeableConcept" => serde_json::json!({
            "coding": [{
                "system": "http://example.org/code-system",
                "code": "generated-code"
            }]
        }),
        "Coding" => serde_json::json!({
            "system": "http://example.org/code-system",
            "code": "generated-code"
        }),
        "Quantity" => serde_json::json!({
            "value": 1.0,
            "unit": "generated-unit",
            "system": "http://example.org/unit-system",
            "code": "generated-unit"
        }),
        "Reference" => {
            if let Some(profile) = target_profiles.first() {
                // Extract resource type from profile URL
                let ref_type = profile
                    .rsplit('/')
                    .next()
                    .unwrap_or("Resource");
                serde_json::json!({
                    "reference": format!("placeholder:{}", ref_type)
                })
            } else {
                serde_json::json!({
                    "reference": "placeholder:Resource"
                })
            }
        }
        "Period" => serde_json::json!({
            "start": "2024-01-01T00:00:00Z",
            "end": "2024-12-31T23:59:59Z"
        }),
        "Attachment" => serde_json::json!({
            "contentType": "text/plain",
            "data": ""
        }),
        "Annotation" => serde_json::json!({
            "text": "Generated annotation"
        }),
        "Range" => serde_json::json!({
            "low": { "value": 0.0 },
            "high": { "value": 100.0 }
        }),
        "Ratio" => serde_json::json!({
            "numerator": { "value": 1.0 },
            "denominator": { "value": 1.0 }
        }),
        "Timing" => serde_json::json!({
            "repeat": { "frequency": 1, "period": 1.0, "periodUnit": "d" }
        }),
        "SampledData" => serde_json::json!({
            "origin": { "value": 0.0 },
            "period": 1.0,
            "dimensions": 1,
            "data": "0"
        }),

        // Fallback: empty object for unknown complex types
        _ => serde_json::json!({}),
    }
}

#[cfg(test)]
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
                    },
                    ElementDefinition {
                        id: "Patient.identifier".to_string(),
                        path: "Patient.identifier".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Identifier".to_string(),
                            target_profile: vec![],
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
                    },
                    ElementDefinition {
                        id: "Patient.name".to_string(),
                        path: "Patient.name".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "HumanName".to_string(),
                            target_profile: vec![],
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
                    },
                    ElementDefinition {
                        id: "Patient.gender".to_string(),
                        path: "Patient.gender".to_string(),
                        min: Some(0),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "code".to_string(),
                            target_profile: vec![],
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
                    },
                ],
            }),
            differential: None,
        }
    }

    #[test]
    fn generate_patient_from_profile() {
        let profile = minimal_patient_profile();
        let resource = generate_resource(&profile).unwrap();

        assert_eq!(resource["resourceType"], "Patient");
        assert!(
            resource.get("name").is_some(),
            "name is required (min=1)"
        );
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
            });
        }

        let resource = generate_resource(&profile).unwrap();
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
                    },
                    ElementDefinition {
                        id: "Observation.subject".to_string(),
                        path: "Observation.subject".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Reference".to_string(),
                            target_profile: vec![
                                "http://hl7.org/fhir/StructureDefinition/Patient".to_string(),
                            ],
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
                    },
                ],
            }),
            differential: None,
        };

        let resource = generate_resource(&profile).unwrap();
        assert_eq!(resource["resourceType"], "Observation");
        let subject = &resource["subject"];
        assert_eq!(subject["reference"], "placeholder:Patient");
    }
}