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
            // When a code type has a required binding (e.g. daysOfWeek bound to
            // DaysOfWeek value set), use a valid code from that system instead
            // of the generic "active" default which HAPI rejects.
            if let Some(system) = bound_system {
                code_value_for_system(&system)
            } else if element.path.ends_with("daysOfWeek") {
                // Fallback: even if the DaysOfWeek ValueSet isn't in the IG
                // package, daysOfWeek only accepts mon/tue/wed/thu/fri/sat/sun.
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
            "value": format!("urn:uuid:{}", uuid::Uuid::new_v4())
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
                    "code": "unknown",
                    "display": "Unknown"
                })
            } else if element.path.ends_with("Endpoint.connectionType") {
                serde_json::json!({
                    "system": "http://terminology.hl7.org/CodeSystem/endpoint-connection-type",
                    "code": "hl7-fhir-rest",
                    "display": "HL7 FHIR REST"
                })
            } else {
                serde_json::json!({
                    "system": "http://terminology.hl7.org/CodeSystem/v3-NullFlavor",
                    "code": "UNK",
                    "display": "unknown"
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

/// Return a valid code value for a known bound system.
///
/// When a `code`-typed element has a required binding (e.g. `daysOfWeek`
/// bound to `http://hl7.org/fhir/ValueSet/days-of-week`), the generic
/// default `"active"` is invalid and HAPI rejects it. This function
/// returns a valid code from the bound system.
fn code_value_for_system(system: &str) -> serde_json::Value {
    match system {
        "http://hl7.org/fhir/days-of-week" => serde_json::json!("mon"),
        "http://hl7.org/fhir/administrative-gender" => serde_json::json!("male"),
        _ => serde_json::json!("active"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── generate_typed_value ───────────────────────────────────────────

    #[test]
    fn test_generate_typed_value_primitives() {
        let el = ElementDefinition {
            id: "test".into(),
            path: "test".into(),
            min: Some(0),
            max: Some("1".into()),
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
        let vs = HashMap::new();

        assert_eq!(
            generate_typed_value("string", &[], &el, &vs),
            json!("Generated string")
        );
        assert_eq!(
            generate_typed_value("uri", &[], &el, &vs),
            json!("urn:ietf:rfc:3986")
        );
        assert_eq!(
            generate_typed_value("url", &[], &el, &vs),
            json!("https://example.org/fhir/resource")
        );
        assert_eq!(generate_typed_value("boolean", &[], &el, &vs), json!(true));
        assert_eq!(generate_typed_value("integer", &[], &el, &vs), json!(1));
        assert_eq!(generate_typed_value("decimal", &[], &el, &vs), json!(1.0));
        assert_eq!(
            generate_typed_value("date", &[], &el, &vs),
            json!("2024-01-01")
        );
        assert_eq!(
            generate_typed_value("dateTime", &[], &el, &vs),
            json!("2024-01-01T00:00:00Z")
        );
        assert_eq!(
            generate_typed_value("instant", &[], &el, &vs),
            json!("2024-01-01T00:00:00Z")
        );
        assert_eq!(
            generate_typed_value("time", &[], &el, &vs),
            json!("00:00:00")
        );
        assert_eq!(generate_typed_value("unsignedInt", &[], &el, &vs), json!(1));
        assert_eq!(generate_typed_value("positiveInt", &[], &el, &vs), json!(1));
        assert_eq!(
            generate_typed_value("base64Binary", &[], &el, &vs),
            json!("")
        );
        assert_eq!(
            generate_typed_value("markdown", &[], &el, &vs),
            json!("Generated text")
        );
        assert_eq!(
            generate_typed_value("oid", &[], &el, &vs),
            json!("urn:oid:2.16.840.1.113883.19.5")
        );
    }

    #[test]
    fn test_generate_typed_value_code_default() {
        let el = ElementDefinition {
            id: "test".into(),
            path: "test".into(),
            min: Some(0),
            max: Some("1".into()),
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
        let vs = HashMap::new();
        assert_eq!(generate_typed_value("code", &[], &el, &vs), json!("active"));
    }

    #[test]
    fn test_generate_typed_value_code_with_bound_system() {
        let mut vs = HashMap::new();
        vs.insert(
            "http://hl7.org/fhir/ValueSet/days-of-week".into(),
            "http://hl7.org/fhir/days-of-week".into(),
        );
        let el = ElementDefinition {
            id: "test.daysOfWeek".into(),
            path: "test.daysOfWeek".into(),
            min: Some(0),
            max: Some("1".into()),
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
            binding: Some(ElementBinding {
                strength: "required".into(),
                value_set: Some("http://hl7.org/fhir/ValueSet/days-of-week".into()),
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
        assert_eq!(generate_typed_value("code", &[], &el, &vs), json!("mon"));
    }

    #[test]
    fn test_generate_typed_value_code_days_of_week_fallback() {
        let el = ElementDefinition {
            id: "test.daysOfWeek".into(),
            path: "test.daysOfWeek".into(),
            min: Some(0),
            max: Some("1".into()),
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
        let vs = HashMap::new();
        assert_eq!(generate_typed_value("code", &[], &el, &vs), json!("mon"));
    }

    #[test]
    fn test_generate_typed_value_complex_types() {
        let el = ElementDefinition {
            id: "test".into(),
            path: "test".into(),
            min: Some(0),
            max: Some("1".into()),
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
        let vs = HashMap::new();

        let val = generate_typed_value("Identifier", &[], &el, &vs);
        assert!(val.get("system").is_some());
        assert!(val.get("value").is_some());

        let val = generate_typed_value("HumanName", &[], &el, &vs);
        assert_eq!(val["family"], "Smith");

        let val = generate_typed_value("ContactPoint", &[], &el, &vs);
        assert_eq!(val["system"], "phone");

        let val = generate_typed_value("CodeableConcept", &[], &el, &vs);
        assert!(val.get("coding").is_some());

        let val = generate_typed_value("Coding", &[], &el, &vs);
        assert!(val.get("code").is_some());

        let val = generate_typed_value("Quantity", &[], &el, &vs);
        assert_eq!(val["value"], 1.0);

        let val = generate_typed_value("Period", &[], &el, &vs);
        assert!(val.get("start").is_some());

        let val = generate_typed_value("Attachment", &[], &el, &vs);
        assert_eq!(val["contentType"], "text/plain");

        let val = generate_typed_value("Annotation", &[], &el, &vs);
        assert_eq!(val["text"], "Generated annotation");

        let val = generate_typed_value("Range", &[], &el, &vs);
        assert!(val.get("low").is_some());

        let val = generate_typed_value("Ratio", &[], &el, &vs);
        assert!(val.get("numerator").is_some());

        let val = generate_typed_value("Timing", &[], &el, &vs);
        assert!(val.get("repeat").is_some());

        let val = generate_typed_value("SampledData", &[], &el, &vs);
        assert_eq!(val["dimensions"], 1);

        let val = generate_typed_value("BackboneElement", &[], &el, &vs);
        assert_eq!(val, json!({}));

        let val = generate_typed_value("UnknownType", &[], &el, &vs);
        assert_eq!(val, json!({}));
    }

    #[test]
    fn test_generate_typed_value_reference_with_profile() {
        let el = ElementDefinition {
            id: "test".into(),
            path: "test".into(),
            min: Some(0),
            max: Some("1".into()),
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
        let vs = HashMap::new();
        let val = generate_typed_value(
            "Reference",
            &["http://example.org/StructureDefinition/MyPatient".into()],
            &el,
            &vs,
        );
        // reference_type_from_target extracts "MyPatient" from the profile URL
        assert!(
            val.get("reference")
                .unwrap()
                .as_str()
                .unwrap()
                .starts_with("MyPatient/")
        );
    }

    #[test]
    fn test_generate_typed_value_codeable_concept_with_bound_system() {
        let mut vs = HashMap::new();
        vs.insert(
            "http://hl7.org/fhir/ValueSet/administrative-gender".into(),
            "http://hl7.org/fhir/administrative-gender".into(),
        );
        let el = ElementDefinition {
            id: "test".into(),
            path: "test".into(),
            min: Some(0),
            max: Some("1".into()),
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
            binding: Some(ElementBinding {
                strength: "required".into(),
                value_set: Some("http://hl7.org/fhir/ValueSet/administrative-gender".into()),
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
        let val = generate_typed_value("CodeableConcept", &[], &el, &vs);
        assert_eq!(
            val["coding"][0]["system"],
            "http://hl7.org/fhir/administrative-gender"
        );
    }

    // ── capitalize_fhir_type ───────────────────────────────────────────

    #[test]
    fn test_capitalize_fhir_type() {
        assert_eq!(capitalize_fhir_type("string"), "String");
        assert_eq!(capitalize_fhir_type("boolean"), "Boolean");
        assert_eq!(capitalize_fhir_type("markdown"), "Markdown");
        assert_eq!(capitalize_fhir_type("CodeableConcept"), "CodeableConcept");
        assert_eq!(capitalize_fhir_type(""), "");
        assert_eq!(capitalize_fhir_type("a"), "A");
    }

    // ── is_complex_type ────────────────────────────────────────────────

    #[test]
    fn test_is_complex_type_true() {
        assert!(is_complex_type("Identifier"));
        assert!(is_complex_type("HumanName"));
        assert!(is_complex_type("Address"));
        assert!(is_complex_type("ContactPoint"));
        assert!(is_complex_type("CodeableConcept"));
        assert!(is_complex_type("Coding"));
        assert!(is_complex_type("Quantity"));
        assert!(is_complex_type("Reference"));
        assert!(is_complex_type("Period"));
        assert!(is_complex_type("Attachment"));
        assert!(is_complex_type("Annotation"));
        assert!(is_complex_type("Range"));
        assert!(is_complex_type("Ratio"));
        assert!(is_complex_type("Timing"));
        assert!(is_complex_type("SampledData"));
        assert!(is_complex_type("BackboneElement"));
    }

    #[test]
    fn test_is_complex_type_false() {
        assert!(!is_complex_type("string"));
        assert!(!is_complex_type("boolean"));
        assert!(!is_complex_type("integer"));
        assert!(!is_complex_type("date"));
        assert!(!is_complex_type("code"));
        assert!(!is_complex_type("uri"));
    }

    // ── is_base_spec_repeatable ────────────────────────────────────────

    #[test]
    fn test_is_base_spec_repeatable_always_true() {
        assert!(is_base_spec_repeatable("Patient", "identifier"));
        assert!(is_base_spec_repeatable("Patient", "telecom"));
        assert!(is_base_spec_repeatable("Patient", "extension"));
        assert!(is_base_spec_repeatable("Patient", "contained"));
        assert!(is_base_spec_repeatable("Patient", "contact"));
        assert!(is_base_spec_repeatable("Patient", "coding"));
        assert!(is_base_spec_repeatable("Patient", "given"));
        assert!(is_base_spec_repeatable("Patient", "line"));
    }

    #[test]
    fn test_is_base_spec_repeatable_resource_specific() {
        assert!(is_base_spec_repeatable("Patient", "name"));
        assert!(is_base_spec_repeatable("Person", "name"));
        assert!(is_base_spec_repeatable("Practitioner", "name"));
        assert!(is_base_spec_repeatable("RelatedPerson", "name"));
        assert!(!is_base_spec_repeatable("Organization", "name"));
        assert!(!is_base_spec_repeatable("Location", "name"));

        assert!(is_base_spec_repeatable("Organization", "address"));
        assert!(is_base_spec_repeatable("Patient", "address"));
        assert!(!is_base_spec_repeatable("Location", "address"));

        assert!(is_base_spec_repeatable("PractitionerRole", "code"));
        assert!(is_base_spec_repeatable("HealthcareService", "code"));

        assert!(is_base_spec_repeatable("Provenance", "target"));
        assert!(is_base_spec_repeatable("Provenance", "agent"));
    }

    #[test]
    fn test_is_base_spec_repeatable_false() {
        assert!(!is_base_spec_repeatable("Patient", "birthDate"));
        assert!(!is_base_spec_repeatable("Patient", "gender"));
        assert!(!is_base_spec_repeatable("Observation", "status"));
    }

    // ── get_field_name ─────────────────────────────────────────────────

    #[test]
    fn test_get_field_name_direct_child() {
        assert_eq!(
            get_field_name("Patient.name", "Patient"),
            Some("name".into())
        );
        assert_eq!(
            get_field_name("Patient.birthDate", "Patient"),
            Some("birthDate".into())
        );
    }

    #[test]
    fn test_get_field_name_strips_slice_notation() {
        assert_eq!(
            get_field_name("Patient.identifier:type", "Patient"),
            Some("identifier".into())
        );
    }

    #[test]
    fn test_get_field_name_wrong_resource_type() {
        assert_eq!(get_field_name("Patient.name", "Observation"), None);
    }

    #[test]
    fn test_get_field_name_nested_path() {
        assert_eq!(get_field_name("Patient.name.family", "Patient"), None);
    }

    #[test]
    fn test_get_field_name_root() {
        assert_eq!(get_field_name("Patient", "Patient"), Some("Patient".into()));
    }

    #[test]
    fn test_get_field_name_no_dot_after_prefix() {
        assert_eq!(get_field_name("PatientX", "Patient"), None);
    }

    // ── has_slices_for_path ────────────────────────────────────────────

    #[test]
    fn test_has_slices_for_path_true() {
        let elements = vec![ElementDefinition {
            id: "Patient.identifier:ABN".into(),
            path: "Patient.identifier".into(),
            slice_name: Some("ABN".into()),
            ..Default::default()
        }];
        assert!(has_slices_for_path(&elements, "Patient.identifier"));
    }

    #[test]
    fn test_has_slices_for_path_false() {
        let elements = vec![ElementDefinition {
            id: "Patient.name".into(),
            path: "Patient.name".into(),
            slice_name: None,
            ..Default::default()
        }];
        assert!(!has_slices_for_path(&elements, "Patient.name"));
    }

    // ── reference_type_from_target ──────────────────────────────────────

    #[test]
    fn test_reference_type_from_target() {
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/Practitioner"),
            "Practitioner"
        );
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/PractitionerRole"),
            "PractitionerRole"
        );
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/HealthcareService"),
            "HealthcareService"
        );
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/Organization"),
            "Organization"
        );
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/Location"),
            "Location"
        );
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/Endpoint"),
            "Endpoint"
        );
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/Provenance"),
            "Provenance"
        );
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/Parameters"),
            "Parameters"
        );
        assert_eq!(reference_type_from_target("Patient"), "Patient");
        assert_eq!(reference_type_from_target(""), "");
    }

    #[test]
    fn test_reference_type_from_target_strips_version() {
        assert_eq!(
            reference_type_from_target("http://example.org/StructureDefinition/Patient|1.0"),
            "Patient"
        );
    }

    // ── is_generic_identifier_system ───────────────────────────────────

    #[test]
    fn test_is_generic_identifier_system() {
        assert!(is_generic_identifier_system(
            "http://example.org/identifier"
        ));
        assert!(is_generic_identifier_system("urn:ietf:rfc:3986"));
        assert!(!is_generic_identifier_system(
            "http://hl7.org/fhir/sid/us-ssn"
        ));
        assert!(!is_generic_identifier_system(""));
    }

    // ── find_identifier_system ─────────────────────────────────────────

    #[test]
    fn test_find_identifier_system_from_snapshot() {
        let profiles = vec![StructureDefinition {
            url: "http://example.org/Profile/MyIdentifier".into(),
            base_type: "Identifier".into(),
            snapshot: Some(Snapshot {
                element: vec![ElementDefinition {
                    id: "Identifier.system".into(),
                    path: "Identifier.system".into(),
                    fixed_uri: Some("http://example.org/sys/my-id".into()),
                    ..Default::default()
                }],
            }),
            differential: None,
            ..Default::default()
        }];
        let result = find_identifier_system("http://example.org/Profile/MyIdentifier", &profiles);
        assert_eq!(result, Some("http://example.org/sys/my-id".into()));
    }

    #[test]
    fn test_find_identifier_system_from_differential() {
        let profiles = vec![StructureDefinition {
            url: "http://example.org/Profile/MyIdentifier".into(),
            base_type: "Identifier".into(),
            snapshot: None,
            differential: Some(Differential {
                element: vec![ElementDefinition {
                    id: "Identifier.system".into(),
                    path: "Identifier.system".into(),
                    pattern_uri: Some("http://example.org/sys/pattern".into()),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        }];
        let result = find_identifier_system("http://example.org/Profile/MyIdentifier", &profiles);
        assert_eq!(result, Some("http://example.org/sys/pattern".into()));
    }

    #[test]
    fn test_find_identifier_system_not_found() {
        let result = find_identifier_system("http://example.org/Profile/Nonexistent", &[]);
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_identifier_system_strips_version() {
        let profiles = vec![StructureDefinition {
            url: "http://example.org/Profile/MyIdentifier".into(),
            base_type: "Identifier".into(),
            snapshot: Some(Snapshot {
                element: vec![ElementDefinition {
                    id: "Identifier.system".into(),
                    path: "Identifier.system".into(),
                    fixed_uri: Some("http://example.org/sys/my-id".into()),
                    ..Default::default()
                }],
            }),
            differential: None,
            ..Default::default()
        }];
        let result =
            find_identifier_system("http://example.org/Profile/MyIdentifier|1.0", &profiles);
        assert_eq!(result, Some("http://example.org/sys/my-id".into()));
    }

    // ── find_slice_system ──────────────────────────────────────────────

    #[test]
    fn test_find_slice_system_found() {
        let elements = vec![ElementDefinition {
            id: "Patient.identifier:ABN.system".into(),
            path: "Patient.identifier:ABN.system".into(),
            fixed_uri: Some("http://example.org/sys/abn".into()),
            ..Default::default()
        }];
        let result = find_slice_system("ABN", &elements);
        assert_eq!(result, Some("http://example.org/sys/abn".into()));
    }

    #[test]
    fn test_find_slice_system_not_found() {
        let elements = vec![];
        let result = find_slice_system("ABN", &elements);
        assert_eq!(result, None);
    }

    // ── find_identifier_type ─────────────────────────────────────────────

    #[test]
    fn test_find_identifier_type_found() {
        let profiles = vec![StructureDefinition {
            url: "http://example.org/Profile/MyIdentifier".into(),
            base_type: "Identifier".into(),
            snapshot: Some(Snapshot {
                element: vec![ElementDefinition {
                    id: "Identifier.type".into(),
                    path: "Identifier.type".into(),
                    pattern_codeable_concept: Some(json!({"coding": [{"code": "XX"}]})),
                    ..Default::default()
                }],
            }),
            differential: None,
            ..Default::default()
        }];
        let result = find_identifier_type("http://example.org/Profile/MyIdentifier", &profiles);
        assert!(result.is_some());
    }

    #[test]
    fn test_find_identifier_type_not_found() {
        let result = find_identifier_type("http://example.org/Profile/Nonexistent", &[]);
        assert_eq!(result, None);
    }

    // ── find_human_name_use ─────────────────────────────────────────────

    #[test]
    fn test_find_human_name_use_found() {
        let elements = vec![ElementDefinition {
            id: "Patient.name:official.use".into(),
            path: "Patient.name:official.use".into(),
            fixed_code: Some("official".into()),
            ..Default::default()
        }];
        let result = find_human_name_use("official", &elements);
        assert_eq!(result, Some("official".into()));
    }

    #[test]
    fn test_find_human_name_use_not_found() {
        let elements = vec![];
        let result = find_human_name_use("official", &elements);
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_human_name_use_pattern_code() {
        let elements = vec![ElementDefinition {
            id: "Patient.name:official.use".into(),
            path: "Patient.name:official.use".into(),
            pattern_code: Some("official".into()),
            ..Default::default()
        }];
        let result = find_human_name_use("official", &elements);
        assert_eq!(result, Some("official".into()));
    }

    // ── resolve_slice_type_code ─────────────────────────────────────────

    #[test]
    fn test_resolve_slice_type_code_from_slice() {
        let slice = ElementDefinition {
            id: "Patient.identifier:ABN".into(),
            path: "Patient.identifier".into(),
            type_: vec![ElementDefinitionType {
                code: "Identifier".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        };
        let result = resolve_slice_type_code(&slice, &[]);
        assert_eq!(result, Some("Identifier".into()));
    }

    #[test]
    fn test_resolve_slice_type_code_from_base() {
        let slice = ElementDefinition {
            id: "Patient.identifier:ABN".into(),
            path: "Patient.identifier".into(),
            type_: vec![],
            ..Default::default()
        };
        let elements = vec![ElementDefinition {
            id: "Patient.identifier".into(),
            path: "Patient.identifier".into(),
            type_: vec![ElementDefinitionType {
                code: "Identifier".into(),
                profile: vec![],
                target_profile: vec![],
                versioning: None,
            }],
            ..Default::default()
        }];
        let result = resolve_slice_type_code(&slice, &elements);
        assert_eq!(result, Some("Identifier".into()));
    }

    #[test]
    fn test_resolve_slice_type_code_not_found() {
        let slice = ElementDefinition {
            id: "Patient.identifier:ABN".into(),
            path: "Patient.identifier".into(),
            type_: vec![],
            ..Default::default()
        };
        let result = resolve_slice_type_code(&slice, &[]);
        assert_eq!(result, None);
    }

    // ── direct_fixed_or_pattern_value ──────────────────────────────────

    #[test]
    fn test_direct_fixed_or_pattern_value_fixed_string() {
        let el = ElementDefinition {
            fixed_string: Some("hello".into()),
            ..Default::default()
        };
        assert_eq!(direct_fixed_or_pattern_value(&el), Some(json!("hello")));
    }

    #[test]
    fn test_direct_fixed_or_pattern_value_fixed_code() {
        let el = ElementDefinition {
            fixed_code: Some("active".into()),
            ..Default::default()
        };
        assert_eq!(direct_fixed_or_pattern_value(&el), Some(json!("active")));
    }

    #[test]
    fn test_direct_fixed_or_pattern_value_fixed_uri() {
        let el = ElementDefinition {
            fixed_uri: Some("http://example.org".into()),
            ..Default::default()
        };
        assert_eq!(
            direct_fixed_or_pattern_value(&el),
            Some(json!("http://example.org"))
        );
    }

    #[test]
    fn test_direct_fixed_or_pattern_value_fixed_boolean() {
        let el = ElementDefinition {
            fixed_boolean: Some(true),
            ..Default::default()
        };
        assert_eq!(direct_fixed_or_pattern_value(&el), Some(json!(true)));
    }

    #[test]
    fn test_direct_fixed_or_pattern_value_fixed_integer() {
        let el = ElementDefinition {
            fixed_integer: Some(42),
            ..Default::default()
        };
        assert_eq!(direct_fixed_or_pattern_value(&el), Some(json!(42)));
    }

    #[test]
    fn test_direct_fixed_or_pattern_value_pattern_string() {
        let el = ElementDefinition {
            pattern_string: Some("pattern".into()),
            ..Default::default()
        };
        assert_eq!(direct_fixed_or_pattern_value(&el), Some(json!("pattern")));
    }

    #[test]
    fn test_direct_fixed_or_pattern_value_pattern_boolean() {
        let el = ElementDefinition {
            pattern_boolean: Some(false),
            ..Default::default()
        };
        assert_eq!(direct_fixed_or_pattern_value(&el), Some(json!(false)));
    }

    #[test]
    fn test_direct_fixed_or_pattern_value_none() {
        let el = ElementDefinition::default();
        assert_eq!(direct_fixed_or_pattern_value(&el), None);
    }

    // ── code_value_for_system ───────────────────────────────────────────

    #[test]
    fn test_code_value_for_system_days_of_week() {
        assert_eq!(
            code_value_for_system("http://hl7.org/fhir/days-of-week"),
            json!("mon")
        );
    }

    #[test]
    fn test_code_value_for_system_gender() {
        assert_eq!(
            code_value_for_system("http://hl7.org/fhir/administrative-gender"),
            json!("male")
        );
    }

    #[test]
    fn test_code_value_for_system_unknown() {
        assert_eq!(
            code_value_for_system("http://unknown.system"),
            json!("active")
        );
    }
}
