use crate::generate::locality::random_au_locality_thread;
use crate::generate::resource_generator::valuesets::bound_system_for_element;
use crate::model::*;
use std::collections::HashMap;

/// Generate a minimal valid value for a given FHIR type code.
pub fn generate_typed_value(
    type_code: &str,
    target_profiles: &[String],
    element: &ElementDefinition,
    value_set_systems: &HashMap<String, String>,
) -> serde_json::Value {
    let bound_system = bound_system_for_element(element, value_set_systems);

    match type_code {
        // Primitive types
        "string" => serde_json::json!("Generated string"),
        "uri" => serde_json::json!("urn:ietf:rfc:3986"),
        "url" => serde_json::json!("https://example.org/fhir/resource"),
        "canonical" => {
            serde_json::json!("https://example.org/fhir/StructureDefinition/example")
        }
        "code" => {
            // daysOfWeek only accepts mon/tue/wed/thu/fri/sat/sun, not generic "active"
            if element.path.ends_with("daysOfWeek") {
                serde_json::json!("mon")
            } else {
                serde_json::json!("active")
            }
        }
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
        "markdown" => serde_json::json!("Generated text"),
        "oid" => serde_json::json!("urn:oid:2.16.840.1.113883.19.5"),
        "uuid" => serde_json::Value::String(format!("urn:uuid:{}", uuid::Uuid::new_v4())),

        // Complex types
        "Identifier" => serde_json::json!({
            "system": "urn:ietf:rfc:3986",
            "value": uuid::Uuid::new_v4().to_string()
        }),
        "HumanName" => serde_json::json!({
            "family": "Smith",
            "given": ["Alex"]
        }),
        "Address" => {
            let loc = random_au_locality_thread();
            serde_json::json!({
                "line": ["1 Example St"],
                "city": loc.city,
                "state": loc.state,
                "postalCode": loc.postcode,
                "country": "AU"
            })
        }
        "ContactPoint" => serde_json::json!({
            "system": "phone",
            "value": "555-0000"
        }),
        "CodeableConcept" => {
            if let Some(system) = bound_system {
                serde_json::json!({
                    "coding": [{
                        "system": system,
                        "code": "unknown"
                    }],
                    "text": "unknown"
                })
            } else {
                // Always include at least one coding — profiles commonly require
                // CodeableConcept.coding with min = 1 (e.g. suppressed.suppressedBy).
                serde_json::json!({
                    "coding": [{
                        "system": "http://terminology.hl7.org/CodeSystem/v3-NullFlavor",
                        "code": "UNK"
                    }],
                    "text": "unknown"
                })
            }
        }
        "Coding" => {
            if let Some(system) = bound_system {
                serde_json::json!({
                    "system": system,
                    "code": "unknown"
                })
            } else if element.path.ends_with("Endpoint.connectionType") {
                serde_json::json!({
                    "system": "http://terminology.hl7.org/CodeSystem/endpoint-connection-type",
                    "code": "hl7-fhir-rest"
                })
            } else {
                serde_json::json!({
                    "system": "http://terminology.hl7.org/CodeSystem/v3-NullFlavor",
                    "code": "UNK"
                })
            }
        }
        "Quantity" => serde_json::json!({
            "value": 1.0,
            "unit": "1",
            "system": "http://unitsofmeasure.org",
            "code": "1"
        }),
        "Reference" => {
            if let Some(profile) = target_profiles.first() {
                // Extract resource type from profile URL
                let ref_type = profile.rsplit('/').next().unwrap_or("Resource");
                serde_json::json!({
                    "reference": format!("{}/{}", reference_type_from_target(ref_type), uuid::Uuid::new_v4())
                })
            } else {
                serde_json::json!({
                    "reference": format!("Resource/{}", uuid::Uuid::new_v4())
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

/// Capitalize the first letter of a FHIR type code to produce a valid value[x] key.
///
/// FHIR JSON requires PascalCase for the type suffix in value[x] properties:
///   markdown  → valueMarkdown
///   boolean   → valueBoolean
///   Period    → valuePeriod  (already PascalCase — unchanged)
pub fn capitalize_fhir_type(type_code: &str) -> String {
    let mut chars = type_code.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Returns true for FHIR complex types that can have sub-fields.
pub fn is_complex_type(type_code: &str) -> bool {
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
pub fn is_base_spec_repeatable(resource_type: &str, field_name: &str) -> bool {
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
        (
            "Organization" | "Practitioner" | "Patient" | "Person" | "RelatedPerson"
            | "PractitionerRole",
            "address",
        ) => true,
        ("Location", "address") => false,
        ("PractitionerRole", "code") => true,
        ("HealthcareService", "code") => true,
        ("Provenance", "target") => true,
        ("Provenance", "agent") => true,
        _ => false,
    }
}

/// Extract the field name from a FHIR path like "Patient.name" → "name".
/// Returns None for paths that don't belong to this resource type or are nested.
pub fn get_field_name(path: &str, resource_type: &str) -> Option<String> {
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

/// Check if a given path has slice definitions.
pub fn has_slices_for_path(elements: &[ElementDefinition], field_path: &str) -> bool {
    elements
        .iter()
        .any(|e| e.path == field_path && e.slice_name.is_some())
}

/// Convert a reference target string to a FHIR resource type name.
pub fn reference_type_from_target(target: &str) -> String {
    let clean = target.split('|').next().unwrap_or(target);
    let tail = clean.rsplit('/').next().unwrap_or(clean).to_lowercase();

    if tail.contains("practitionerrole") {
        "PractitionerRole".to_string()
    } else if tail.contains("practitioner") {
        "Practitioner".to_string()
    } else if tail.contains("healthcareservice") {
        "HealthcareService".to_string()
    } else if tail.contains("organization") {
        "Organization".to_string()
    } else if tail.contains("location") {
        "Location".to_string()
    } else if tail.contains("endpoint") {
        "Endpoint".to_string()
    } else if tail.contains("provenance") {
        "Provenance".to_string()
    } else if tail.contains("parameters") {
        "Parameters".to_string()
    } else if let Some(first) = clean.rsplit('/').next() {
        first.to_string()
    } else {
        "Resource".to_string()
    }
}

/// Check if a system string is a generic placeholder identifier system.
pub fn is_generic_identifier_system(system: &str) -> bool {
    matches!(
        system,
        "http://example.org/identifier" | "urn:ietf:rfc:3986"
    )
}

/// Find the fixed/pattern system URI for an Identifier profile.
pub fn find_identifier_system(
    profile_url: &str,
    all_profiles: &[StructureDefinition],
) -> Option<String> {
    let clean_url = profile_url.split('|').next().unwrap_or(profile_url);

    let profile = all_profiles.iter().find(|p| p.url == clean_url)?;

    let elements = match (&profile.snapshot, &profile.differential) {
        (Some(snapshot), _) => &snapshot.element,
        (None, Some(diff)) => &diff.element,
        _ => return None,
    };

    for el in elements {
        if el.id.ends_with(".system") || el.path.ends_with(".system") {
            if let Some(v) = &el.fixed_uri {
                return Some(v.clone());
            }

            if let Some(v) = &el.pattern_uri {
                return Some(v.clone());
            }
        }
    }

    None
}

/// Find the fixed/pattern system URI for a named slice.
pub fn find_slice_system(slice_name: &str, elements: &[ElementDefinition]) -> Option<String> {
    for el in elements {
        let matches_slice = el.path.contains(&format!(":{}.", slice_name))
            || el.id.contains(&format!(":{}.", slice_name));

        if !matches_slice {
            continue;
        }

        if el.id.ends_with(".system") || el.path.ends_with(".system") {
            if let Some(v) = &el.fixed_uri {
                return Some(v.clone());
            }

            if let Some(v) = &el.pattern_uri {
                return Some(v.clone());
            }
        }
    }

    None
}

/// Apply Identifier profile constraints (system, type) to a value.
pub fn apply_identifier_profile_constraints(
    value: &mut serde_json::Value,
    type_def: &ElementDefinitionType,
    all_profiles: &[StructureDefinition],
) {
    if let Some(obj) = value.as_object_mut() {
        for profile_url in type_def
            .profile
            .iter()
            .chain(type_def.target_profile.iter())
        {
            if (obj.get("system").is_none()
                || obj
                    .get("system")
                    .and_then(|v| v.as_str())
                    .is_some_and(is_generic_identifier_system))
                && let Some(system) = find_identifier_system(profile_url, all_profiles)
            {
                obj.insert("system".to_string(), serde_json::json!(system));
            }

            if !obj.contains_key("type")
                && let Some(identifier_type) = find_identifier_type(profile_url, all_profiles)
            {
                obj.insert("type".to_string(), identifier_type);
            }

            if obj.contains_key("system") && obj.contains_key("type") {
                break;
            }
        }
    }
}

/// Find the fixed/pattern CodeableConcept type for an Identifier profile.
pub fn find_identifier_type(
    profile_url: &str,
    all_profiles: &[StructureDefinition],
) -> Option<serde_json::Value> {
    let clean_url = profile_url.split('|').next().unwrap_or(profile_url);

    let profile = all_profiles.iter().find(|p| p.url == clean_url)?;

    let elements = match (&profile.snapshot, &profile.differential) {
        (Some(snapshot), _) => &snapshot.element,
        (None, Some(diff)) => &diff.element,
        _ => return None,
    };

    for el in elements {
        if el.id.ends_with(".type") || el.path.ends_with(".type") {
            if let Some(v) = &el.pattern_codeable_concept {
                return Some(v.clone());
            }

            if let Some(v) = &el.fixed_codeable_concept {
                return Some(v.clone());
            }
        }
    }

    None
}

/// Find the fixed use code for a HumanName slice.
pub fn find_human_name_use(slice_name: &str, elements: &[ElementDefinition]) -> Option<String> {
    for el in elements {
        // Match elements that belong to this named slice and have a .use path.
        // Element IDs for sliced sub-elements look like:
        //   Practitioner.name:officialName.use  (id contains ":officialName")
        let matches_slice = el.id.contains(&format!(":{}", slice_name))
            || el.path.contains(&format!(":{}", slice_name));

        if !matches_slice {
            continue;
        }

        if el.path.ends_with(".use") || el.id.ends_with(".use") {
            if let Some(v) = &el.fixed_code {
                return Some(v.clone());
            }

            if let Some(v) = &el.pattern_code {
                return Some(v.clone());
            }
        }
    }

    None
}

/// Resolve the type code for a slice element, falling back to the unsliced base element.
pub fn resolve_slice_type_code(
    slice: &ElementDefinition,
    elements: &[ElementDefinition],
) -> Option<String> {
    if let Some(type_def) = slice.type_.first() {
        return Some(type_def.code.clone());
    }

    elements
        .iter()
        .find(|el| {
            el.path == slice.path
                && el.slice_name.is_none()
                && !el.type_.is_empty()
                && !el.id.contains(':')
        })
        .and_then(|el| el.type_.first())
        .map(|t| t.code.clone())
}

/// Return a fixed or pattern value from an element definition, if one exists.
pub fn direct_fixed_or_pattern_value(element: &ElementDefinition) -> Option<serde_json::Value> {
    if let Some(val) = &element.fixed_string {
        return Some(serde_json::Value::String(val.clone()));
    }
    if let Some(val) = &element.fixed_code {
        return Some(serde_json::Value::String(val.clone()));
    }
    if let Some(val) = &element.fixed_uri {
        return Some(serde_json::Value::String(val.clone()));
    }
    if let Some(val) = &element.fixed_boolean {
        return Some(serde_json::Value::Bool(*val));
    }
    if let Some(val) = &element.fixed_integer {
        return Some(serde_json::Value::Number((*val).into()));
    }
    if let Some(val) = &element.fixed_quantity {
        return Some(val.clone());
    }
    if let Some(val) = &element.fixed_coding {
        return Some(val.clone());
    }
    if let Some(val) = &element.fixed_codeable_concept {
        return Some(val.clone());
    }

    if let Some(val) = &element.pattern_string {
        return Some(serde_json::Value::String(val.clone()));
    }
    if let Some(val) = &element.pattern_code {
        return Some(serde_json::Value::String(val.clone()));
    }
    if let Some(val) = &element.pattern_uri {
        return Some(serde_json::Value::String(val.clone()));
    }
    if let Some(val) = &element.pattern_boolean {
        return Some(serde_json::Value::Bool(*val));
    }
    if let Some(val) = &element.pattern_quantity {
        return Some(val.clone());
    }
    if let Some(val) = &element.pattern_coding {
        return Some(val.clone());
    }
    if let Some(val) = &element.pattern_codeable_concept {
        return Some(val.clone());
    }

    None
}
