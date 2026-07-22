use crate::generate::model::*;
use serde_json::Value;

/// Evaluate a response assertion against an actual HTTP response body.
/// Returns a list of assertion failures (empty = all assertions pass).
pub fn assert_response(
    assertion: &ResponseAssertion,
    _status_code: u16,
    body: &Option<Value>,
) -> Vec<String> {
    let mut errors = Vec::new();

    // --- Bundle type ---
    if let Some(expected_type) = &assertion.bundle_type {
        if let Some(body) = body {
            if let Some(rt) = body.get("resourceType").and_then(|v| v.as_str()) {
                if rt == "Bundle" {
                    if let Some(actual_type) = body.get("type").and_then(|v| v.as_str()) {
                        if actual_type != expected_type {
                            errors.push(format!(
                                "Bundle type is '{}', expected '{}'",
                                actual_type, expected_type
                            ));
                        }
                    } else {
                        errors.push("Bundle has no 'type' field".to_string());
                    }
                } else if rt != "OperationOutcome" {
                    errors.push(format!("Expected Bundle, got resourceType '{}'", rt));
                }
            } else {
                errors.push("Response has no resourceType".to_string());
            }
        } else {
            errors.push("No response body to assert Bundle type".to_string());
        }
    }

    // --- Entry count and content ---
    if let Some(body) = body {
        if let Some(entries) = body.get("entry").and_then(|v| v.as_array()) {
            let count = entries.len();
            if let Some(min) = assertion.min_entries {
                if count < min {
                    errors.push(format!(
                        "Bundle has {} entries, expected at least {}",
                        count, min
                    ));
                }
            }
            if let Some(max) = assertion.max_entries {
                if count > max {
                    errors.push(format!(
                        "Bundle has {} entries, expected at most {}",
                        count, max
                    ));
                }
            }

            // --- Resource types present ---
            if !assertion.resource_types.is_empty() {
                let present_types: std::collections::HashSet<String> = entries
                    .iter()
                    .filter_map(|e| {
                        e.get("resource")
                            .and_then(|r| r.get("resourceType"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();

                for expected_rt in &assertion.resource_types {
                    if !present_types.contains(expected_rt) {
                        errors.push(format!(
                            "Expected Bundle to contain '{}' resource(s), but found: {:?}",
                            expected_rt,
                            present_types.iter().collect::<Vec<_>>()
                        ));
                    }
                }
            }

            // --- Field values ---
            for (resource_type, fields) in &assertion.field_values {
                let matching_entries: Vec<_> = entries
                    .iter()
                    .filter(|e| {
                        e.get("resource")
                            .and_then(|r| r.get("resourceType"))
                            .and_then(|v| v.as_str())
                            == Some(resource_type.as_str())
                    })
                    .collect();

                if matching_entries.is_empty() {
                    errors.push(format!(
                        "Expected at least one {} in Bundle for field assertion, found none",
                        resource_type
                    ));
                    continue;
                }

                for entry in &matching_entries {
                    let resource = entry.get("resource").unwrap();
                    for (path, expected_value) in fields {
                        let actual = resolve_json_path(resource, path);
                        match actual {
                            None => {
                                errors.push(format!(
                                    "{}: field '{}' not found in response",
                                    resource_type, path
                                ));
                            }
                            Some(val) if val != *expected_value => {
                                errors.push(format!(
                                    "{}: field '{}' expected {:?}, got {:?}",
                                    resource_type, path, expected_value, val
                                ));
                            }
                            _ => {} // matches
                        }
                    }
                }
            }

            // --- Include types ---
            for include_type in assertion.include_types.keys() {
                let found = entries.iter().any(|e| {
                    e.get("resource")
                        .and_then(|r| r.get("resourceType"))
                        .and_then(|v| v.as_str())
                        == Some(include_type.as_str())
                });
                if !found {
                    errors.push(format!(
                        "Expected Bundle to include '{}' resources from _include/_revinclude, but none found",
                        include_type
                    ));
                }
            }

            // --- Sort assertion ---
            if let Some(sort) = &assertion.sort_by {
                let resources: Vec<&Value> =
                    entries.iter().filter_map(|e| e.get("resource")).collect();

                if resources.len() >= 2 {
                    let values: Vec<Option<Value>> = resources
                        .iter()
                        .map(|r| resolve_json_path(r, &sort.field))
                        .collect();

                    let sorted = match sort.direction.as_str() {
                        "asc" => values.windows(2).all(|w| compare_values(&w[0], &w[1]) <= 0),
                        "desc" => values.windows(2).all(|w| compare_values(&w[0], &w[1]) >= 0),
                        _ => true,
                    };

                    if !sorted {
                        errors.push(format!(
                            "Bundle entries not sorted by '{}' in {} order",
                            sort.field, sort.direction
                        ));
                    }
                }
            }

            // --- Absent fields (for _summary) ---
            for field in &assertion.absent_fields {
                for entry in entries.iter() {
                    if let Some(resource) = entry.get("resource") {
                        if resource.get(field).is_some() {
                            errors.push(format!(
                                "Resource contains field '{}' which should be absent with _summary",
                                field
                            ));
                        }
                    }
                }
            }
        } else if (assertion.bundle_type.is_some()
            || assertion.min_entries.is_some()
            || !assertion.resource_types.is_empty())
            && body.get("resourceType").and_then(|v| v.as_str()) == Some("Bundle")
        {
            errors.push("Bundle has no 'entry' array".to_string());
        }
    }

    // --- OperationOutcome severity ---
    if let Some(expected_severity) = &assertion.outcome_severity {
        if let Some(body) = body {
            if body.get("resourceType").and_then(|v| v.as_str()) != Some("OperationOutcome") {
                errors.push(format!(
                    "Expected OperationOutcome, got resourceType '{}'",
                    body.get("resourceType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                ));
            } else {
                let issues = body.get("issue").and_then(|v| v.as_array());
                match issues {
                    None => {
                        errors.push("OperationOutcome has no 'issue' array".to_string());
                    }
                    Some(issues) if issues.is_empty() => {
                        errors.push("OperationOutcome has empty 'issue' array".to_string());
                    }
                    Some(issues) => {
                        let has_matching = issues.iter().any(|i| {
                            i.get("severity")
                                .and_then(|v| v.as_str())
                                .map(|s| s == expected_severity)
                                .unwrap_or(false)
                        });
                        if !has_matching {
                            let severities: Vec<&str> = issues
                                .iter()
                                .filter_map(|i| i.get("severity").and_then(|v| v.as_str()))
                                .collect();
                            errors.push(format!(
                                "Expected OperationOutcome with severity '{}', found: {:?}",
                                expected_severity, severities
                            ));
                        }
                    }
                }
            }
        } else {
            errors.push("No response body for OperationOutcome assertion".to_string());
        }
    }

    // --- Top-level key presence ---
    if let Some(key) = &assertion.response_contains_key {
        if let Some(body) = body {
            if body.get(key).is_none() {
                errors.push(format!(
                    "Expected response to contain key '{}', but it was not found",
                    key
                ));
            }
        }
    }

    errors
}

/// Resolve a dotted JSON path like "name.family" or "birthDate" to a value.
/// Handles arrays by returning the first matching value.
fn resolve_json_path(value: &Value, path: &str) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in parts {
        if let Some(arr) = current.as_array() {
            if arr.is_empty() {
                return None;
            }
            current = &arr[0];
        }

        match current.get(part) {
            Some(v) => current = v,
            None => return None,
        }
    }

    Some(current.clone())
}

/// Compare two JSON values for sorting. Returns negative if a < b, 0 if equal, positive if a > b.
fn compare_values(a: &Option<Value>, b: &Option<Value>) -> i32 {
    match (a, b) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(a_val), Some(b_val)) => {
            if let (Some(a_str), Some(b_str)) = (a_val.as_str(), b_val.as_str()) {
                a_str.cmp(b_str) as i32
            } else if let (Some(a_num), Some(b_num)) = (a_val.as_f64(), b_val.as_f64()) {
                a_num
                    .partial_cmp(&b_num)
                    .unwrap_or(std::cmp::Ordering::Equal) as i32
            } else {
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn assert_bundle_type_match() {
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_bundle_type_mismatch() {
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "batch",
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors
            .iter()
            .any(|e| e.contains("batch") && e.contains("searchset")));
    }

    #[test]
    fn assert_min_entries_pass() {
        let assertion = ResponseAssertion {
            min_entries: Some(1),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{"resource": {"resourceType": "Patient", "id": "123"}}]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_min_entries_fail() {
        let assertion = ResponseAssertion {
            min_entries: Some(2),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{"resource": {"resourceType": "Patient", "id": "123"}}]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("at least 2")));
    }

    #[test]
    fn assert_resource_types_present() {
        let assertion = ResponseAssertion {
            resource_types: vec!["Patient".to_string(), "Provenance".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}},
                {"resource": {"resourceType": "Provenance", "id": "2"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_field_values_match() {
        let mut field_values = HashMap::new();
        let mut patient_fields = HashMap::new();
        patient_fields.insert("name.family".to_string(), json!("GeneratedFamily"));
        field_values.insert("Patient".to_string(), patient_fields);

        let assertion = ResponseAssertion {
            field_values,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{
                "resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "name": [{"family": "GeneratedFamily", "given": ["Test"]}]
                }
            }]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_field_values_mismatch() {
        let mut field_values = HashMap::new();
        let mut patient_fields = HashMap::new();
        patient_fields.insert("name.family".to_string(), json!("GeneratedFamily"));
        field_values.insert("Patient".to_string(), patient_fields);

        let assertion = ResponseAssertion {
            field_values,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{
                "resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "name": [{"family": "Smith", "given": ["John"]}]
                }
            }]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors
            .iter()
            .any(|e| e.contains("family") && e.contains("Smith")));
    }

    #[test]
    fn assert_include_types_present() {
        let mut include_types = HashMap::new();
        include_types.insert("Observation".to_string(), "subject".to_string());

        let assertion = ResponseAssertion {
            include_types,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}},
                {"resource": {"resourceType": "Observation", "id": "2"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_operation_outcome_severity() {
        let assertion = ResponseAssertion {
            outcome_severity: Some("error".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "not-found"}]
        });
        let errors = assert_response(&assertion, 404, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_sort_ascending() {
        let assertion = ResponseAssertion {
            sort_by: Some(SortAssertion {
                field: "birthDate".to_string(),
                direction: "asc".to_string(),
            }),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "birthDate": "1990-01-01"}},
                {"resource": {"resourceType": "Patient", "birthDate": "1995-06-15"}},
                {"resource": {"resourceType": "Patient", "birthDate": "2000-12-31"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_absent_fields_pass() {
        let assertion = ResponseAssertion {
            absent_fields: vec!["text".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{
                "resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "name": [{"family": "Test"}]
                }
            }]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for absent 'text', got: {:?}",
            errors
        );
    }

    #[test]
    fn resolve_json_path_simple() {
        let v = json!({"birthDate": "1990-01-01"});
        assert_eq!(
            resolve_json_path(&v, "birthDate"),
            Some(json!("1990-01-01"))
        );
    }

    #[test]
    fn resolve_json_path_nested() {
        let v = json!({"name": [{"family": "Smith"}]});
        assert_eq!(resolve_json_path(&v, "name.family"), Some(json!("Smith")));
    }

    #[test]
    fn resolve_json_path_missing() {
        let v = json!({"id": "123"});
        assert_eq!(resolve_json_path(&v, "missing"), None);
    }
}
