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

/// Stamp FHIR meta fields (versionId, lastUpdated) on a resource.
fn stamp_meta(body: &mut serde_json::Value) {
    if body.get("meta").is_none() {
        body["meta"] = serde_json::json!({});
    }
    let meta = body.get_mut("meta").unwrap();
    if meta.get("versionId").is_none() {
        meta["versionId"] = serde_json::Value::String("1".to_string());
    }
    if meta.get("lastUpdated").is_none() {
        meta["lastUpdated"] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());
    }
}

async fn create_resource(
    State(store): State<MockStore>,
    Path(rtype): Path<String>,
    Json(mut body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    body["id"] = serde_json::Value::String(id.clone());
    stamp_meta(&mut body);
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
    #[serde(default)]
    _sort: Option<String>,
    #[serde(default)]
    _include: Option<String>,
    #[serde(default)]
    _revinclude: Option<String>,
    #[serde(default)]
    _elements: Option<String>,
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

    // Apply _sort
    if let Some(sort_param) = params._sort.as_deref() {
        let ascending = !sort_param.starts_with('-');
        let field = sort_param.trim_start_matches('-');
        resources.sort_by(|a, b| {
            let a_val = a.get(field).and_then(|v| v.as_str()).unwrap_or("");
            let b_val = b.get(field).and_then(|v| v.as_str()).unwrap_or("");
            if ascending {
                a_val.cmp(b_val)
            } else {
                b_val.cmp(a_val)
            }
        });
    }

    // Apply _summary
    if params._summary.as_deref() == Some("true") {
        resources = resources
            .into_iter()
            .map(|r| {
                let summary = serde_json::json!({
                    "resourceType": r["resourceType"],
                    "id": r["id"],
                    "meta": r["meta"],
                });
                // Preserve any fields explicitly marked as summary elements
                // For now, keep id, meta, resourceType as per FHIR _summary=true
                summary
            })
            .collect();
    }

    // Apply _elements
    if let Some(elements_str) = params._elements.as_deref() {
        let elements: Vec<&str> = elements_str.split(',').map(|s| s.trim()).collect();
        if !elements.is_empty() {
            resources = resources
                .into_iter()
                .map(|r| {
                    let mut filtered = serde_json::json!({
                        "resourceType": r["resourceType"],
                        "id": r["id"],
                    });
                    for elem in &elements {
                        if let Some(val) = r.get(*elem) {
                            filtered[elem] = val.clone();
                        }
                    }
                    filtered
                })
                .collect();
        }
    }

    // Apply _count (save total before truncation)
    let total_before_count = resources.len();
    if let Some(count) = params._count {
        resources.truncate(count as usize);
    }

    // Build entries
    let mut entries: Vec<serde_json::Value> = resources
        .iter()
        .map(|r| {
            serde_json::json!({
                "resource": r,
                "fullUrl": format!("http://localhost/fhir/{}/{}", rtype, r["id"].as_str().unwrap_or_default())
            })
        })
        .collect();

    // Handle _include: include referenced resources in the same Bundle
    if let Some(include_param) = params._include.as_deref() {
        // Format: ResourceType:search-parameter or ResourceType:search-parameter:targetType
        let parts: Vec<&str> = include_param.split(':').collect();
        if parts.len() >= 2 {
            let search_param = parts[1];
            // Collect referenced resource IDs from the matching resources
            let mut included_resources = Vec::new();
            for r in &resources {
                if let Some(refs) = r.get(search_param).and_then(|v| v.as_array()) {
                    for reference in refs {
                        if let Some(ref_str) = reference.get("reference").and_then(|v| v.as_str()) {
                            // Parse "ResourceType/id" from the reference
                            if let Some((ref_type, ref_id)) = ref_str.split_once('/') {
                                if let Some(ref_resources) = store.get(ref_type) {
                                    if let Some(found) = ref_resources.iter().find(|rr| {
                                        rr.get("id").and_then(|v| v.as_str()) == Some(ref_id)
                                    }) {
                                        included_resources.push(serde_json::json!({
                                            "resource": found,
                                            "fullUrl": format!("http://localhost/fhir/{}/{}", ref_type, ref_id)
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            entries.extend(included_resources);
        }
    }

    // Handle _revinclude: include resources that reference the matched resources
    if let Some(revinclude_param) = params._revinclude.as_deref() {
        // Format: SourceType:search-parameter
        let parts: Vec<&str> = revinclude_param.split(':').collect();
        if parts.len() >= 2 {
            let source_type = parts[0];
            let search_param = parts[1];
            if let Some(source_resources) = store.get(source_type) {
                let mut rev_included = Vec::new();
                for r in &resources {
                    let rid = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    for source in source_resources {
                        if let Some(refs) = source.get(search_param).and_then(|v| v.as_array()) {
                            for reference in refs {
                                if let Some(ref_str) =
                                    reference.get("reference").and_then(|v| v.as_str())
                                {
                                    if ref_str == format!("{}/{}", rtype, rid) {
                                        let sid =
                                            source.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                        rev_included.push(serde_json::json!({
                                            "resource": source,
                                            "fullUrl": format!("http://localhost/fhir/{}/{}", source_type, sid)
                                        }));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                entries.extend(rev_included);
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": total_before_count,
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
                    .map(|s| {
                        s.to_lowercase() == value_lower || s.to_lowercase().contains(&value_lower)
                    })
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
                        if g.as_str()
                            .map(|s| s.to_lowercase().contains(&value_lower))
                            .unwrap_or(false)
                        {
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
    stamp_meta(&mut body);
    let mut store = store.lock().unwrap();
    let resources = store.entry(rtype.clone()).or_default();
    if let Some(idx) = resources
        .iter()
        .position(|r| r.get("id").and_then(|v| v.as_str()) == Some(&id))
    {
        resources[idx] = body.clone();
        (StatusCode::OK, Json(body))
    } else {
        // Update-as-create: resource doesn't exist yet, create it
        resources.push(body.clone());
        (StatusCode::CREATED, Json(body))
    }
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
    let addr = std::net::Ipv4Addr::LOCALHOST;
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    /// Start the mock server on a random port and return the base URL.
    async fn setup_server() -> String {
        let app = create_mock_app();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{}/fhir", addr)
    }

    #[tokio::test]
    async fn test_create_and_read_resource() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create
        let resp = client
            .post(format!("{}/Patient", base_url))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "name": [{"family": "Test"}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: serde_json::Value = resp.json().await.unwrap();
        let id = body["id"].as_str().unwrap().to_string();

        // Read
        let resp = client
            .get(format!("{}/Patient/{}", base_url, id))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["id"], id);
        assert_eq!(body["name"][0]["family"], "Test");
    }

    #[tokio::test]
    async fn test_read_nonexistent_resource() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{}/Patient/nonexistent-id", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["resourceType"], "OperationOutcome");
    }

    #[tokio::test]
    async fn test_search_all_resources() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create 3 resources
        for i in 0..3 {
            client
                .post(format!("{}/Patient", base_url))
                .header("Content-Type", "application/fhir+json")
                .json(&serde_json::json!({
                    "resourceType": "Patient",
                    "id": format!("pat-{}", i),
                    "name": [{"family": format!("Test{}", i)}]
                }))
                .send()
                .await
                .unwrap();
        }

        // Search all
        let resp = client
            .get(format!("{}/Patient", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["resourceType"], "Bundle");
        assert_eq!(body["total"], 3);
        assert_eq!(body["entry"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_search_with_filter() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create resources with different family names
        let patients = ["Smith", "Jones", "Smith"];
        for (i, family) in patients.iter().enumerate() {
            client
                .post(format!("{}/Patient", base_url))
                .header("Content-Type", "application/fhir+json")
                .json(&serde_json::json!({
                    "resourceType": "Patient",
                    "id": format!("pat-{}", i),
                    "name": [{"family": family}]
                }))
                .send()
                .await
                .unwrap();
        }

        // Search with filter
        let resp = client
            .get(format!("{}/Patient?family=Smith", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 2);
        assert_eq!(body["entry"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_search_with_count() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create 5 resources
        for i in 0..5 {
            client
                .post(format!("{}/Patient", base_url))
                .header("Content-Type", "application/fhir+json")
                .json(&serde_json::json!({
                    "resourceType": "Patient",
                    "id": format!("pat-{}", i),
                    "name": [{"family": format!("Test{}", i)}]
                }))
                .send()
                .await
                .unwrap();
        }

        // Search with _count=2
        let resp = client
            .get(format!("{}/Patient?_count=2", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 5);
        assert_eq!(body["entry"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_search_empty_results() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create a resource
        client
            .post(format!("{}/Patient", base_url))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "name": [{"family": "Smith"}]
            }))
            .send()
            .await
            .unwrap();

        // Search for a non-matching value
        let resp = client
            .get(format!("{}/Patient?family=Nonexistent", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 0);
        assert_eq!(body["entry"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_update_resource() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create
        let resp = client
            .post(format!("{}/Patient", base_url))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "name": [{"family": "Old"}]
            }))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        let id = body["id"].as_str().unwrap().to_string();

        // Update
        let resp = client
            .put(format!("{}/Patient/{}", base_url, id))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "name": [{"family": "Updated"}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["name"][0]["family"], "Updated");

        // Verify via GET
        let resp = client
            .get(format!("{}/Patient/{}", base_url, id))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["name"][0]["family"], "Updated");
    }

    #[tokio::test]
    async fn test_delete_resource() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create
        let resp = client
            .post(format!("{}/Patient", base_url))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "name": [{"family": "DeleteMe"}]
            }))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        let id = body["id"].as_str().unwrap().to_string();

        // Delete
        let resp = client
            .delete(format!("{}/Patient/{}", base_url, id))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify gone
        let resp = client
            .get(format!("{}/Patient/{}", base_url, id))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_resource() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!("{}/Patient/nonexistent-id", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["resourceType"], "OperationOutcome");
    }

    #[tokio::test]
    async fn test_create_resource_with_id() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // PUT with a specific ID (create-as-update)
        let resp = client
            .put(format!("{}/Patient/my-custom-id", base_url))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "name": [{"family": "Custom"}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["id"], "my-custom-id");

        // Verify via GET
        let resp = client
            .get(format!("{}/Patient/my-custom-id", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["id"], "my-custom-id");
        assert_eq!(body["name"][0]["family"], "Custom");
    }

    #[tokio::test]
    async fn test_search_with_unknown_param() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create a resource
        client
            .post(format!("{}/Patient", base_url))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "name": [{"family": "Test"}]
            }))
            .send()
            .await
            .unwrap();

        // Search with an unknown parameter — should return 200 Bundle (permissive)
        let resp = client
            .get(format!("{}/Patient?unknownparam=foo", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["resourceType"], "Bundle");
        assert_eq!(body["total"], 0);
    }
}
