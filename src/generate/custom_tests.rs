use serde_json::Value;
use std::collections::HashMap;

/// Resolve `{ResourceType.id}` and `{ResourceType.field.path}` templates in a URL.
///
/// Supports:
/// - `{base_url}` → the server's base URL
/// - `{ResourceType.id}` → the created resource's server-assigned ID
/// - `{ResourceType.field.path}` → a field value extracted from the resource
///   (e.g. `{Patient.name.family}` → the first name's family value)
/// - `{steps.<name>.id}` → the server-assigned ID from a sequence step
/// - `{steps.<name>.<field>}` → a field value from a sequence step response
pub fn resolve_url_templates(
    url: &str,
    base_url: &str,
    created_ids: &HashMap<String, String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    step_responses: &HashMap<String, Value>,
) -> String {
    let mut result = url.to_string();

    // Resolve {base_url}
    result = result.replace("{base_url}", base_url);

    // Resolve {ResourceType.id} and {ResourceType.field.path}
    for (resource_type, id) in created_ids {
        let id_pattern = format!("{{{}.id}}", resource_type);
        result = result.replace(&id_pattern, id);

        if let Some(fields) = field_values.get(resource_type) {
            for (field_path, value) in fields {
                let pattern = format!("{{{}.{}}}", resource_type, field_path);
                result = result.replace(&pattern, value);
            }
        }
    }

    // Resolve {steps.<name>.id} and {steps.<name>.<field>}
    for (step_name, response) in step_responses {
        let id_pattern = format!("{{steps.{}.id}}", step_name);
        if let Some(id) = response.get("id").and_then(|v| v.as_str()) {
            result = result.replace(&id_pattern, id);
        }

        // Walk the response for field paths
        if let Some(obj) = response.as_object() {
            resolve_step_fields(&mut result, step_name, "", obj);
        }
    }

    result
}

/// Recursively walk a JSON object and resolve `{steps.<name>.<path>}` templates.
fn resolve_step_fields(
    result: &mut String,
    step_name: &str,
    prefix: &str,
    obj: &serde_json::Map<String, Value>,
) {
    for (key, value) in obj {
        let current_path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };

        let pattern = format!("{{steps.{}.{}}}", step_name, current_path);

        match value {
            Value::String(s) => {
                let has_pattern = result.contains(&pattern);
                if has_pattern {
                    let replacement = result.replacen(&pattern, s, 1);
                    *result = replacement;
                }
            }
            Value::Number(n) => {
                let s = n.to_string();
                let has_pattern = result.contains(&pattern);
                if has_pattern {
                    let replacement = result.replacen(&pattern, &s, 1);
                    *result = replacement;
                }
            }
            Value::Array(arr) => {
                // For arrays, try index 0 as a shorthand
                if let Some(Value::Object(child)) = arr.first() {
                    resolve_step_fields(result, step_name, &current_path, child);
                }
            }
            Value::Object(child) => {
                resolve_step_fields(result, step_name, &current_path, child);
            }
            _ => {}
        }
    }
}

/// Resolve `{steps.<name>.id}` and `{steps.<name>.<field>}` templates in a string.
/// This is a simpler version for sequence step fields (id, params, body_overrides).
pub fn resolve_step_templates(s: &str, step_responses: &HashMap<String, Value>) -> String {
    let mut result = s.to_string();

    for (step_name, response) in step_responses {
        let id_pattern = format!("{{steps.{}.id}}", step_name);
        if let Some(id) = response.get("id").and_then(|v| v.as_str()) {
            result = result.replace(&id_pattern, id);
        }

        if let Some(obj) = response.as_object() {
            resolve_step_fields(&mut result, step_name, "", obj);
        }
    }

    result
}

/// Resolve `{steps.<name>.*}` templates in a JSON value (for body_overrides).
pub fn resolve_step_templates_in_value(value: &mut Value, step_responses: &HashMap<String, Value>) {
    match value {
        Value::String(s) => {
            *s = resolve_step_templates(s, step_responses);
        }
        Value::Object(obj) => {
            for v in obj.values_mut() {
                resolve_step_templates_in_value(v, step_responses);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                resolve_step_templates_in_value(v, step_responses);
            }
        }
        _ => {}
    }
}

/// Convert a `CustomAssertDef` into a `ResponseAssertion`.
pub fn custom_assert_to_response_assertion(
    assert: &crate::config::models::CustomAssertDef,
) -> crate::generate::model::ResponseAssertion {
    let mut field_values: HashMap<String, HashMap<String, serde_json::Value>> = HashMap::new();
    for (key, value) in &assert.field_values {
        // Parse the key as a field path and resource type
        // Format: "resourceType_fieldpath" or just "fieldpath"
        // We use a generic "_response" key for the response resource
        let entry = field_values.entry("_response".to_string()).or_default();
        entry.insert(key.clone(), serde_json::Value::String(value.clone()));
    }

    crate::generate::model::ResponseAssertion {
        bundle_type: assert.bundle_type.clone(),
        min_entries: assert.min_entries,
        max_entries: assert.max_entries,
        bundle_total_present: assert.bundle_total_present,
        summary_mode: assert.summary_mode.clone(),
        outcome_severity: assert.outcome_severity.clone(),
        accept_statuses: assert.accept_statuses.clone(),
        absent_fields: assert.absent_fields.clone(),
        required_fields: assert.required_fields.clone(),
        resource_types: assert.resource_types.clone(),
        response_contains_key: assert.response_contains_key.clone(),
        response_resource_types: assert.response_resource_types.clone(),
        sort_by: assert
            .sort_by
            .as_ref()
            .map(|s| crate::generate::model::SortAssertion {
                field: s.field.clone(),
                direction: s.direction.clone(),
                additional_fields: s.additional_fields.clone(),
            }),
        field_values,
        ..crate::generate::model::ResponseAssertion::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_base_url() {
        let result = resolve_url_templates(
            "{base_url}/Patient",
            "http://localhost:8080/fhir",
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(result, "http://localhost:8080/fhir/Patient");
    }

    #[test]
    fn resolve_resource_type_id() {
        let mut ids = HashMap::new();
        ids.insert("Patient".to_string(), "abc-123".to_string());
        let result = resolve_url_templates(
            "/Patient/{Patient.id}",
            "",
            &ids,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(result, "/Patient/abc-123");
    }

    #[test]
    fn resolve_field_value() {
        let mut ids = HashMap::new();
        ids.insert("Patient".to_string(), "abc-123".to_string());
        let mut fields: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut patient_fields = HashMap::new();
        patient_fields.insert("name.family".to_string(), "Smith".to_string());
        fields.insert("Patient".to_string(), patient_fields);

        let result = resolve_url_templates(
            "/Patient?name={Patient.name.family}",
            "",
            &ids,
            &fields,
            &HashMap::new(),
        );
        assert_eq!(result, "/Patient?name=Smith");
    }

    #[test]
    fn resolve_step_id() {
        let mut steps = HashMap::new();
        steps.insert(
            "prac".to_string(),
            serde_json::json!({
                "resourceType": "Practitioner",
                "id": "prac-001"
            }),
        );

        let result = resolve_step_templates("/Practitioner/{steps.prac.id}", &steps);
        assert_eq!(result, "/Practitioner/prac-001");
    }

    #[test]
    fn resolve_step_field() {
        let mut steps = HashMap::new();
        steps.insert(
            "prac".to_string(),
            serde_json::json!({
                "resourceType": "Practitioner",
                "id": "prac-001",
                "name": [{"family": "Smith"}]
            }),
        );

        let result = resolve_step_templates("/Practitioner?name={steps.prac.name.family}", &steps);
        assert_eq!(result, "/Practitioner?name=Smith");
    }
}
