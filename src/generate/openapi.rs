//! OpenAPI 3.0 spec generation from a FHIR CapabilityStatement.
//!
//! Walks the server-mode `rest` block of a CapabilityStatement and emits an
//! OpenAPI 3.0.3 document describing the REST API: one path per declared
//! interaction (read/vread/search/create/update/delete/history), typed query
//! parameters for each declared search parameter, resource-level and
//! system-level operations, and OAuth security when declared.
//!
//! This is a lightweight spec: request/response bodies reference generic
//! `Resource`, `Bundle`, and `OperationOutcome` schemas rather than full
//! per-profile JSON Schemas.

use crate::model::*;
use serde_json::{Map, Value, json};

/// Generate an OpenAPI 3.0.3 document from a CapabilityStatement.
///
/// `base_url` is used as the single `servers[0].url` entry.
pub fn generate_openapi(cs: &CapabilityStatement, base_url: &str) -> Value {
    let title = cs
        .software
        .as_ref()
        .and_then(|s| s.name.clone())
        .or_else(|| cs.name.clone())
        .unwrap_or_else(|| "FHIR API".to_string());
    let version = cs
        .software
        .as_ref()
        .and_then(|s| s.version.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let description = cs
        .implementation
        .as_ref()
        .and_then(|i| i.description.clone())
        .unwrap_or_else(|| "Generated from FHIR CapabilityStatement".to_string());

    let rest = cs.rest.iter().find(|r| r.mode == "server");

    let mut paths = Map::new();
    let mut tags: Vec<Value> = Vec::new();

    if let Some(rest) = rest {
        for res in &rest.resource {
            let rt = res.resource_type.as_str();
            tags.push(json!({ "name": rt }));

            let codes: Vec<&str> = res.interaction.iter().map(|i| i.code.as_str()).collect();

            // Type-level path: /{Type}
            let mut type_path = Map::new();
            if codes.contains(&"search-type") {
                type_path.insert("get".into(), search_operation(res));
            }
            if codes.contains(&"create") {
                type_path.insert("post".into(), create_operation(rt));
            }
            if !type_path.is_empty() {
                paths.insert(format!("/{rt}"), Value::Object(type_path));
            }

            // Instance-level path: /{Type}/{id}
            let mut instance_path = Map::new();
            if codes.contains(&"read") {
                instance_path.insert("get".into(), read_operation(rt));
            }
            if codes.contains(&"update") {
                instance_path.insert("put".into(), update_operation(rt));
            }
            if codes.contains(&"delete") {
                instance_path.insert("delete".into(), delete_operation(rt));
            }
            if !instance_path.is_empty() {
                paths.insert(format!("/{rt}/{{id}}"), Value::Object(instance_path));
            }

            // Version read: /{Type}/{id}/_history/{vid}
            if codes.contains(&"vread") {
                paths.insert(
                    format!("/{rt}/{{id}}/_history/{{vid}}"),
                    json!({ "get": vread_operation(rt) }),
                );
            }

            // Instance history: /{Type}/{id}/_history
            if codes.contains(&"history-instance") {
                paths.insert(
                    format!("/{rt}/{{id}}/_history"),
                    json!({ "get": history_operation(rt, true) }),
                );
            }

            // Type history: /{Type}/_history
            if codes.contains(&"history-type") {
                paths.insert(
                    format!("/{rt}/_history"),
                    json!({ "get": history_operation(rt, false) }),
                );
            }

            // Resource-level operations: POST /{Type}/${op}
            for op in &res.operation {
                let name = op.name.trim_start_matches('$');
                paths.insert(
                    format!("/{rt}/${name}"),
                    json!({ "post": operation_op(Some(rt), name) }),
                );
            }
        }

        // System-level operations: POST /${op}
        for op in &rest.operation {
            let name = op.name.trim_start_matches('$');
            paths.insert(
                format!("/${name}"),
                json!({ "post": operation_op(None, name) }),
            );
        }
    }

    let mut components = json!({
        "schemas": {
            "Resource": {
                "type": "object",
                "properties": {
                    "resourceType": { "type": "string" },
                    "id": { "type": "string" }
                },
                "required": ["resourceType"]
            },
            "Bundle": {
                "type": "object",
                "properties": {
                    "resourceType": { "type": "string", "enum": ["Bundle"] },
                    "type": { "type": "string" },
                    "total": { "type": "integer" },
                    "entry": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["resourceType"]
            },
            "OperationOutcome": {
                "type": "object",
                "properties": {
                    "resourceType": { "type": "string", "enum": ["OperationOutcome"] },
                    "issue": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["resourceType", "issue"]
            }
        }
    });

    let mut security: Vec<Value> = Vec::new();
    if let Some((scheme_name, scheme)) = rest
        .and_then(|r| r.security.as_ref())
        .and_then(build_security_scheme)
    {
        components["securitySchemes"] = json!({ scheme_name.clone(): scheme });
        security.push(json!({ scheme_name: [] }));
    }

    let mut doc = json!({
        "openapi": "3.0.3",
        "info": {
            "title": title,
            "version": version,
            "description": description
        },
        "servers": [ { "url": base_url } ],
        "tags": tags,
        "paths": Value::Object(paths),
        "components": components
    });

    if !security.is_empty() {
        doc["security"] = Value::Array(security);
    }

    doc
}

/// Map a FHIR search-parameter type to an OpenAPI parameter schema.
fn fhir_param_schema(param_type: &str) -> Value {
    match param_type {
        "number" => json!({ "type": "number" }),
        // date, string, token, reference, composite, quantity, uri, special
        // are all conveyed as strings on the wire (with FHIR-specific syntax).
        _ => json!({ "type": "string" }),
    }
}

/// The `{id}` path parameter.
fn id_param() -> Value {
    json!({
        "name": "id",
        "in": "path",
        "required": true,
        "schema": { "type": "string" },
        "description": "Logical id of the resource"
    })
}

/// A `requestBody` accepting a FHIR resource.
fn resource_request_body() -> Value {
    json!({
        "required": true,
        "content": {
            "application/fhir+json": {
                "schema": { "$ref": "#/components/schemas/Resource" }
            }
        }
    })
}

/// Standard responses returning a single resource.
fn resource_responses(rt: &str) -> Value {
    json!({
        "200": {
            "description": format!("{rt} resource"),
            "content": { "application/fhir+json": { "schema": { "$ref": "#/components/schemas/Resource" } } }
        },
        "404": operation_outcome_response("Resource not found"),
        "4XX": operation_outcome_response("Client error"),
        "5XX": operation_outcome_response("Server error")
    })
}

/// Standard responses returning a searchset Bundle.
fn bundle_responses() -> Value {
    json!({
        "200": {
            "description": "Search results bundle",
            "content": { "application/fhir+json": { "schema": { "$ref": "#/components/schemas/Bundle" } } }
        },
        "4XX": operation_outcome_response("Client error"),
        "5XX": operation_outcome_response("Server error")
    })
}

fn operation_outcome_response(description: &str) -> Value {
    json!({
        "description": description,
        "content": { "application/fhir+json": { "schema": { "$ref": "#/components/schemas/OperationOutcome" } } }
    })
}

fn read_operation(rt: &str) -> Value {
    json!({
        "tags": [rt],
        "summary": format!("Read {rt} by id"),
        "operationId": format!("read{rt}"),
        "parameters": [ id_param() ],
        "responses": resource_responses(rt)
    })
}

fn vread_operation(rt: &str) -> Value {
    json!({
        "tags": [rt],
        "summary": format!("Read a specific version of {rt}"),
        "operationId": format!("vread{rt}"),
        "parameters": [
            id_param(),
            {
                "name": "vid",
                "in": "path",
                "required": true,
                "schema": { "type": "string" },
                "description": "Version id of the resource"
            }
        ],
        "responses": resource_responses(rt)
    })
}

fn create_operation(rt: &str) -> Value {
    json!({
        "tags": [rt],
        "summary": format!("Create {rt}"),
        "operationId": format!("create{rt}"),
        "requestBody": resource_request_body(),
        "responses": {
            "201": {
                "description": format!("{rt} created"),
                "content": { "application/fhir+json": { "schema": { "$ref": "#/components/schemas/Resource" } } }
            },
            "4XX": operation_outcome_response("Client error"),
            "5XX": operation_outcome_response("Server error")
        }
    })
}

fn update_operation(rt: &str) -> Value {
    json!({
        "tags": [rt],
        "summary": format!("Update {rt} by id"),
        "operationId": format!("update{rt}"),
        "parameters": [ id_param() ],
        "requestBody": resource_request_body(),
        "responses": resource_responses(rt)
    })
}

fn delete_operation(rt: &str) -> Value {
    json!({
        "tags": [rt],
        "summary": format!("Delete {rt} by id"),
        "operationId": format!("delete{rt}"),
        "parameters": [ id_param() ],
        "responses": {
            "200": operation_outcome_response("Deletion outcome"),
            "204": { "description": "Deleted (no content)" },
            "404": operation_outcome_response("Resource not found")
        }
    })
}

/// Common FHIR result parameters available on every search.
fn result_parameters() -> Vec<Value> {
    [
        ("_count", "Number of results per page"),
        (
            "_sort",
            "Sort order (comma-separated fields, prefix '-' for descending)",
        ),
        ("_include", "Include referenced resources"),
        ("_revinclude", "Reverse-include referencing resources"),
        ("_summary", "Summary mode (true|text|data|count|false)"),
        ("_elements", "Comma-separated subset of elements to return"),
        (
            "_total",
            "Whether to include the total (none|estimate|accurate)",
        ),
    ]
    .into_iter()
    .map(|(name, desc)| {
        json!({
            "name": name,
            "in": "query",
            "required": false,
            "schema": { "type": "string" },
            "description": desc
        })
    })
    .collect()
}

fn search_operation(res: &RestResource) -> Value {
    let rt = res.resource_type.as_str();
    let mut parameters = result_parameters();
    for sp in &res.search_param {
        parameters.push(json!({
            "name": sp.name,
            "in": "query",
            "required": false,
            "schema": fhir_param_schema(&sp.param_type),
            "description": sp.documentation.clone().unwrap_or_else(|| {
                format!("{} search parameter ({})", sp.name, sp.param_type)
            })
        }));
    }
    json!({
        "tags": [rt],
        "summary": format!("Search {rt} resources"),
        "operationId": format!("search{rt}"),
        "parameters": parameters,
        "responses": bundle_responses()
    })
}

fn history_operation(rt: &str, instance: bool) -> Value {
    let (summary, params) = if instance {
        (format!("Retrieve history of a {rt}"), vec![id_param()])
    } else {
        (format!("Retrieve history of all {rt} resources"), vec![])
    };
    json!({
        "tags": [rt],
        "summary": summary,
        "operationId": format!("history{}{rt}", if instance { "Instance" } else { "Type" }),
        "parameters": params,
        "responses": bundle_responses()
    })
}

fn operation_op(rt: Option<&str>, name: &str) -> Value {
    let tag = rt.unwrap_or("System");
    let scope = rt.map(|r| format!("{r} ")).unwrap_or_default();
    json!({
        "tags": [tag],
        "summary": format!("Invoke {scope}operation ${name}"),
        "operationId": format!("operation_{}_{}", tag, name),
        "requestBody": {
            "required": false,
            "content": {
                "application/fhir+json": {
                    "schema": { "$ref": "#/components/schemas/Resource" }
                }
            }
        },
        "responses": {
            "200": {
                "description": "Operation result",
                "content": {
                    "application/fhir+json": {
                        "schema": {
                            "oneOf": [
                                { "$ref": "#/components/schemas/Resource" },
                                { "$ref": "#/components/schemas/Bundle" },
                                { "$ref": "#/components/schemas/OperationOutcome" }
                            ]
                        }
                    }
                }
            },
            "202": { "description": "Accepted (asynchronous operation)" },
            "4XX": operation_outcome_response("Client error"),
            "5XX": operation_outcome_response("Server error")
        }
    })
}

/// Build an OpenAPI `securityScheme` from a CapabilityStatement `rest.security`.
///
/// Returns `(scheme_name, scheme_object)`. When SMART/OAuth endpoints are
/// declared via the standard `oauth-uris` extension they are surfaced as an
/// OAuth2 scheme; otherwise a generic bearer scheme is produced.
fn build_security_scheme(security: &Security) -> Option<(String, Value)> {
    let declares_oauth = security.service.iter().any(|svc| {
        svc.coding.iter().any(|c| {
            let code = c.code.as_deref().unwrap_or("");
            let text = svc.text.as_deref().unwrap_or("");
            code.eq_ignore_ascii_case("OAuth")
                || code.eq_ignore_ascii_case("SMART-on-FHIR")
                || text.to_ascii_lowercase().contains("oauth")
                || text.to_ascii_lowercase().contains("smart")
        })
    });

    if declares_oauth {
        Some((
            "oauth2".to_string(),
            json!({
                "type": "oauth2",
                "description": security.description.clone().unwrap_or_default(),
                "flows": {
                    "clientCredentials": { "tokenUrl": "", "scopes": {} },
                    "authorizationCode": { "authorizationUrl": "", "tokenUrl": "", "scopes": {} }
                }
            }),
        ))
    } else if security.cors == Some(true) || !security.service.is_empty() {
        Some((
            "bearerAuth".to_string(),
            json!({
                "type": "http",
                "scheme": "bearer",
                "bearerFormat": "JWT",
                "description": security.description.clone().unwrap_or_default()
            }),
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cs() -> CapabilityStatement {
        serde_json::from_value(json!({
            "resourceType": "CapabilityStatement",
            "status": "active",
            "name": "Test",
            "software": { "name": "Test Server", "version": "1.2.3" },
            "rest": [{
                "mode": "server",
                "security": { "cors": true, "service": [{ "coding": [{ "code": "OAuth" }] }] },
                "resource": [{
                    "type": "Patient",
                    "interaction": [
                        { "code": "read" }, { "code": "vread" }, { "code": "search-type" },
                        { "code": "create" }, { "code": "update" }, { "code": "delete" }
                    ],
                    "searchParam": [
                        { "name": "name", "type": "string" },
                        { "name": "birthdate", "type": "date" },
                        { "name": "_count", "type": "number" }
                    ],
                    "operation": [{ "name": "everything" }]
                }],
                "operation": [{ "name": "export" }]
            }]
        }))
        .unwrap()
    }

    #[test]
    fn generates_paths_for_declared_interactions() {
        let doc = generate_openapi(&sample_cs(), "http://example.org/fhir");
        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["info"]["title"], "Test Server");
        assert_eq!(doc["info"]["version"], "1.2.3");
        assert_eq!(doc["servers"][0]["url"], "http://example.org/fhir");

        let paths = &doc["paths"];
        assert!(paths.get("/Patient").is_some());
        assert!(paths["/Patient"].get("get").is_some()); // search
        assert!(paths["/Patient"].get("post").is_some()); // create
        assert!(paths.get("/Patient/{id}").is_some());
        assert!(paths["/Patient/{id}"].get("get").is_some()); // read
        assert!(paths["/Patient/{id}"].get("put").is_some()); // update
        assert!(paths["/Patient/{id}"].get("delete").is_some()); // delete
        assert!(paths.get("/Patient/{id}/_history/{vid}").is_some()); // vread
        assert!(paths.get("/Patient/$everything").is_some());
        assert!(paths.get("/$export").is_some());
    }

    #[test]
    fn search_params_become_query_parameters() {
        let doc = generate_openapi(&sample_cs(), "http://example.org/fhir");
        let params = doc["paths"]["/Patient"]["get"]["parameters"]
            .as_array()
            .unwrap();
        let names: Vec<&str> = params.iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"birthdate"));
        // Result parameters are always present
        assert!(names.contains(&"_count"));
        assert!(names.contains(&"_sort"));
    }

    #[test]
    fn oauth_security_scheme_is_emitted() {
        let doc = generate_openapi(&sample_cs(), "http://example.org/fhir");
        assert!(doc["components"]["securitySchemes"]["oauth2"].is_object());
        assert_eq!(doc["security"][0]["oauth2"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn no_security_when_none_declared() {
        let mut cs = sample_cs();
        cs.rest[0].security = None;
        let doc = generate_openapi(&cs, "http://example.org/fhir");
        assert!(doc.get("security").is_none());
        assert!(doc["components"].get("securitySchemes").is_none());
    }
}
