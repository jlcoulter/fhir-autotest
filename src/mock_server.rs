use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

type MockStore = Arc<Mutex<HashMap<String, Vec<serde_json::Value>>>>;

async fn create_resource(
    State(store): State<MockStore>,
    Path(rtype): Path<String>,
    Json(mut body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    body["id"] = serde_json::Value::String(id.clone());
    let mut store = store.lock().unwrap();
    store.entry(rtype.clone()).or_default().push(body.clone());
    (StatusCode::CREATED, Json(body))
}

async fn read_resource(
    State(store): State<MockStore>,
    Path((rtype, id)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = store.lock().unwrap();
    if let Some(resources) = store.get(&rtype) {
        if let Some(resource) = resources
            .iter()
            .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(&id))
        {
            return (StatusCode::OK, Json(resource.clone()));
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "not-found", "diagnostics": format!("{}/{} not found", rtype, id)}]
        })),
    )
}

#[derive(Deserialize, Default)]
struct SearchParams {
    #[serde(default)]
    _count: Option<u32>,
    #[serde(default)]
    _summary: Option<String>,
    // Accept any other params without erroring
    #[serde(flatten)]
    _rest: HashMap<String, String>,
}

async fn search_resources(
    State(store): State<MockStore>,
    Path(rtype): Path<String>,
    Query(params): Query<SearchParams>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = store.lock().unwrap();
    let mut resources = store.get(&rtype).cloned().unwrap_or_default();

    // Basic parameter filtering: if query params are present, try to match
    // string/token fields on the stored resources. Params starting with _ are
    // FHIR special params and are skipped for filtering.
    let filter_keys: Vec<String> = params
        ._rest
        .keys()
        .filter(|k| !k.starts_with('_'))
        .cloned()
        .collect();

    if !filter_keys.is_empty() {
        resources.retain(|r| {
            filter_keys.iter().all(|key| {
                let desired = &params._rest[key];
                // Check top-level field and nested fields (name.family, etc.)
                match_field(r, key, desired)
            })
        });
    }

    // Apply _count
    let total = resources.len();
    if let Some(count) = params._count {
        resources.truncate(count as usize);
    }

    let entries: Vec<serde_json::Value> = resources
        .iter()
        .map(|r| {
            serde_json::json!({
                "resource": r,
                "fullUrl": format!("http://localhost/fhir/{}/{}", rtype, r["id"].as_str().unwrap_or_default())
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": total,
            "entry": entries
        })),
    )
}

/// Try to match a search parameter against a resource field.
/// Handles top-level fields (e.g. "active" → r.active) and
/// nested fields with dotted notation (e.g. "name" checks r.name[].family).
fn match_field(resource: &serde_json::Value, param: &str, value: &str) -> bool {
    let value_lower = value.to_lowercase();

    // Direct top-level match
    if let Some(v) = resource.get(param) {
        if json_contains(v, &value_lower) {
            return true;
        }
    }

    // Token-style: check coding.code and coding.display
    if param == "code" || param.ends_with("-code") {
        if let Some(codings) = find_all_codings(resource) {
            return codings.iter().any(|c| {
                c.get("code")
                    .or_else(|| c.get("display"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_lowercase() == value_lower || s.to_lowercase().contains(&value_lower))
                    .unwrap_or(false)
            });
        }
    }

    // Name search: check HumanName arrays
    if param == "name" || param == "family" || param == "given" {
        if let Some(names) = resource.get("name").and_then(|n| n.as_array()) {
            for name in names {
                if let Some(family) = name.get("family").and_then(|f| f.as_str()) {
                    if family.to_lowercase().contains(&value_lower) {
                        return true;
                    }
                }
                if let Some(given) = name.get("given").and_then(|g| g.as_array()) {
                    for g in given {
                        if g.as_str().map(|s| s.to_lowercase().contains(&value_lower)).unwrap_or(false) {
                            return true;
                        }
                    }
                }
            }
        }
    }

    // Identifier search
    if param == "identifier" {
        if let Some(ids) = resource.get("identifier").and_then(|i| i.as_array()) {
            return ids.iter().any(|id| {
                id.get("value")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_lowercase().contains(&value_lower))
                    .unwrap_or(false)
            });
        }
    }

    false
}

/// Check if a JSON value contains a string (case-insensitive).
fn json_contains(value: &serde_json::Value, search: &str) -> bool {
    match value {
        serde_json::Value::String(s) => s.to_lowercase().contains(search),
        serde_json::Value::Bool(b) => search == b.to_string(),
        serde_json::Value::Number(n) => search == n.to_string(),
        serde_json::Value::Array(arr) => arr.iter().any(|v| json_contains(v, search)),
        serde_json::Value::Object(map) => map.values().any(|v| json_contains(v, search)),
        _ => false,
    }
}

/// Extract all Coding objects from code/CodeableConcept fields in a resource.
fn find_all_codings(resource: &serde_json::Value) -> Option<Vec<&serde_json::Value>> {
    let mut codings = Vec::new();
    // Check common code fields
    for field in &["code", "type", "specialty", "role"] {
        if let Some(v) = resource.get(*field) {
            extract_codings_from_value(v, &mut codings);
        }
    }
    if codings.is_empty() {
        None
    } else {
        Some(codings)
    }
}

fn extract_codings_from_value<'a>(
    value: &'a serde_json::Value,
    codings: &mut Vec<&'a serde_json::Value>,
) {
    if let Some(coding_arr) = value.get("coding").and_then(|c| c.as_array()) {
        codings.extend(coding_arr.iter());
    } else if let Some(arr) = value.as_array() {
        for item in arr {
            extract_codings_from_value(item, codings);
        }
    }
}

async fn update_resource(
    State(store): State<MockStore>,
    Path((rtype, id)): Path<(String, String)>,
    Json(mut body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    body["id"] = serde_json::Value::String(id.clone());
    let mut store = store.lock().unwrap();
    if let Some(resources) = store.get_mut(&rtype) {
        if let Some(idx) = resources
            .iter()
            .position(|r| r.get("id").and_then(|v| v.as_str()) == Some(&id))
        {
            resources[idx] = body.clone();
            return (StatusCode::OK, Json(body));
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "not-found"}]
        })),
    )
}

async fn delete_resource(
    State(store): State<MockStore>,
    Path((rtype, id)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut store = store.lock().unwrap();
    if let Some(resources) = store.get_mut(&rtype) {
        let before = resources.len();
        resources.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(&id));
        if resources.len() < before {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "resourceType": "OperationOutcome",
                    "issue": [{"severity": "information", "code": "informational", "diagnostics": "Deleted"}]
                })),
            );
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "not-found"}]
        })),
    )
}

async fn operation_handler(
    State(_store): State<MockStore>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "resourceType": "Parameters",
            "parameter": [{"name": "result", "valueBoolean": true}]
        })),
    )
}

/// Build the mock FHIR server axum Router.
pub fn create_mock_app() -> Router {
    let store: MockStore = Arc::new(Mutex::new(HashMap::new()));
    Router::new()
        .route("/fhir/{rtype}", post(create_resource))
        .route("/fhir/{rtype}", get(search_resources))
        .route("/fhir/{rtype}/{id}", get(read_resource))
        .route("/fhir/{rtype}/{id}", put(update_resource))
        .route("/fhir/{rtype}/{id}", delete(delete_resource))
        .route("/fhir/${*op}", get(operation_handler))
        .route("/fhir/{rtype}/${*op}", get(operation_handler))
        .with_state(store)
}

/// Start the mock FHIR server and return the address it's bound to.
///
/// The server runs in a background tokio task. Call `shutdown` to stop it.
pub async fn start_mock_server(port: u16) -> anyhow::Result<SocketAddr> {
    let addr = if port == 0 {
        std::net::Ipv4Addr::LOCALHOST
    } else {
        std::net::Ipv4Addr::LOCALHOST
    };
    let listener = tokio::net::TcpListener::bind((addr, port)).await?;
    let bound_addr = listener.local_addr()?;

    let app = create_mock_app();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start accepting connections
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    Ok(bound_addr)
}