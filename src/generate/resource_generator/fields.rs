use crate::generate::resource_generator::slices::apply_slices_for_path;
use crate::generate::resource_generator::types::{
    apply_identifier_profile_constraints, generate_typed_value, get_field_name,
    has_slices_for_path, is_base_spec_repeatable, is_complex_type,
};
use crate::model::*;
use anyhow::Result;
use std::collections::HashMap;

/// Populate mustSupport fields with min=0 that are not BackboneElements.
///
/// The required-fields pass (pass 1) only populates fields with min > 0.
/// But many profiles mark optional fields as mustSupport (e.g.
/// Organization.telecom, Location.alias, Provenance.activity). The
/// conformance checker verifies these fields are present in responses,
/// so the generated test resource should include them.
///
/// This pass generates a single default value for each such field.
/// Sub-fields (e.g. telecom.system, telecom.value) are handled by
/// generate_typed_value which populates complex types with defaults.
pub fn populate_must_support_optional_fields(
    resource: &mut serde_json::Value,
    elements: &[ElementDefinition],
    resource_type: &str,
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
) {
    for element in elements {
        let field_name = match get_field_name(&element.path, resource_type) {
            Some(name) => name,
            None => continue,
        };
        if field_name == resource_type || field_name.contains(':') {
            continue;
        }

        // Must be optional (min=0) and mustSupport
        if element.min.unwrap_or(0) != 0 {
            continue;
        }
        if !element.must_support {
            continue;
        }

        // Skip BackboneElements — handled by populate_must_support_backbones
        if element.type_.first().map(|t| t.code.as_str()) == Some("BackboneElement") {
            continue;
        }

        // Skip if the field is already populated
        if resource.get(&field_name).is_some() {
            continue;
        }

        // Skip fields with no type information
        if element.type_.is_empty() {
            continue;
        }

        let type_def = &element.type_[0];
        let type_code = &type_def.code;

        // Skip Extension — can't generate valid extensions without knowing the URL
        if type_code == "Extension" {
            continue;
        }

        let target_profiles = &type_def.target_profile;

        let mut value =
            generate_typed_value(type_code, target_profiles, element, value_set_systems);

        if type_code == "Identifier" {
            apply_identifier_profile_constraints(&mut value, type_def, all_profiles);
        }

        // For complex types inside a mustSupport field, check for required sub-fields
        // at depth 2 (e.g. telecom.system, telecom.value)
        if is_complex_type(type_code) {
            let child_path = format!("{}.{}", resource_type, field_name);
            populate_nested_required_fields(
                &mut value,
                &child_path,
                elements,
                all_profiles,
                value_set_systems,
            );
        }

        let max = element.max.as_deref().unwrap_or("1");
        if max != "1" || is_base_spec_repeatable(resource_type, &field_name) {
            resource[&field_name] = serde_json::json!([value]);
        } else {
            resource[&field_name] = value;
        }
    }
}

/// Populate mustSupport BackboneElement fields that have min=0 but whose children
/// include at least one required field (min ≥ 1).
///
/// The required-fields pass (pass 1) skips optional fields entirely. But when a
/// BackboneElement is mustSupport, a server must be able to store and return it —
/// so the generated test resource should include it so the must_support checker
/// can verify its presence. Only direct children are considered (depth 1 off the
/// backbone), matching the behaviour of `populate_backbone_fields`.
pub fn populate_must_support_backbones(
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

        // Check whether any direct child has min ≥ 1 or is mustSupport.
        // A backbone with all-min=0 children that are mustSupport still needs
        // to be generated so the conformance checker can verify field presence.
        let parent_path = format!("{}.{}", resource_type, field_name);
        let has_relevant_child = elements.iter().any(|e| {
            e.path.starts_with(&format!("{}.", parent_path))
                && !e
                    .path
                    .strip_prefix(&format!("{}.", parent_path))
                    .unwrap_or("")
                    .contains('.')
                && (e.min.unwrap_or(0) >= 1 || e.must_support)
        });

        if !has_relevant_child {
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

/// Populate required fields (min > 0) at depth 1 of the resource.
pub fn populate_required_fields(
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

        if let Some(val) = super::types::direct_fixed_or_pattern_value(element) {
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

        // For complex types (Reference, CodeableConcept, etc.), populate
        // mustSupport sub-fields (e.g. target.extension inside Reference).
        if is_complex_type(type_code) {
            let child_path = format!("{}.{}", resource_type, field_name);
            populate_nested_required_fields(
                &mut value,
                &child_path,
                elements,
                all_profiles,
                value_set_systems,
            );
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
pub fn populate_backbone_fields(
    backbone: &mut serde_json::Map<String, serde_json::Value>,
    parent_path: &str,
    elements: &[ElementDefinition],
    resource_type: &str,
    all_profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
) {
    // Only apply base-spec repeatability rules at the top level
    // (parent_path == resource_type). Nested fields inside backbones
    // (e.g. HealthcareService.eligibility.code) should use the
    // element's own max cardinality, not the top-level field rule.
    let is_top_level = parent_path == resource_type;

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

        if let Some(val) = super::types::direct_fixed_or_pattern_value(element) {
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
        if max != "1" || (is_top_level && is_base_spec_repeatable(resource_type, field_name)) {
            if value_prewrapped_by_slices {
                backbone.insert(field_name.to_string(), value);
                continue;
            }
            backbone.insert(field_name.to_string(), serde_json::json!([value]));
        } else {
            backbone.insert(field_name.to_string(), value);
        }
    }

    // Second pass: populate mustSupport children with min=0.
    // These are optional in the profile but marked mustSupport, so the
    // conformance checker expects them in responses. Examples:
    //   availableTime.daysOfWeek, availableTime.allDay,
    //   availableTime.availableStartTime, availableTime.availableEndTime,
    //   notAvailable.during, eligibility.code
    for element in elements {
        if !element.path.starts_with(&format!("{}.", parent_path)) {
            continue;
        }

        let suffix = element
            .path
            .strip_prefix(&format!("{}.", parent_path))
            .unwrap_or("");
        if suffix.contains('.') {
            continue;
        }

        let field_name = suffix.split(':').next().unwrap_or(suffix);
        if backbone.contains_key(field_name) {
            continue;
        }

        // Must be optional (min=0) and mustSupport
        if element.min.unwrap_or(0) != 0 {
            continue;
        }
        if !element.must_support {
            continue;
        }

        if element.type_.is_empty() {
            continue;
        }

        let type_def = &element.type_[0];
        let type_code = &type_def.code;

        if type_code == "Extension" {
            continue;
        }

        let target_profiles = &type_def.target_profile;
        let mut value =
            generate_typed_value(type_code, target_profiles, element, value_set_systems);

        if type_code == "Identifier" {
            apply_identifier_profile_constraints(&mut value, type_def, all_profiles);
        }

        // For complex types inside a backbone, check for required sub-fields
        if is_complex_type(type_code) {
            let child_path = format!("{}.{}", parent_path, field_name);
            populate_nested_required_fields(
                &mut value,
                &child_path,
                elements,
                all_profiles,
                value_set_systems,
            );
        }

        let max = element.max.as_deref().unwrap_or("1");
        if max != "1" || (is_top_level && is_base_spec_repeatable(resource_type, field_name)) {
            backbone.insert(field_name.to_string(), serde_json::json!([value]));
        } else {
            backbone.insert(field_name.to_string(), value);
        }
    }
}

/// Populate required sub-fields at depth 2 inside a complex type.
/// E.g., for "Practitioner.qualification.code", find
/// "Practitioner.qualification.code.text" (min=1) and populate it.
/// Also populates mustSupport sub-fields with min=0 (e.g.
/// "telecom.extension", "qualification.issuer", "availableTime.daysOfWeek").
pub fn populate_nested_required_fields(
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

    // First pass: populate required children (min > 0)
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

        if let Some(val) = super::types::direct_fixed_or_pattern_value(element) {
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

    // Second pass: populate mustSupport children with min=0.
    // These are optional in the profile but marked mustSupport, so the
    // conformance checker expects them in responses. Examples:
    //   telecom.extension, qualification.issuer, target.extension,
    //   availableTime.daysOfWeek, availableTime.allDay,
    //   availableTime.availableStartTime, availableTime.availableEndTime,
    //   notAvailable.during, eligibility.code
    for element in elements {
        if !element.path.starts_with(&format!("{}.", parent_path)) {
            continue;
        }

        let suffix = element
            .path
            .strip_prefix(&format!("{}.", parent_path))
            .unwrap_or("");
        if suffix.contains('.') {
            continue;
        }

        let field_name = suffix.split(':').next().unwrap_or(suffix);
        if obj.contains_key(field_name) {
            continue;
        }

        // Must be optional (min=0) and mustSupport
        if element.min.unwrap_or(0) != 0 {
            continue;
        }
        if !element.must_support {
            continue;
        }

        if element.type_.is_empty() {
            continue;
        }

        let type_def = &element.type_[0];
        let type_code = &type_def.code;

        // For Extension type, generate a minimal valid extension with a URL.
        // The conformance checker verifies the field exists in responses, so
        // we need at least a stub extension even without knowing the exact URL.
        // Must include a value[x] to satisfy ext-1 ("Must have either extensions
        // or value[x], not both"). Always wrap in array since extension is 0..*
        // in the base spec.
        if type_code == "Extension" {
            let child_value = serde_json::json!({
                "url": "http://example.org/fhir/StructureDefinition/generated-extension",
                "valueString": "generated"
            });
            obj.insert(field_name.to_string(), serde_json::json!([child_value]));
            continue;
        }

        let target_profiles = &type_def.target_profile;
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
