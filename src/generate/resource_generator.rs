use crate::generate::random_au_locality_thread;
use crate::model::*;
use anyhow::Result;
use std::collections::HashMap;

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
                    let code = concept.get("code").and_then(|c| c.as_str()).map(|s| s.to_string());
                    let display = concept.get("display").and_then(|d| d.as_str()).map(|s| s.to_string());
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

    populate_required_fields(
        &mut resource,
        elements,
        &profile.base_type,
        all_profiles,
        value_set_systems,
    )?;

    // Second pass: populate required slices (e.g., identifier:abn with patternUri)
    populate_required_slices(
        &mut resource,
        elements,
        &profile.base_type,
        all_profiles,
        value_set_systems,
    )?;

    // Third pass: populate extension slices defined by the profile
    // (e.g., HealthcareService.extension:active-period, Location.extension:amenity)
    populate_extension_slices(
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
    populate_must_support_backbones(
        &mut resource,
        elements,
        &profile.base_type,
        all_profiles,
        value_set_systems,
    );

    Ok(resource)
}

/// Populate mustSupport BackboneElement fields that have min=0 but whose children
/// include at least one required field (min ≥ 1).
///
/// The required-fields pass (pass 1) skips optional fields entirely. But when a
/// BackboneElement is mustSupport, a server must be able to store and return it —
/// so the generated test resource should include it so the must_support checker
/// can verify its presence. Only direct children are considered (depth 1 off the
/// backbone), matching the behaviour of `populate_backbone_fields`.
fn populate_must_support_backbones(
    resource: &mut serde_json::Value,
    elements: &[ElementDefinition],
    resource_type: &str,
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
) {
    for element in elements {
        // Only direct children of the resource root (depth 1, no slices)
        let field_name = match get_field_name(&element.path, resource_type) {
            Some(name) => name,
            None => continue,
        };
        if field_name == resource_type || field_name.contains(':') {
            continue;
        }

        // Must be optional (min=0), mustSupport, and a BackboneElement
        if element.min.unwrap_or(0) != 0 {
            continue;
        }
        if !element.must_support {
            continue;
        }
        if element.type_.first().map(|t| t.code.as_str()) != Some("BackboneElement") {
            continue;
        }

        // Skip if the field is already populated
        if resource.get(&field_name).is_some() {
            continue;
        }

        // Check whether any direct child has min ≥ 1 (would be populated in a backbone)
        let parent_path = format!("{}.{}", resource_type, field_name);
        let has_required_child = elements.iter().any(|e| {
            e.path.starts_with(&format!("{}.", parent_path))
                && !e
                    .path
                    .strip_prefix(&format!("{}.", parent_path))
                    .unwrap_or("")
                    .contains('.')
                && e.min.unwrap_or(0) >= 1
        });

        if !has_required_child {
            continue;
        }

        // Generate and populate the backbone
        let mut backbone = serde_json::Map::new();
        populate_backbone_fields(
            &mut backbone,
            &parent_path,
            elements,
            resource_type,
            all_profiles,
            value_set_systems,
        );

        let max = element.max.as_deref().unwrap_or("1");
        if max != "1" || is_base_spec_repeatable(resource_type, &field_name) {
            resource[&field_name] = serde_json::json!([backbone]);
        } else {
            resource[&field_name] = serde_json::json!(backbone);
        }
    }
}

fn populate_required_fields(
    resource: &mut serde_json::Value,
    elements: &[ElementDefinition],
    resource_type: &str,
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
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

        if let Some(val) = direct_fixed_or_pattern_value(element) {
            let max = element.max.as_deref().unwrap_or("1");
            if max != "1" || is_base_spec_repeatable(resource_type, &field_name) {
                resource[&field_name] = serde_json::json!([val]);
            } else {
                resource[&field_name] = val;
            }
            continue;
        }

        // Skip fields with no type information (slice definitions etc.)
        if element.type_.is_empty() {
            continue;
        }

        let type_def = &element.type_[0];
        let type_code = &type_def.code;

        // Skip Extension type — empty extensions are always invalid (violates ext-1:
        // "Must have either extensions or value[x], not both"). Without knowing the
        // extension URL and value, we can't generate a valid extension.
        if type_code == "Extension" {
            continue;
        }

        let target_profiles = &type_def.target_profile;

        let mut value =
            generate_typed_value(type_code, target_profiles, element, value_set_systems);

        if type_code == "Identifier" {
            apply_identifier_profile_constraints(&mut value, type_def, all_profiles);
        }

        // For BackboneElement, populate required sub-fields from nested elements
        if type_code == "BackboneElement" {
            let mut backbone = value.as_object().cloned().unwrap_or_default();
            populate_backbone_fields(
                &mut backbone,
                &element.path,
                elements,
                resource_type,
                all_profiles,
                value_set_systems,
            );
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
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
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

        if let Some(val) = direct_fixed_or_pattern_value(element) {
            let max = element.max.as_deref().unwrap_or("1");
            if max != "1" || is_base_spec_repeatable(resource_type, field_name) {
                backbone.insert(field_name.to_string(), serde_json::json!([val]));
            } else {
                backbone.insert(field_name.to_string(), val);
            }
            continue;
        }

        if element.type_.is_empty() {
            continue;
        }

        let type_def = &element.type_[0];
        let type_code = &type_def.code;
        let target_profiles = &type_def.target_profile;

        if type_code == "Extension" {
            continue;
        }

        let mut value =
            generate_typed_value(type_code, target_profiles, element, value_set_systems);

        if type_code == "Identifier" {
            apply_identifier_profile_constraints(&mut value, type_def, all_profiles);
        }

        let mut value_prewrapped_by_slices = false;

        // For complex types inside a backbone, check for required sub-fields
        // at depth 2 (e.g. qualification.code.text)
        if is_complex_type(type_code) {
            let child_path = format!("{}.{}", parent_path, field_name);
            populate_nested_required_fields(
                &mut value,
                &child_path,
                elements,
                all_profiles,
                value_set_systems,
            );

            if has_slices_for_path(elements, &child_path) {
                value = apply_slices_for_path(
                    value,
                    &child_path,
                    elements,
                    all_profiles,
                    value_set_systems,
                );
                value_prewrapped_by_slices = true;
            }
        }

        let max = element.max.as_deref().unwrap_or("1");
        if max != "1" || is_base_spec_repeatable(resource_type, field_name) {
            if value_prewrapped_by_slices {
                backbone.insert(field_name.to_string(), value);
                continue;
            }
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
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
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

        if let Some(val) = direct_fixed_or_pattern_value(element) {
            let max = element.max.as_deref().unwrap_or("1");
            if max != "1" {
                obj.insert(field_name.to_string(), serde_json::json!([val]));
            } else {
                obj.insert(field_name.to_string(), val);
            }
            continue;
        }

        if element.type_.is_empty() {
            continue;
        }

        let type_def = &element.type_[0];
        let type_code = &type_def.code;
        let target_profiles = &type_def.target_profile;

        if type_code == "Extension" {
            continue;
        }

        let mut child_value =
            generate_typed_value(type_code, target_profiles, element, value_set_systems);

        if type_code == "Identifier" {
            apply_identifier_profile_constraints(&mut child_value, type_def, all_profiles);
        }
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

fn has_slices_for_path(elements: &[ElementDefinition], field_path: &str) -> bool {
    elements
        .iter()
        .any(|e| e.path == field_path && e.slice_name.is_some())
}

fn reference_type_from_target(target: &str) -> String {
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

fn apply_slices_for_path(
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
        if let Some(first_slice) = required_slices.first() {
            if let Some(v) = generate_slice_value(
                first_slice,
                "",
                discriminator_path.as_deref(),
                all_profiles,
                elements,
                value_set_systems,
            ) {
                if slice_values.is_empty() {
                    slice_values.push(v);
                } else {
                    slice_values[0] = v;
                }
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

fn direct_fixed_or_pattern_value(element: &ElementDefinition) -> Option<serde_json::Value> {
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

/// Capitalize the first letter of a FHIR type code to produce a valid value[x] key.
///
/// FHIR JSON requires PascalCase for the type suffix in value[x] properties:
///   markdown  → valueMarkdown
///   boolean   → valueBoolean
///   Period    → valuePeriod  (already PascalCase — unchanged)
fn capitalize_fhir_type(type_code: &str) -> String {
    let mut chars = type_code.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn bound_system_for_element(
    element: &ElementDefinition,
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

/// Generate a minimal valid value for a given FHIR type code.
fn generate_typed_value(
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
        "code" => serde_json::json!("active"),
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

/// Generate slice-aware elements for fields that have required slices.
///
/// For each field with slicing, check if any slice has min > 0.
/// If so, generate an additional element that applies the slice's
/// pattern values (e.g., patternUri on system for identifier slices).
///
/// If no slice is individually required but the field itself is required
/// and has slicing, replace the generic value with one matching the
/// first slice's discriminator pattern so the resource passes validation.
fn populate_required_slices(
    resource: &mut serde_json::Value,
    elements: &[ElementDefinition],
    resource_type: &str,
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
) -> Result<()> {
    // Collect all fields that have slice definitions
    let mut slice_fields: std::collections::HashMap<String, Vec<&ElementDefinition>> =
        std::collections::HashMap::new();

    for element in elements {
        if let Some(ref _slice_name) = element.slice_name {
            let field_name = match get_field_name(&element.path, resource_type) {
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
                let fname = get_field_name(&e.path, resource_type);
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
            if let Some(first_slice) = required_slices.first() {
                if let Some(val) = generate_slice_value(
                    first_slice,
                    resource_type,
                    discriminator_path.as_deref(),
                    all_profiles,
                    elements,
                    value_set_systems,
                ) {
                    // Replace the first generic value
                    if slice_values.is_empty() {
                        slice_values.push(val);
                    } else {
                        slice_values[0] = val;
                    }
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
    if type_code == "HumanName" {
        if let Some(slice_name) = &slice.slice_name {
            if let Some(use_code) = find_human_name_use(slice_name, elements) {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("use".to_string(), serde_json::json!(use_code));
                }
            }
        }
    }

    // Apply pattern values from the slice definition
    if let Some(val) = &slice.pattern_uri {
        if let Some(obj) = value.as_object_mut() {
            if type_code == "Identifier" {
                obj.insert("system".to_string(), serde_json::json!(val));
            } else {
                obj.insert("value".to_string(), serde_json::json!(val));
            }
        }
    }

    if let Some(val) = &slice.pattern_code {
        if let Some(obj) = value.as_object_mut() {
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
    }

    if let Some(val) = &slice.pattern_string {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("value".to_string(), serde_json::json!(val));
        }
    }

    if let Some(val) = &slice.pattern_coding {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("coding".to_string(), val.clone());
        }
    }

    if let Some(val) = &slice.pattern_codeable_concept {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("coding".to_string(), val.clone());
        }
    }

    // Identifier slice handling
    if type_code == "Identifier" {
        if let Some(obj) = value.as_object_mut() {
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

            if let Some(system) = find_identifier_system(profile_url, all_profiles) {
                if !obj.contains_key("system")
                    || obj
                        .get("system")
                        .and_then(|v| v.as_str())
                        .is_some_and(is_generic_identifier_system)
                {
                    obj.insert("system".to_string(), serde_json::json!(system));
                }
            }

            // Some IG slices define Identifier.system at nested paths without
            // repeating a discriminator. Use slice-specific nested constraints
            // as a direct source of truth when available.
            if let Some(slice_name) = &slice.slice_name {
                if let Some(system) = find_slice_system(slice_name, elements) {
                    obj.insert("system".to_string(), serde_json::json!(system));
                }
            }

            if let Some(identifier_type) = find_identifier_type(profile_url, all_profiles) {
                if !obj.contains_key("type") {
                    obj.insert("type".to_string(), identifier_type);
                }
            }

            match discriminator_path {
                Some("system") => {
                    if let Some(slice_name) = &slice.slice_name {
                        if let Some(system) = find_slice_system(slice_name, elements) {
                            obj.insert("system".to_string(), serde_json::json!(system));
                            return Some(value);
                        }
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
    }

    Some(value)
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
fn populate_extension_slices(
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
    let ext_def_map: std::collections::HashMap<&str, &StructureDefinition> = all_profiles
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
                        let fixed_coding = elements.iter().find(|e| {
                            e.id.contains(&format!(":{}", slice_name_str))
                                && e.id.contains(&format!(":{}", sub_name))
                                && (e.id.ends_with(".value[x].coding")
                                    || e.path.ends_with("value[x].coding"))
                        }).and_then(|e| e.fixed_coding.as_ref().or(e.pattern_coding.as_ref()));

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

fn find_identifier_system(
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

fn find_slice_system(slice_name: &str, elements: &[ElementDefinition]) -> Option<String> {
    for el in elements {
        let matches_slice = el.path.contains(&format!(":{}.\"", slice_name))
            || el.id.contains(&format!(":{}.\"", slice_name));

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

fn apply_identifier_profile_constraints(
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
            if obj.get("system").is_none()
                || obj
                    .get("system")
                    .and_then(|v| v.as_str())
                    .is_some_and(is_generic_identifier_system)
            {
                if let Some(system) = find_identifier_system(profile_url, all_profiles) {
                    obj.insert("system".to_string(), serde_json::json!(system));
                }
            }

            if !obj.contains_key("type") {
                if let Some(identifier_type) = find_identifier_type(profile_url, all_profiles) {
                    obj.insert("type".to_string(), identifier_type);
                }
            }

            if obj.contains_key("system") && obj.contains_key("type") {
                break;
            }
        }
    }
}

fn resolve_slice_type_code(
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

fn is_generic_identifier_system(system: &str) -> bool {
    matches!(
        system,
        "http://example.org/identifier" | "urn:ietf:rfc:3986"
    )
}

fn find_identifier_type(
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

fn find_human_name_use(slice_name: &str, elements: &[ElementDefinition]) -> Option<String> {
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
