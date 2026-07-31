use crate::model::*;
use crate::runner::response_assertions::resolve_json_path;

/// Validate a FHIR resource against a StructureDefinition profile.
///
/// Checks:
/// - resourceType matches the profile's base type
/// - Required elements (min > 0) are present
/// - Fixed/pattern values match
pub fn validate_against_profile(
    resource: &serde_json::Value,
    profile: &StructureDefinition,
) -> Vec<String> {
    let mut errors = Vec::new();

    // Check resourceType
    if let Some(rt) = resource.get("resourceType").and_then(|v| v.as_str()) {
        if rt != profile.base_type {
            errors.push(format!(
                "resourceType is '{}', expected '{}'",
                rt, profile.base_type
            ));
        }
    } else {
        errors.push("Missing resourceType".to_string());
    }

    let elements = match &profile.snapshot {
        Some(snapshot) => &snapshot.element,
        None => match &profile.differential {
            Some(diff) => &diff.element,
            None => return errors,
        },
    };

    for element in elements {
        let field_path = match get_field_path(&element.path, &profile.base_type) {
            Some(name) => name,
            None => continue,
        };

        // Skip the root element
        if field_path == profile.base_type {
            continue;
        }

        // Resolve the value at this path once (handles nested paths via resolve_json_path)
        let resolved_value = resolve_json_path(resource, &field_path);

        // Check required elements
        if element.min.unwrap_or(0) > 0 && resolved_value.is_none() {
            errors.push(format!(
                "Missing required element: {} (min={})",
                element.path,
                element.min.unwrap_or(0)
            ));
        }

        // Check fixed values
        if let Some(fixed) = &element.fixed_string
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_str())
            && val != fixed
        {
            errors.push(format!(
                "{}: expected '{}', got '{}'",
                element.path, fixed, val
            ));
        }
        if let Some(fixed) = &element.fixed_code
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_str())
            && val != fixed
        {
            errors.push(format!(
                "{}: expected code '{}', got '{}'",
                element.path, fixed, val
            ));
        }
        if let Some(fixed) = &element.fixed_uri
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_str())
            && val != fixed
        {
            errors.push(format!(
                "{}: expected uri '{}', got '{}'",
                element.path, fixed, val
            ));
        }
        if let Some(fixed) = &element.fixed_boolean
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_bool())
            && val != *fixed
        {
            errors.push(format!("{}: expected {}, got {}", element.path, fixed, val));
        }
        if let Some(fixed) = &element.fixed_integer
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_i64())
            && val != *fixed as i64
        {
            errors.push(format!("{}: expected {}, got {}", element.path, fixed, val));
        }

        // Check pattern values
        if let Some(pattern) = &element.pattern_string
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_str())
            && val != pattern
        {
            errors.push(format!(
                "{}: pattern expected '{}', got '{}'",
                element.path, pattern, val
            ));
        }
        if let Some(pattern) = &element.pattern_code
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_str())
            && val != pattern
        {
            errors.push(format!(
                "{}: pattern code expected '{}', got '{}'",
                element.path, pattern, val
            ));
        }

        // fixed_decimal
        if let Some(fixed) = &element.fixed_decimal
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_f64())
            && (val - fixed).abs() > f64::EPSILON
        {
            errors.push(format!(
                "{}: expected decimal {}, got {}",
                element.path, fixed, val
            ));
        }

        // fixed_quantity, pattern_quantity
        if let Some(fixed) = &element.fixed_quantity
            && let Some(val) = resolved_value.as_ref()
            && val != fixed
        {
            errors.push(format!(
                "{}: expected quantity {:?}, got {:?}",
                element.path, fixed, val
            ));
        }
        if let Some(pattern) = &element.pattern_quantity
            && let Some(val) = resolved_value.as_ref()
            && val != pattern
        {
            errors.push(format!(
                "{}: pattern quantity expected {:?}, got {:?}",
                element.path, pattern, val
            ));
        }

        // fixed_coding, pattern_coding
        if let Some(fixed) = &element.fixed_coding
            && let Some(val) = resolved_value.as_ref()
            && val != fixed
        {
            errors.push(format!(
                "{}: expected coding {:?}, got {:?}",
                element.path, fixed, val
            ));
        }
        if let Some(pattern) = &element.pattern_coding
            && let Some(val) = resolved_value.as_ref()
            && val != pattern
        {
            errors.push(format!(
                "{}: pattern coding expected {:?}, got {:?}",
                element.path, pattern, val
            ));
        }

        // fixed_codeable_concept, pattern_codeable_concept
        if let Some(fixed) = &element.fixed_codeable_concept
            && let Some(val) = resolved_value.as_ref()
            && val != fixed
        {
            errors.push(format!(
                "{}: expected codeable concept {:?}, got {:?}",
                element.path, fixed, val
            ));
        }
        if let Some(pattern) = &element.pattern_codeable_concept
            && let Some(val) = resolved_value.as_ref()
            && val != pattern
        {
            errors.push(format!(
                "{}: pattern codeable concept expected {:?}, got {:?}",
                element.path, pattern, val
            ));
        }

        // pattern_boolean
        if let Some(pattern) = &element.pattern_boolean
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_bool())
            && val != *pattern
        {
            errors.push(format!(
                "{}: pattern expected {}, got {}",
                element.path, pattern, val
            ));
        }

        // pattern_uri
        if let Some(pattern) = &element.pattern_uri
            && let Some(val) = resolved_value.as_ref().and_then(|v| v.as_str())
            && val != pattern
        {
            errors.push(format!(
                "{}: pattern uri expected '{}', got '{}'",
                element.path, pattern, val
            ));
        }
    }

    errors
}

/// Extract the field path from a FHIR path like "Patient.name.family" → "name.family".
/// Handles nested paths recursively.
fn get_field_path(path: &str, resource_type: &str) -> Option<String> {
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

    // Empty field part (e.g., "Patient.") → None
    if field_part.is_empty() {
        return None;
    }

    // Strip slice notation (e.g., "identifier:type" → "identifier")
    // For nested paths, only strip the first segment's slice notation
    let field_name = if let Some((first, rest)) = field_part.split_once('.') {
        let first_clean = first.split(':').next().unwrap_or(first);
        format!("{}.{}", first_clean, rest)
    } else {
        field_part
            .split(':')
            .next()
            .unwrap_or(field_part)
            .to_string()
    };

    Some(field_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_patient_profile() -> StructureDefinition {
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
                ],
            }),
            differential: None,
        }
    }

    #[test]
    fn validate_valid_resource() {
        let profile = test_patient_profile();
        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test", "given": ["Patient"]}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_wrong_resource_type() {
        let profile = test_patient_profile();
        let resource = serde_json::json!({
            "resourceType": "Observation",
            "name": [{"family": "Test"}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(errors.iter().any(|e| e.contains("resourceType")));
    }

    #[test]
    fn validate_missing_required_field() {
        let profile = test_patient_profile();
        let resource = serde_json::json!({
            "resourceType": "Patient"
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("name") && e.contains("Missing required"))
        );
    }

    #[test]
    fn validate_fixed_value_mismatch() {
        let mut profile = test_patient_profile();
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "gender": "female"
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("gender") && e.contains("expected code 'male'"))
        );
    }

    #[test]
    fn validate_nested_required_field_present() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.name.family".to_string(),
                path: "Patient.name.family".to_string(),
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith", "given": ["John"]}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_nested_required_field_missing() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.name.family".to_string(),
                path: "Patient.name.family".to_string(),
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

        // name exists but family is missing
        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"given": ["John"]}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("name.family") && e.contains("Missing required")),
            "Expected error about missing name.family, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_nested_required_field_empty_name() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.name.family".to_string(),
                path: "Patient.name.family".to_string(),
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

        // name exists but is an empty object — family is missing
        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("name.family") && e.contains("Missing required")),
            "Expected error about missing name.family, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_nested_fixed_value() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.name.family".to_string(),
                path: "Patient.name.family".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "string".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_string: Some("Smith".to_string()),
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
            });
        }

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Jones"}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("name.family") && e.contains("expected 'Smith'")),
            "Expected error about name.family fixed value mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_fixed_coding_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.gender".to_string(),
                path: "Patient.gender".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "Coding".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_coding: Some(serde_json::json!({
                    "system": "http://example.org",
                    "code": "male"
                })),
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "gender": {"system": "http://example.org", "code": "female"}
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("gender") && e.contains("expected coding")),
            "Expected error about gender coding mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_fixed_coding_match() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.gender".to_string(),
                path: "Patient.gender".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "Coding".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_coding: Some(serde_json::json!({
                    "system": "http://example.org",
                    "code": "male"
                })),
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "gender": {"system": "http://example.org", "code": "male"}
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_pattern_codeable_concept_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.gender".to_string(),
                path: "Patient.gender".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "CodeableConcept".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                pattern_codeable_concept: Some(serde_json::json!({
                    "coding": [{"system": "http://example.org", "code": "male"}]
                })),
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
                constraint: vec![],
                is_modifier: false,
                is_summary: false,
                slice_name: None,
                slicing: None,
            });
        }

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "gender": {"coding": [{"system": "http://other.org", "code": "female"}]}
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("gender") && e.contains("pattern codeable concept")),
            "Expected error about gender codeable concept mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_pattern_codeable_concept_match() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.gender".to_string(),
                path: "Patient.gender".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "CodeableConcept".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                pattern_codeable_concept: Some(serde_json::json!({
                    "coding": [{"system": "http://example.org", "code": "male"}]
                })),
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
                constraint: vec![],
                is_modifier: false,
                is_summary: false,
                slice_name: None,
                slicing: None,
            });
        }

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "gender": {"coding": [{"system": "http://example.org", "code": "male"}]}
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_fixed_decimal_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.height".to_string(),
                path: "Patient.height".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "decimal".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_decimal: Some(1.75),
                fixed_string: None,
                fixed_uri: None,
                fixed_code: None,
                fixed_boolean: None,
                fixed_integer: None,
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "height": 1.80
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("height") && e.contains("expected decimal")),
            "Expected error about height decimal mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_fixed_quantity_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.weight".to_string(),
                path: "Patient.weight".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "Quantity".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_quantity: Some(serde_json::json!({
                    "value": 70.0,
                    "unit": "kg"
                })),
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "weight": {"value": 80.0, "unit": "kg"}
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("weight") && e.contains("expected quantity")),
            "Expected error about weight quantity mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_pattern_boolean_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.active".to_string(),
                path: "Patient.active".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "boolean".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                pattern_boolean: Some(true),
                fixed_string: None,
                fixed_uri: None,
                fixed_code: None,
                fixed_boolean: None,
                fixed_integer: None,
                fixed_decimal: None,
                pattern_string: None,
                pattern_uri: None,
                pattern_code: None,
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "active": false
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("active") && e.contains("pattern expected")),
            "Expected error about active boolean mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_pattern_uri_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.photo.url".to_string(),
                path: "Patient.photo.url".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "uri".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                pattern_uri: Some("http://example.org/photo".to_string()),
                fixed_string: None,
                fixed_uri: None,
                fixed_code: None,
                fixed_boolean: None,
                fixed_integer: None,
                fixed_decimal: None,
                pattern_string: None,
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "photo": [{"url": "http://other.org/photo"}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("photo.url") && e.contains("pattern uri")),
            "Expected error about photo.url uri mismatch, got: {:?}",
            errors
        );
    }

    // ── Tests for get_field_path ──────────────────────────────────────

    #[test]
    fn get_field_path_root() {
        assert_eq!(
            get_field_path("Patient", "Patient"),
            Some("Patient".to_string())
        );
    }

    #[test]
    fn get_field_path_simple() {
        assert_eq!(
            get_field_path("Patient.name", "Patient"),
            Some("name".to_string())
        );
    }

    #[test]
    fn get_field_path_nested() {
        assert_eq!(
            get_field_path("Patient.name.family", "Patient"),
            Some("name.family".to_string())
        );
    }

    #[test]
    fn get_field_path_with_slice() {
        assert_eq!(
            get_field_path("Patient.identifier:type", "Patient"),
            Some("identifier".to_string())
        );
    }

    #[test]
    fn get_field_path_nested_with_slice() {
        assert_eq!(
            get_field_path("Patient.identifier:type.value", "Patient"),
            Some("identifier.value".to_string())
        );
    }

    #[test]
    fn get_field_path_wrong_prefix() {
        assert_eq!(get_field_path("Observation.subject", "Patient"), None);
    }

    #[test]
    fn get_field_path_no_dot_after_prefix() {
        // Path like "PatientExtra" — starts with "Patient" but no dot after
        assert_eq!(get_field_path("PatientExtra", "Patient"), None);
    }

    #[test]
    fn get_field_path_empty_after_dot() {
        assert_eq!(get_field_path("Patient.", "Patient"), None);
    }

    #[test]
    fn validate_against_profile_with_differential() {
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestPatient".to_string(),
            base_type: "Patient".to_string(),
            name: "TestPatient".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: None,
            differential: Some(Differential {
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
                ],
            }),
        };

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_against_profile_no_snapshot_no_differential() {
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Empty".to_string(),
            base_type: "Patient".to_string(),
            name: "Empty".to_string(),
            kind: "resource".to_string(),
            derivation: None,
            base_definition: None,
            snapshot: None,
            differential: None,
        };

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}]
        });
        let errors = validate_against_profile(&resource, &profile);
        // No snapshot or differential — should return early with no errors
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_fixed_string_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.name.family".to_string(),
                path: "Patient.name.family".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "string".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_string: Some("ExpectedName".to_string()),
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
            });
        }

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "ActualName"}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("name.family") && e.contains("expected 'ExpectedName'")),
            "Expected error about name.family fixed string mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_fixed_uri_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.photo.url".to_string(),
                path: "Patient.photo.url".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "uri".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_string: None,
                fixed_uri: Some("http://expected.uri".to_string()),
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
            });
        }

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "photo": [{"url": "http://actual.uri"}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("photo.url") && e.contains("expected uri")),
            "Expected error about photo.url uri mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_fixed_integer_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.extension.value".to_string(),
                path: "Patient.extension.value".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "integer".to_string(),
                    target_profile: vec![],
                    profile: vec![],
                    versioning: None,
                }],
                fixed_string: None,
                fixed_uri: None,
                fixed_code: None,
                fixed_boolean: None,
                fixed_integer: Some(42),
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "extension": [{"value": 100}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors.iter().any(|e| e.contains("expected 42")),
            "Expected error about integer mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_pattern_string_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.name.family".to_string(),
                path: "Patient.name.family".to_string(),
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
                pattern_string: Some("ExpectedPattern".to_string()),
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "ActualName"}]
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors.iter().any(|e| e.contains("pattern expected")),
            "Expected error about pattern string mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_pattern_code_mismatch() {
        let mut profile = test_patient_profile();
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
                fixed_string: None,
                fixed_uri: None,
                fixed_code: None,
                fixed_boolean: None,
                fixed_integer: None,
                fixed_decimal: None,
                pattern_string: None,
                pattern_uri: None,
                pattern_code: Some("male".to_string()),
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "gender": "female"
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors.iter().any(|e| e.contains("pattern code expected")),
            "Expected error about pattern code mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_pattern_quantity_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.weight".to_string(),
                path: "Patient.weight".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "Quantity".to_string(),
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
                pattern_quantity: Some(serde_json::json!({
                    "value": 70.0,
                    "unit": "kg"
                })),
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

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "weight": {"value": 80.0, "unit": "kg"}
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors.iter().any(|e| e.contains("pattern quantity")),
            "Expected error about pattern quantity mismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_pattern_coding_mismatch() {
        let mut profile = test_patient_profile();
        if let Some(ref mut snapshot) = profile.snapshot {
            snapshot.element.push(ElementDefinition {
                id: "Patient.gender".to_string(),
                path: "Patient.gender".to_string(),
                min: Some(1),
                max: Some("1".to_string()),
                type_: vec![ElementDefinitionType {
                    code: "Coding".to_string(),
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
                pattern_coding: Some(serde_json::json!({
                    "system": "http://example.org",
                    "code": "male"
                })),
                fixed_codeable_concept: None,
                pattern_codeable_concept: None,
                constraint: vec![],
                is_modifier: false,
                is_summary: false,
                slice_name: None,
                slicing: None,
            });
        }

        let resource = serde_json::json!({
            "resourceType": "Patient",
            "name": [{"family": "Test"}],
            "gender": {"system": "http://other.org", "code": "female"}
        });
        let errors = validate_against_profile(&resource, &profile);
        assert!(
            errors.iter().any(|e| e.contains("pattern coding")),
            "Expected error about pattern coding mismatch, got: {:?}",
            errors
        );
    }
}
