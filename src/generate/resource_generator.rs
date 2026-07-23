use crate::model::*;
use anyhow::Result;

/// Generate a synthetic FHIR resource that conforms to a StructureDefinition profile.
///
/// Walks the snapshot elements, fills in required fields (min > 0) with appropriate
/// sentinel values, and applies any fixed/pattern constraints. Also stamps
/// `meta.profile` with the profile's canonical URL.
pub fn generate_resource(profile: &StructureDefinition) -> Result<serde_json::Value> {
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

    populate_required_fields(&mut resource, elements, &profile.base_type)?;

    Ok(resource)
}

fn populate_required_fields(
    resource: &mut serde_json::Value,
    elements: &[ElementDefinition],
    resource_type: &str,
) -> Result<()> {
    // First pass: populate direct required children (depth 1 fields)
    for element in elements {
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
            resource[&field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.fixed_code {
            resource[&field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.fixed_uri {
            resource[&field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.fixed_boolean {
            resource[&field_name] = serde_json::Value::Bool(*val);
            continue;
        }
        if let Some(val) = &element.fixed_integer {
            resource[&field_name] = serde_json::Value::Number((*val).into());
            continue;
        }

        // Apply pattern values
        if let Some(val) = &element.pattern_string {
            resource[&field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.pattern_code {
            resource[&field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.pattern_uri {
            resource[&field_name] = serde_json::Value::String(val.clone());
            continue;
        }
        if let Some(val) = &element.pattern_boolean {
            resource[&field_name] = serde_json::Value::Bool(*val);
            continue;
        }

        // Skip fields with no type information (slice definitions etc.)
        if element.type_.is_empty() {
            continue;
        }

        let type_code = &element.type_[0].code;

        // Skip Extension type — empty extensions are always invalid (violates ext-1:
        // "Must have either extensions or value[x], not both"). Without knowing the
        // extension URL and value, we can't generate a valid extension.
        if type_code == "Extension" {
            continue;
        }

        let target_profiles = &element.type_[0].target_profile;
        let value = generate_typed_value(type_code, target_profiles);

        // For BackboneElement, populate required sub-fields from nested elements
        if type_code == "BackboneElement" {
            let mut backbone = value.as_object().cloned().unwrap_or_default();
            populate_backbone_fields(&mut backbone, &element.path, elements, resource_type);
            let max = element.max.as_deref().unwrap_or("1");
            if max != "1" {
                resource[&field_name] = serde_json::json!([backbone]);
            } else {
                resource[&field_name] = serde_json::json!(backbone);
            }
            continue;
        }

        // Handle cardinality: if max is "*" or > 1, wrap in array.
        // Also wrap in array for fields that are 0..* in the FHIR R4 base
        // spec but constrained to max=1 by a profile. HAPI validates against
        // the base spec and rejects non-array values for these fields.
        let max = element.max.as_deref().unwrap_or("1");
        if max != "1" || is_base_spec_repeatable(resource_type, &field_name) {
            resource[&field_name] = serde_json::json!([value]);
        } else {
            resource[&field_name] = value;
        }
    }

    Ok(())
}

/// Populate required sub-fields of a BackboneElement by looking at nested elements
/// in the snapshot. E.g., for "Practitioner.qualification", find
/// "Practitioner.qualification.identifier" (min=1) and "Practitioner.qualification.code" (min=1).
/// Also handles 2-level nesting: e.g. "Practitioner.qualification.code.text" (min=1)
/// populates the `text` field inside the `code` CodeableConcept.
fn populate_backbone_fields(
    backbone: &mut serde_json::Map<String, serde_json::Value>,
    parent_path: &str,
    elements: &[ElementDefinition],
    resource_type: &str,
) {
    // First pass: populate direct children (depth 1)
    for element in elements {
        if !element.path.starts_with(&format!("{}.", parent_path)) {
            continue;
        }

        let suffix = element
            .path
            .strip_prefix(&format!("{}.", parent_path))
            .unwrap_or("");
        if suffix.contains('.') {
            continue; // Skip deeply nested paths in first pass
        }

        // Strip slice notation
        let field_name = suffix.split(':').next().unwrap_or(suffix);

        let min = element.min.unwrap_or(0);
        if min == 0 {
            continue;
        }

        if backbone.contains_key(field_name) {
            continue;
        }

        // Apply fixed/pattern values
        if let Some(val) = &element.fixed_string {
            backbone.insert(
                field_name.to_string(),
                serde_json::Value::String(val.clone()),
            );
            continue;
        }
        if let Some(val) = &element.fixed_code {
            backbone.insert(
                field_name.to_string(),
                serde_json::Value::String(val.clone()),
            );
            continue;
        }
        if let Some(val) = &element.fixed_uri {
            backbone.insert(
                field_name.to_string(),
                serde_json::Value::String(val.clone()),
            );
            continue;
        }
        if let Some(val) = &element.fixed_boolean {
            backbone.insert(field_name.to_string(), serde_json::Value::Bool(*val));
            continue;
        }

        if element.type_.is_empty() {
            continue;
        }

        let type_code = &element.type_[0].code;
        let target_profiles = &element.type_[0].target_profile.clone();

        if type_code == "Extension" {
            continue;
        }

        let mut value = generate_typed_value(type_code, target_profiles);

        // For complex types inside a backbone, check for required sub-fields
        // at depth 2 (e.g. qualification.code.text)
        if is_complex_type(type_code) {
            let child_path = format!("{}.{}", parent_path, field_name);
            populate_nested_required_fields(&mut value, &child_path, elements);
        }

        let max = element.max.as_deref().unwrap_or("1");
        if max != "1" || is_base_spec_repeatable(resource_type, field_name) {
            backbone.insert(field_name.to_string(), serde_json::json!([value]));
        } else {
            backbone.insert(field_name.to_string(), value);
        }
    }
}

/// Populate required sub-fields at depth 2 inside a complex type.
/// E.g., for "Practitioner.qualification.code", find
/// "Practitioner.qualification.code.text" (min=1) and populate it.
fn populate_nested_required_fields(
    value: &mut serde_json::Value,
    parent_path: &str,
    elements: &[ElementDefinition],
) {
    let obj = match value.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    for element in elements {
        if !element.path.starts_with(&format!("{}.", parent_path)) {
            continue;
        }

        let suffix = element
            .path
            .strip_prefix(&format!("{}.", parent_path))
            .unwrap_or("");
        if suffix.contains('.') {
            continue; // Only depth 2 (direct children of the complex type)
        }

        let field_name = suffix.split(':').next().unwrap_or(suffix);
        let min = element.min.unwrap_or(0);
        if min == 0 {
            continue;
        }
        if obj.contains_key(field_name) {
            continue;
        }

        // Apply fixed/pattern values
        if let Some(val) = &element.fixed_string {
            obj.insert(
                field_name.to_string(),
                serde_json::Value::String(val.clone()),
            );
            continue;
        }
        if let Some(val) = &element.fixed_code {
            obj.insert(
                field_name.to_string(),
                serde_json::Value::String(val.clone()),
            );
            continue;
        }
        if let Some(val) = &element.fixed_uri {
            obj.insert(
                field_name.to_string(),
                serde_json::Value::String(val.clone()),
            );
            continue;
        }
        if let Some(val) = &element.fixed_boolean {
            obj.insert(field_name.to_string(), serde_json::Value::Bool(*val));
            continue;
        }

        if element.type_.is_empty() {
            continue;
        }

        let type_code = &element.type_[0].code;
        let target_profiles = &element.type_[0].target_profile.clone();

        if type_code == "Extension" {
            continue;
        }

        let child_value = generate_typed_value(type_code, target_profiles);
        let max = element.max.as_deref().unwrap_or("1");
        if max != "1" {
            obj.insert(field_name.to_string(), serde_json::json!([child_value]));
        } else {
            obj.insert(field_name.to_string(), child_value);
        }
    }
}

/// Returns true for FHIR complex types that can have sub-fields.
fn is_complex_type(type_code: &str) -> bool {
    matches!(
        type_code,
        "Identifier"
            | "HumanName"
            | "Address"
            | "ContactPoint"
            | "CodeableConcept"
            | "Coding"
            | "Quantity"
            | "Reference"
            | "Period"
            | "Attachment"
            | "Annotation"
            | "Range"
            | "Ratio"
            | "Timing"
            | "SampledData"
            | "BackboneElement"
    )
}

/// Returns true for fields that are 0..* in the FHIR R4 base spec.
///
/// Some fields (identifier, telecom, extension, etc.) are always repeatable
/// across all resource types. Others (name, address) are only repeatable for
/// specific resource types. HAPI validates against the base spec and rejects
/// non-array values for these fields even when a profile constrains max=1.
fn is_base_spec_repeatable(resource_type: &str, field_name: &str) -> bool {
    // Fields that are always 0..* regardless of resource type
    if matches!(
        field_name,
        "identifier"
            | "telecom"
            | "extension"
            | "contained"
            | "contact"
            | "qualification"
            | "location"
            | "healthcareService"
            | "endpoint"
            | "alias"
            | "type"
            | "specialty"
            | "availableTime"
            | "notAvailable"
            | "communication"
            | "category"
            | "language"
            | "referralMethod"
            | "practiceSetting"
            | "coverageArea"
            | "serviceType"
            | "eligibility"
            | "program"
            | "characteristic"
            | "annotation"
            | "note"
            | "photo"
            | "review"
            | "usage"
            | "coverage"
            | "plan"
            | "guarantor"
            | "network"
            | "resource"
            | "entry"
            | "link"
            | "outcome"
            | "issue"
            | "coding"
            | "given"
            | "line"
    ) {
        return true;
    }

    // Fields that are 0..* only for specific resource types
    match (resource_type, field_name) {
        ("Patient" | "Person" | "Practitioner" | "RelatedPerson", "name") => true,
        ("Organization" | "HealthcareService" | "Location", "name") => false,
        ("Organization" | "Location" | "HealthcareService" | "PractitionerRole", "address") => true,
        ("Patient" | "Person" | "Practitioner" | "RelatedPerson", "address") => true,
        _ => false,
    }
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
fn generate_typed_value(type_code: &str, target_profiles: &[String]) -> serde_json::Value {
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
            "state": "NSW",
            "postalCode": "2000",
            "country": "AU"
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
                let ref_type = profile.rsplit('/').next().unwrap_or("Resource");
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

        // BackboneElement: start with empty object, will be populated by
        // populate_backbone_fields with required sub-fields
        "BackboneElement" => serde_json::json!({}),

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
                                "http://hl7.org/fhir/StructureDefinition/Patient".to_string()
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
        let subject = &resource["subject"];
        assert_eq!(
            subject["reference"], "placeholder:Patient",
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
                    },
                    ElementDefinition {
                        id: "Practitioner.qualification".to_string(),
                        path: "Practitioner.qualification".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "BackboneElement".to_string(),
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
                    ElementDefinition {
                        id: "Practitioner.qualification.identifier".to_string(),
                        path: "Practitioner.qualification.identifier".to_string(),
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
                        id: "Practitioner.qualification.code".to_string(),
                        path: "Practitioner.qualification.code".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "CodeableConcept".to_string(),
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
        };

        let resource = generate_resource(&profile).unwrap();

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
                    },
                    ElementDefinition {
                        id: "Patient.extension".to_string(),
                        path: "Patient.extension".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Extension".to_string(),
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
        };

        let resource = generate_resource(&profile).unwrap();

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
                    },
                    ElementDefinition {
                        id: "Organization.identifier".to_string(),
                        path: "Organization.identifier".to_string(),
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
                        id: "Organization.name".to_string(),
                        path: "Organization.name".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "string".to_string(),
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
                    ElementDefinition {
                        id: "Organization.address".to_string(),
                        path: "Organization.address".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Address".to_string(),
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
        };

        let resource = generate_resource(&profile).unwrap();

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
                    },
                    ElementDefinition {
                        id: "Practitioner.qualification".to_string(),
                        path: "Practitioner.qualification".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "BackboneElement".to_string(),
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
                    ElementDefinition {
                        id: "Practitioner.qualification.code".to_string(),
                        path: "Practitioner.qualification.code".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "CodeableConcept".to_string(),
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
                    ElementDefinition {
                        id: "Practitioner.qualification.code.text".to_string(),
                        path: "Practitioner.qualification.code.text".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "string".to_string(),
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
        };

        let resource = generate_resource(&profile).unwrap();

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
        assert_eq!(
            text.as_str().unwrap(),
            "generated-string",
            "code.text should be populated (required at depth 2)"
        );
    }
}
