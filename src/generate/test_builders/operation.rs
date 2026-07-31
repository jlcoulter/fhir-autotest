use crate::generate::model::*;
use crate::generate::test_builders::search::resolve_param_value;
use crate::model::OperationDefinition;
use std::collections::HashMap;

pub(crate) fn build_operation_test(
    resource_type: &str,
    code: &str,
    op_def: Option<&OperationDefinition>,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    // Build request body from operation parameters
    let has_required_input_params = op_def
        .map(|def| {
            def.parameter
                .iter()
                .any(|p| p.use_.as_deref() == Some("in") && p.min.unwrap_or(0) > 0)
        })
        .unwrap_or(false);

    let body = if has_required_input_params {
        op_def.map(|def| {
            let mut params = serde_json::Map::new();
            params.insert(
                "resourceType".to_string(),
                serde_json::Value::String("Parameters".to_string()),
            );
            let mut param_array = Vec::new();
            for p in &def.parameter {
                if p.use_.as_deref() == Some("in") && p.min.unwrap_or(0) > 0 {
                    let mut param_obj = serde_json::Map::new();
                    param_obj.insert(
                        "name".to_string(),
                        serde_json::Value::String(p.name.clone()),
                    );
                    if let Some(ptype) = &p.param_type {
                        let value = resolve_param_value(
                            resource_type,
                            &p.name,
                            ptype,
                            field_values,
                            created_ids,
                        );
                        param_obj.insert("value".to_string(), serde_json::Value::String(value));
                    }
                    param_array.push(serde_json::Value::Object(param_obj));
                }
            }
            params.insert(
                "parameter".to_string(),
                serde_json::Value::Array(param_array),
            );
            serde_json::Value::Object(params)
        })
    } else {
        // No required input params — use GET with optional params as
        // query-string parameters instead of a POST body.  Many FHIR
        // operations (e.g. $export) only support GET and return 404/405
        // for POST.
        None
    };

    let method = if has_required_input_params {
        "POST"
    } else {
        "GET"
    };

    // Determine URL based on operation scope
    let url = match op_def {
        Some(def)
            if def.system.unwrap_or(false)
                && !def.type_.unwrap_or(false)
                && !def.instance.unwrap_or(false) =>
        {
            format!("/${code}")
        }
        Some(def) if def.instance.unwrap_or(false) => {
            format!("/{resource_type}/{{id}}/${code}")
        }
        Some(def) if def.type_.unwrap_or(false) => {
            format!("/{resource_type}/${code}")
        }
        _ => {
            tracing::warn!(
                "Unknown operation scope for {}, defaulting to resource-level",
                code
            );
            format!("/{resource_type}/${code}")
        }
    };

    let mut assertion = ResponseAssertion {
        response_contains_key: Some("resourceType".to_string()),
        response_resource_types: vec![
            "Bundle".to_string(),
            "Parameters".to_string(),
            "OperationOutcome".to_string(),
        ],
        ..ResponseAssertion::none()
    };
    if op_def
        .map(|d| {
            d.parameter
                .iter()
                .any(|p| p.use_.as_deref() == Some("out") && p.min.unwrap_or(0) > 0)
        })
        .unwrap_or(false)
    {
        assertion.response_contains_key = Some("parameter".to_string());
        assertion.response_resource_types =
            vec!["Parameters".to_string(), "OperationOutcome".to_string()];
        // Collect output parameter names for validation
        if let Some(def) = op_def {
            assertion.operation_output_params = def
                .parameter
                .iter()
                .filter(|p| p.use_.as_deref() == Some("out"))
                .map(|p| p.name.clone())
                .collect();
        }
    }

    TestCase {
        name: format!(
            "{}_operation_{}",
            resource_type.to_lowercase(),
            code.replace('-', "_")
        ),
        kind: TestCaseKind::Operation {
            code: code.to_string(),
        },
        interaction: Interaction::Operation(code.to_string()),
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: method.to_string(),
            url,
            headers: HashMap::new(),
            body,
        },
        validation: ValidationSpec {
            expected_status: 200,
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: Some(assertion),
        },
    }
}

/// Build a test that invokes an operation twice to verify idempotency.
/// Operations with `idempotent=true` should return the same result when called
/// multiple times with the same parameters.
pub(crate) fn build_operation_idempotent_test(
    resource_type: &str,
    code: &str,
    op_def: Option<&OperationDefinition>,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let base_test = build_operation_test(
        resource_type,
        code,
        op_def,
        profile_url,
        field_values,
        created_ids,
    );

    // For idempotent tests, we use expected_status=0 to accept any status
    // (the executor will call the operation twice and compare responses).
    // The test name and kind signal that this is an idempotency check.
    TestCase {
        name: format!(
            "{}_operation_{}_idempotent",
            resource_type.to_lowercase(),
            code.replace('-', "_")
        ),
        kind: TestCaseKind::Conformance {
            description: format!("{} operation idempotent", code),
        },
        interaction: Interaction::Operation(code.to_string()),
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: base_test.request,
        validation: ValidationSpec {
            expected_status: 0, // executor handles the comparison
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build a test that invokes an operation twice to verify no side effects.
/// Operations with `affectsState=false` should be safe to call (no side effects).
pub(crate) fn build_operation_affects_state_test(
    resource_type: &str,
    code: &str,
    op_def: Option<&OperationDefinition>,
    profile_url: &Option<String>,
    field_values: &HashMap<String, HashMap<String, String>>,
    created_ids: &HashMap<String, String>,
) -> TestCase {
    let base_test = build_operation_test(
        resource_type,
        code,
        op_def,
        profile_url,
        field_values,
        created_ids,
    );

    TestCase {
        name: format!(
            "{}_operation_{}_affects_state",
            resource_type.to_lowercase(),
            code.replace('-', "_")
        ),
        kind: TestCaseKind::Conformance {
            description: format!("{} operation affectsState=false", code),
        },
        interaction: Interaction::Operation(code.to_string()),
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: base_test.request,
        validation: ValidationSpec {
            expected_status: 0, // executor handles the comparison
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build a negative test for an operation invoked without required parameters.
/// Operations with required input params (min > 0) should reject requests
/// that omit those parameters with a 4xx error.
pub(crate) fn build_operation_error_test(
    resource_type: &str,
    code: &str,
    op_def: Option<&OperationDefinition>,
    profile_url: &Option<String>,
) -> TestCase {
    // Determine URL based on operation scope (same logic as build_operation_test)
    let url = match op_def {
        Some(def)
            if def.system.unwrap_or(false)
                && !def.type_.unwrap_or(false)
                && !def.instance.unwrap_or(false) =>
        {
            format!("/${code}")
        }
        Some(def) if def.instance.unwrap_or(false) => {
            format!("/{resource_type}/{{id}}/${code}")
        }
        Some(def) if def.type_.unwrap_or(false) => {
            format!("/{resource_type}/${code}")
        }
        _ => {
            format!("/{resource_type}/${code}")
        }
    };

    TestCase {
        name: format!(
            "{}_operation_{}_missing_required_params",
            resource_type.to_lowercase(),
            code.replace('-', "_")
        ),
        kind: TestCaseKind::Negative {
            description: format!("{} operation missing required params", code),
        },
        interaction: Interaction::Operation(code.to_string()),
        resource_type: resource_type.to_string(),
        profile_url: profile_url.clone(),
        request: HttpRequest {
            method: "POST".to_string(),
            url,
            headers: HashMap::new(),
            body: Some(serde_json::json!({
                "resourceType": "Parameters",
                "parameter": []
            })),
        },
        validation: ValidationSpec {
            expected_status: 0, // accept any non-2xx (400, 422, etc.)
            profile_url: None,
            required_elements: Vec::new(),
            forbidden_elements: Vec::new(),
            response_assertion: None,
        },
    }
}

/// Build a negative test for invoking an operation at an undeclared scope.
/// Operations should only be invocable at their declared scopes (system, type, instance).
pub(crate) fn build_operation_scope_test(
    resource_type: &str,
    code: &str,
    op_def: Option<&OperationDefinition>,
    profile_url: &Option<String>,
) -> Vec<TestCase> {
    let mut tests = Vec::new();

    let def = match op_def {
        Some(d) => d,
        None => return tests,
    };

    let is_system = def.system.unwrap_or(false);
    let is_type = def.type_.unwrap_or(false);
    let is_instance = def.instance.unwrap_or(false);

    // For system-only operations: try at resource/instance level
    if is_system && !is_type && !is_instance {
        tests.push(TestCase {
            name: format!(
                "{}_operation_{}_scope_type_level",
                resource_type.to_lowercase(),
                code.replace('-', "_")
            ),
            kind: TestCaseKind::Negative {
                description: format!("{} operation at type scope (system-only)", code),
            },
            interaction: Interaction::Operation(code.to_string()),
            resource_type: resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/{resource_type}/${code}"),
                headers: HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 0, // accept any non-2xx
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion: None,
            },
        });
        tests.push(TestCase {
            name: format!(
                "{}_operation_{}_scope_instance_level",
                resource_type.to_lowercase(),
                code.replace('-', "_")
            ),
            kind: TestCaseKind::Negative {
                description: format!("{} operation at instance scope (system-only)", code),
            },
            interaction: Interaction::Operation(code.to_string()),
            resource_type: resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/{resource_type}/{{id}}/${code}"),
                headers: HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 0,
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion: None,
            },
        });
    }

    // For instance-only operations: try at system level
    if !is_system && !is_type && is_instance {
        tests.push(TestCase {
            name: format!(
                "{}_operation_{}_scope_system_level",
                resource_type.to_lowercase(),
                code.replace('-', "_")
            ),
            kind: TestCaseKind::Negative {
                description: format!("{} operation at system scope (instance-only)", code),
            },
            interaction: Interaction::Operation(code.to_string()),
            resource_type: resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/${code}"),
                headers: HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 0,
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion: None,
            },
        });
    }

    // For type-only operations: try at instance level
    if !is_system && is_type && !is_instance {
        tests.push(TestCase {
            name: format!(
                "{}_operation_{}_scope_instance_level",
                resource_type.to_lowercase(),
                code.replace('-', "_")
            ),
            kind: TestCaseKind::Negative {
                description: format!("{} operation at instance scope (type-only)", code),
            },
            interaction: Interaction::Operation(code.to_string()),
            resource_type: resource_type.to_string(),
            profile_url: profile_url.clone(),
            request: HttpRequest {
                method: "GET".to_string(),
                url: format!("/{resource_type}/{{id}}/${code}"),
                headers: HashMap::new(),
                body: None,
            },
            validation: ValidationSpec {
                expected_status: 0,
                profile_url: None,
                required_elements: Vec::new(),
                forbidden_elements: Vec::new(),
                response_assertion: None,
            },
        });
    }

    tests
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OperationParameter;

    #[test]
    fn build_operation_test_instance_scope() {
        let op_def = OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/OperationDefinition/Patient-everything".to_string(),
            name: "everything".to_string(),
            code: "everything".to_string(),
            system: Some(false),
            type_: Some(false),
            instance: Some(true),
            parameter: vec![],
            affects_state: None,
            idempotent: None,
        };
        let test = build_operation_test(
            "Patient",
            "everything",
            Some(&op_def),
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.method, "GET");
        assert_eq!(test.request.url, "/Patient/{id}/$everything");
        assert_eq!(test.name, "patient_operation_everything");
        assert!(matches!(test.kind, TestCaseKind::Operation { ref code } if code == "everything"));
        assert_eq!(test.validation.expected_status, 200);
        let assertion = test.validation.response_assertion.as_ref().unwrap();
        assert_eq!(
            assertion.response_contains_key,
            Some("resourceType".to_string())
        );
        // No required input params → GET with no body
        assert!(test.request.body.is_none());
    }

    #[test]
    fn build_operation_test_system_scope() {
        let op_def = OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/uv/bulkdata/OperationDefinition/export".to_string(),
            name: "export".to_string(),
            code: "export".to_string(),
            system: Some(true),
            type_: Some(false),
            instance: Some(false),
            parameter: vec![],
            affects_state: None,
            idempotent: None,
        };
        let test = build_operation_test(
            "System",
            "export",
            Some(&op_def),
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(test.request.url, "/$export");
        assert_eq!(test.request.method, "GET"); // No required params → GET
        assert_eq!(test.name, "system_operation_export");
    }

    #[test]
    fn build_operation_test_with_body_params() {
        let op_def = OperationDefinition {
            resource_type: "OperationDefinition".to_string(),
            url: "http://hl7.org/fhir/OperationDefinition/Patient-everything".to_string(),
            name: "everything".to_string(),
            code: "everything".to_string(),
            system: Some(false),
            type_: Some(false),
            instance: Some(true),
            parameter: vec![OperationParameter {
                name: "start".to_string(),
                use_: Some("in".to_string()),
                min: Some(1),
                max: Some("1".to_string()),
                param_type: Some("date".to_string()),
            }],
            affects_state: None,
            idempotent: None,
        };
        let test = build_operation_test(
            "Patient",
            "everything",
            Some(&op_def),
            &None,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            test.request.body.is_some(),
            "Should have a request body for operations with required params"
        );
        assert_eq!(test.request.method, "POST"); // Has required params → POST
        let body = test.request.body.unwrap();
        assert_eq!(body["resourceType"], "Parameters");
        assert!(body["parameter"].is_array());
        assert_eq!(body["parameter"][0]["name"], "start");
    }
}
