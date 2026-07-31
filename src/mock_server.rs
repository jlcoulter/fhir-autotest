//! Mock FHIR server for CI and development testing.
//!
//! This module provides a lightweight in-memory FHIR server backed by axum.
//! It supports basic CRUD operations, search with parameter filtering, sorting,
//! `_summary`, `_elements`, `_count`, `_include`, and `_revinclude`.
//!
//! # Limitations
//!
//! - **Chained search** (e.g., `GET /Patient?organization.name=Smith`) is not
//!   implemented — dotted parameters are treated as field paths on the resource
//!   itself, not resolved through related resources.
//! - **`_has` / `_list` / `_query`** result parameters are not supported.
//! - **Conditional operations** (`If-None-Exist`, `If-Match`) are not handled.
//! - **`_elements`** filtering does not handle nested paths like `name.family`.
//! - **Modifier support** is limited to `:exact`, `:contains`, `:missing`, and
//!   `:not` on string fields. Other FHIR modifiers (`:text`, `:of-type`, etc.)
//!   are not implemented.
//! - **Prefix support** is limited to `eq`, `ne`, `gt`, `lt`, `ge`, `le` on
//!   numeric/date fields. Comparison is string-based, not semantic.
//! - **Operation handler** returns canned responses for `$export` and
//!   `$validate`; all other operations return a generic success.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post, put},
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
    tracing::debug!("Mock POST /{} → 201 Created (id={})", rtype, id);
    (StatusCode::CREATED, Json(body))
}

async fn read_resource(
    State(store): State<MockStore>,
    Path((rtype, id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = store.lock().unwrap();
    if let Some(resources) = store.get(&rtype)
        && let Some(resource) = resources
            .iter()
            .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(&id))
    {
        // Handle conditional read headers
        if headers.contains_key("if-none-match") || headers.contains_key("if-modified-since") {
            tracing::debug!("Mock GET /{}/{} → 304 Not Modified", rtype, id);
            return (StatusCode::NOT_MODIFIED, Json(serde_json::json!({})));
        }
        tracing::debug!("Mock GET /{}/{} → 200 OK", rtype, id);
        return (StatusCode::OK, Json(resource.clone()));
    }
    tracing::debug!("Mock GET /{}/{} → 404 Not Found", rtype, id);
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
    _include: Option<Vec<String>>,
    #[serde(default)]
    _revinclude: Option<Vec<String>>,
    #[serde(default)]
    _elements: Option<String>,
    #[serde(default)]
    _total: Option<String>,
    #[serde(default)]
    _filter: Option<String>,
    #[serde(default)]
    _source: Option<String>,
    #[serde(default)]
    _language: Option<String>,
    #[serde(default)]
    _contained: Option<String>,
    #[serde(default, rename = "_containedType")]
    _contained_type: Option<String>,
    #[serde(default)]
    _getpagesoffset: Option<u32>,
    // Accept any other params without erroring
    #[serde(flatten)]
    _rest: HashMap<String, String>,
}

/// Parsed search parameter with optional modifier and prefix.
struct ParsedParam<'a> {
    field: &'a str,
    modifier: Option<&'a str>,
    prefix: Option<&'a str>,
    value: &'a str,
}

/// Parse a search parameter key like `family:exact` or `name:contains`
/// into (field, modifier). Returns (field, None) if no modifier.
fn parse_param_key(key: &str) -> (&str, Option<&str>) {
    if let Some(pos) = key.find(':') {
        (&key[..pos], Some(&key[pos + 1..]))
    } else {
        (key, None)
    }
}

/// Parse a search parameter value like `gt2020-01-01` or `eq123`
/// into (prefix, value). Returns (None, value) if no prefix.
fn parse_param_value(value: &str) -> (Option<&str>, &str) {
    let prefixes = ["eq", "ne", "gt", "lt", "ge", "le", "sa", "eb", "ap"];
    for p in &prefixes {
        if let Some(rest) = value.strip_prefix(p) {
            return (Some(p), rest);
        }
    }
    (None, value)
}

/// Strip FHIR modifiers like :recurse and :iterate from a parameter value
/// so the mock server can process the base include/revinclude.
fn strip_include_modifiers(param: &str) -> String {
    param
        .split(':')
        .filter(|part| *part != "recurse" && *part != "iterate")
        .collect::<Vec<_>>()
        .join(":")
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
    // Supports modifiers (:exact, :contains, :missing, :not) and
    // prefixes (eq, ne, gt, lt, ge, le).
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
                let (field, modifier) = parse_param_key(key);
                let (prefix, value) = parse_param_value(desired);
                let parsed = ParsedParam {
                    field,
                    modifier,
                    prefix,
                    value,
                };
                // Check top-level field and nested fields (name.family, etc.)
                match_field_with_modifiers(r, &parsed)
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
    match params._summary.as_deref() {
        Some("true") => {
            resources = resources
                .into_iter()
                .map(|r| {
                    let summary = serde_json::json!({
                        "resourceType": r["resourceType"],
                        "id": r["id"],
                        "meta": r["meta"],
                    });
                    summary
                })
                .collect();
        }
        Some("count") => {
            // _summary=count: return total but no entries
            // Resources are cleared; total is preserved
            resources.clear();
        }
        Some("text") => {
            // _summary=text: ensure text field is present
            // (no-op in mock — resources already have their fields)
        }
        Some("data") => {
            // _summary=data: remove text field from resources
            resources = resources
                .into_iter()
                .map(|mut r| {
                    r.as_object_mut().map(|obj| obj.remove("text"));
                    r
                })
                .collect();
        }
        _ => {}
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
    if let Some(include_params) = params._include.as_deref() {
        for include_param in include_params {
            // Strip :recurse and :iterate modifiers for mock processing
            let clean = strip_include_modifiers(include_param);
            // Format: ResourceType:search-parameter or ResourceType:search-parameter:targetType
            let parts: Vec<&str> = clean.split(':').collect();
            if parts.len() >= 2 {
                let search_param = parts[1];
                // Map search parameter names to actual JSON field names.
                // Different resource types use different field names for the same
                // search parameter (e.g. Location uses managingOrganization for
                // the "organization" search param).
                let field_name = match (rtype.as_str(), search_param) {
                    ("Location", "organization") => "managingOrganization",
                    ("PractitionerRole", "service") => "healthcareService",
                    ("HealthcareService", "organization") => "providedBy",
                    _ => search_param,
                };
                // Collect referenced resource IDs from the matching resources
                let mut included_resources = Vec::new();
                for r in &resources {
                    // References can be single objects or arrays
                    let ref_values: Vec<serde_json::Value> = {
                        let field_val = r.get(field_name);
                        if let Some(arr) = field_val.and_then(|v| v.as_array()) {
                            arr.clone()
                        } else if let Some(obj) = field_val.and_then(|v| v.as_object()) {
                            vec![serde_json::Value::Object(obj.clone())]
                        } else {
                            Vec::new()
                        }
                    };
                    for reference in &ref_values {
                        if let Some(ref_str) = reference.get("reference").and_then(|v| v.as_str()) {
                            // Parse "ResourceType/id" from the reference
                            if let Some((ref_type, ref_id)) = ref_str.split_once('/')
                                && let Some(ref_resources) = store.get(ref_type)
                                && let Some(found) = ref_resources.iter().find(|rr| {
                                    rr.get("id").and_then(|v| v.as_str()) == Some(ref_id)
                                })
                            {
                                included_resources.push(serde_json::json!({
                                    "resource": found,
                                    "fullUrl": format!("http://localhost/fhir/{}/{}", ref_type, ref_id)
                                }));
                            }
                        }
                    }
                }
                entries.extend(included_resources);
            }
        }
    }

    // Handle _revinclude: include resources that reference the matched resources
    if let Some(revinclude_params) = params._revinclude.as_deref() {
        for revinclude_param in revinclude_params {
            // Strip :recurse and :iterate modifiers for mock processing
            let clean = strip_include_modifiers(revinclude_param);
            // Format: SourceType:search-parameter
            let parts: Vec<&str> = clean.split(':').collect();
            if parts.len() >= 2 {
                let source_type = parts[0];
                let search_param = parts[1];
                if let Some(source_resources) = store.get(source_type) {
                    let mut rev_included = Vec::new();
                    for r in &resources {
                        let rid = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        for source in source_resources {
                            // References can be single objects or arrays
                            let ref_values: Vec<serde_json::Value> = {
                                let field_val = source.get(search_param);
                                if let Some(arr) = field_val.and_then(|v| v.as_array()) {
                                    arr.clone()
                                } else if let Some(obj) = field_val.and_then(|v| v.as_object()) {
                                    vec![serde_json::Value::Object(obj.clone())]
                                } else {
                                    Vec::new()
                                }
                            };
                            for reference in &ref_values {
                                if let Some(ref_str) =
                                    reference.get("reference").and_then(|v| v.as_str())
                                    && ref_str == format!("{}/{}", rtype, rid)
                                {
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
                    entries.extend(rev_included);
                }
            }
        }
    }

    // Handle _total: control whether total field is present
    let include_total = !matches!(params._total.as_deref(), Some("none"));

    // Build response
    let mut response = serde_json::json!({
        "resourceType": "Bundle",
        "type": "searchset",
        "entry": entries
    });
    if include_total {
        response["total"] = serde_json::json!(total_before_count);
    }

    tracing::debug!(
        "Mock GET /{}? → 200 OK ({} results, total={})",
        rtype,
        resources.len(),
        total_before_count
    );

    (StatusCode::OK, Json(response))
}

/// Try to match a search parameter against a resource field, supporting
/// FHIR modifiers (`:exact`, `:contains`, `:missing`, `:not`) and
/// prefixes (`eq`, `ne`, `gt`, `lt`, `ge`, `le`).
///
/// Handles top-level fields (e.g. `active` → r.active) and
/// nested fields with dotted notation (e.g. `name` checks r.name[].family).
fn match_field_with_modifiers(resource: &serde_json::Value, param: &ParsedParam) -> bool {
    let value_lower = param.value.to_lowercase();

    // Handle :missing modifier
    if param.modifier == Some("missing") {
        let present = resource.get(param.field).is_some()
            && resource.get(param.field).and_then(|v| v.as_str()) != Some("");
        return match param.value {
            "true" => !present,
            "false" => present,
            _ => false,
        };
    }

    // Handle :not modifier — invert the match
    if param.modifier == Some("not") {
        return !match_field_inner(
            resource,
            param.field,
            param.value,
            &value_lower,
            param.prefix,
            param.modifier,
        );
    }

    // Default matching with optional modifier
    match_field_inner(
        resource,
        param.field,
        param.value,
        &value_lower,
        param.prefix,
        param.modifier,
    )
}

/// Core field matching logic, without :not/:missing modifiers.
fn match_field_inner(
    resource: &serde_json::Value,
    field: &str,
    _value: &str,
    value_lower: &str,
    prefix: Option<&str>,
    modifier: Option<&str>,
) -> bool {
    // Direct top-level match
    if let Some(v) = resource.get(field)
        && json_contains_with_modifier(v, value_lower, prefix, modifier)
    {
        return true;
    }

    // Token-style: check coding.code and coding.display
    if (field == "code" || field.ends_with("-code"))
        && let Some(codings) = find_all_codings(resource)
    {
        return codings.iter().any(|c| {
            c.get("code")
                .or_else(|| c.get("display"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase() == value_lower || s.to_lowercase().contains(value_lower))
                .unwrap_or(false)
        });
    }

    // Name search: check HumanName arrays
    if (field == "name" || field == "family" || field == "given")
        && let Some(names) = resource.get("name").and_then(|n| n.as_array())
    {
        for name in names {
            if let Some(family) = name.get("family").and_then(|f| f.as_str())
                && name_value_matches(family, value_lower, modifier)
            {
                return true;
            }
            if let Some(given) = name.get("given").and_then(|g| g.as_array()) {
                for g in given {
                    if let Some(g_str) = g.as_str()
                        && name_value_matches(g_str, value_lower, modifier)
                    {
                        return true;
                    }
                }
            }
        }
    }

    // Identifier search
    if field == "identifier"
        && let Some(ids) = resource.get("identifier").and_then(|i| i.as_array())
    {
        return ids.iter().any(|id| {
            id.get("value")
                .and_then(|v| v.as_str())
                .map(|s| name_value_matches(s, value_lower, modifier))
                .unwrap_or(false)
        });
    }

    false
}

/// Check if a string value matches a search term, respecting the modifier.
fn name_value_matches(value: &str, search_lower: &str, modifier: Option<&str>) -> bool {
    let v_lower = value.to_lowercase();
    match modifier {
        Some("exact") => v_lower == search_lower,
        _ => v_lower.contains(search_lower),
    }
}

/// Check if a JSON value matches a search string, with optional prefix comparison
/// and modifier support (`:exact`, `:contains`).
///
/// Without a modifier, uses case-insensitive substring matching (default FHIR
/// string search behavior). With `:exact`, requires exact case-insensitive match.
/// With `:contains`, uses substring matching (same as default).
/// Prefixes (`ne`, `gt`, `lt`, `ge`, `le`) invert or compare the string value.
fn json_contains_with_modifier(
    value: &serde_json::Value,
    search: &str,
    prefix: Option<&str>,
    modifier: Option<&str>,
) -> bool {
    match value {
        serde_json::Value::String(s) => {
            let s_lower = s.to_lowercase();
            // :exact modifier takes precedence over prefix
            if modifier == Some("exact") {
                return s_lower == search;
            }
            match prefix {
                Some("ne") => !s_lower.contains(search),
                Some("gt") => s_lower.as_str() > search,
                Some("lt") => s_lower.as_str() < search,
                Some("ge") => s_lower.as_str() >= search,
                Some("le") => s_lower.as_str() <= search,
                _ => s_lower.contains(search),
            }
        }
        serde_json::Value::Bool(b) => {
            let b_str = b.to_string();
            match prefix {
                Some("ne") => search != b_str,
                Some("gt") => b_str.as_str() > search,
                Some("lt") => b_str.as_str() < search,
                Some("ge") => b_str.as_str() >= search,
                Some("le") => b_str.as_str() <= search,
                _ => search == b_str,
            }
        }
        serde_json::Value::Number(n) => {
            let n_str = n.to_string();
            match prefix {
                Some("ne") => search != n_str,
                Some("gt") => n_str.as_str() > search,
                Some("lt") => n_str.as_str() < search,
                Some("ge") => n_str.as_str() >= search,
                Some("le") => n_str.as_str() <= search,
                _ => search == n_str,
            }
        }
        serde_json::Value::Array(arr) => arr
            .iter()
            .any(|v| json_contains_with_modifier(v, search, prefix, modifier)),
        serde_json::Value::Object(map) => map
            .values()
            .any(|v| json_contains_with_modifier(v, search, prefix, modifier)),
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
        tracing::debug!("Mock PUT /{}/{} → 200 OK (updated)", rtype, id);
        (StatusCode::OK, Json(body))
    } else {
        // Update-as-create: resource doesn't exist yet, create it
        resources.push(body.clone());
        tracing::debug!(
            "Mock PUT /{}/{} → 201 Created (update-as-create)",
            rtype,
            id
        );
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
            tracing::debug!("Mock DELETE /{}/{} → 200 OK (deleted)", rtype, id);
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "resourceType": "OperationOutcome",
                    "issue": [{"severity": "information", "code": "informational", "diagnostics": "Deleted"}]
                })),
            );
        }
    }
    tracing::debug!("Mock DELETE /{}/{} → 404 Not Found", rtype, id);
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
    Path(op): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    operation_response(&op)
}

async fn operation_handler_with_type(
    State(_store): State<MockStore>,
    Path((_rtype, op)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    operation_response(&op)
}

fn operation_response(op: &str) -> (StatusCode, Json<serde_json::Value>) {
    // The wildcard route captures the operation name without the $ prefix
    match op {
        "export" | "$export" => (
            StatusCode::OK,
            Json(serde_json::json!({
                "resourceType": "Bundle",
                "type": "collection",
                "entry": []
            })),
        ),
        "validate" | "$validate" => (
            StatusCode::OK,
            Json(serde_json::json!({
                "resourceType": "OperationOutcome",
                "issue": [{"severity": "information", "code": "informational", "diagnostics": "Validation successful (mock)"}]
            })),
        ),
        _ => (
            StatusCode::OK,
            Json(serde_json::json!({
                "resourceType": "Parameters",
                "parameter": [{"name": "result", "valueBoolean": true}]
            })),
        ),
    }
}

async fn history_handler(
    State(store): State<MockStore>,
    Path((rtype, id)): Path<(String, String)>,
    Query(params): Query<SearchParams>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = store.lock().unwrap();
    let resources = store.get(&rtype).cloned().unwrap_or_default();

    // Find the specific resource
    let resource = resources
        .iter()
        .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(&id));

    match resource {
        Some(r) => {
            // Build a history Bundle with the resource
            let mut entries = Vec::new();
            entries.push(serde_json::json!({
                "resource": r,
                "fullUrl": format!("http://localhost/fhir/{}/{}/_history/1", rtype, id),
            }));

            // Apply _count if present
            if let Some(count) = params._count {
                entries.truncate(count as usize);
            }

            let mut response = serde_json::json!({
                "resourceType": "Bundle",
                "type": "history",
                "entry": entries,
            });
            response["total"] = serde_json::json!(entries.len());

            (StatusCode::OK, Json(response))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "resourceType": "OperationOutcome",
                "issue": [{"severity": "error", "code": "not-found", "diagnostics": format!("{}/{} not found", rtype, id)}]
            })),
        ),
    }
}

async fn history_type_handler(
    State(store): State<MockStore>,
    Path(rtype): Path<String>,
    Query(params): Query<SearchParams>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = store.lock().unwrap();
    let resources = store.get(&rtype).cloned().unwrap_or_default();

    let mut entries: Vec<serde_json::Value> = resources
        .iter()
        .map(|r| {
            serde_json::json!({
                "resource": r,
                "fullUrl": format!("http://localhost/fhir/{}/{}/_history/1", rtype, r["id"].as_str().unwrap_or("")),
            })
        })
        .collect();

    // Apply _count if present
    if let Some(count) = params._count {
        entries.truncate(count as usize);
    }

    let total_before = resources.len();
    let mut response = serde_json::json!({
        "resourceType": "Bundle",
        "type": "history",
        "entry": entries,
    });
    response["total"] = serde_json::json!(total_before);

    (StatusCode::OK, Json(response))
}

/// Handle system-level search (`GET /?_type=...`).
/// Searches across multiple resource types.
async fn system_search_handler(
    State(store): State<MockStore>,
    Query(params): Query<SearchParams>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = store.lock().unwrap();

    // Determine which resource types to search
    let type_param = params._rest.get("_type");
    let types: Vec<&str> = type_param
        .map(|t| t.split(',').collect())
        .unwrap_or_default();

    let mut all_entries = Vec::new();
    let mut total = 0usize;

    if types.is_empty() {
        // No _type filter: search all resource types
        for (rtype, resources) in store.iter() {
            for r in resources {
                all_entries.push(serde_json::json!({
                    "resource": r,
                    "fullUrl": format!("http://localhost/fhir/{}/{}", rtype, r["id"].as_str().unwrap_or(""))
                }));
                total += 1;
            }
        }
    } else {
        for t in &types {
            if let Some(resources) = store.get(*t) {
                for r in resources {
                    all_entries.push(serde_json::json!({
                        "resource": r,
                        "fullUrl": format!("http://localhost/fhir/{}/{}", t, r["id"].as_str().unwrap_or(""))
                    }));
                    total += 1;
                }
            }
        }
    }

    // Apply _count
    if let Some(count) = params._count {
        all_entries.truncate(count as usize);
    }

    let response = serde_json::json!({
        "resourceType": "Bundle",
        "type": "searchset",
        "total": total,
        "entry": all_entries
    });

    (StatusCode::OK, Json(response))
}

/// Handle batch/transaction operations (`POST /`).
/// Returns a Bundle with type matching the request.
async fn batch_transaction_handler(
    State(_store): State<MockStore>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let bundle_type = body.get("type").and_then(|v| v.as_str()).unwrap_or("batch");

    let response_type = match bundle_type {
        "batch" => "batch-response",
        "transaction" => "transaction-response",
        _ => "batch-response",
    };

    let entries: Vec<serde_json::Value> = body
        .get("entry")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .map(|_entry| {
                    serde_json::json!({
                        "response": {
                            "status": "200 OK"
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let response = serde_json::json!({
        "resourceType": "Bundle",
        "type": response_type,
        "entry": entries
    });

    (StatusCode::OK, Json(response))
}

/// Build the mock FHIR server axum Router.
pub fn create_mock_app() -> Router {
    let store: MockStore = Arc::new(Mutex::new(HashMap::new()));
    Router::new()
        // Operation routes must come before resource routes so $export etc.
        // don't match as a resource type.
        .route(
            "/fhir/${*op}",
            get(operation_handler).post(operation_handler),
        )
        .route(
            "/fhir/{rtype}/${*op}",
            get(operation_handler_with_type).post(operation_handler_with_type),
        )
        // System-level search (GET /) and batch/transaction (POST /)
        .route(
            "/fhir/",
            get(system_search_handler).post(batch_transaction_handler),
        )
        .route("/fhir/{rtype}", post(create_resource))
        .route("/fhir/{rtype}", get(search_resources))
        .route("/fhir/{rtype}/{id}", get(read_resource))
        .route("/fhir/{rtype}/{id}", put(update_resource))
        .route("/fhir/{rtype}/{id}", delete(delete_resource))
        .route("/fhir/{rtype}/{id}/_history", get(history_handler))
        .route("/fhir/{rtype}/_history", get(history_type_handler))
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

    #[tokio::test]
    async fn test_search_with_exact_modifier() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create resources
        for (i, family) in ["Smith", "Smithson", "Smith"].iter().enumerate() {
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

        // Search with :exact modifier — should only match exact "Smith"
        let resp = client
            .get(format!("{}/Patient?family:exact=Smith", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 2);
    }

    #[tokio::test]
    async fn test_search_with_missing_modifier() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create a Patient with active=true and one without active field
        client
            .post(format!("{}/Patient", base_url))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "id": "pat-active",
                "active": true,
                "name": [{"family": "Active"}]
            }))
            .send()
            .await
            .unwrap();

        client
            .post(format!("{}/Patient", base_url))
            .header("Content-Type", "application/fhir+json")
            .json(&serde_json::json!({
                "resourceType": "Patient",
                "id": "pat-inactive",
                "name": [{"family": "Inactive"}]
            }))
            .send()
            .await
            .unwrap();

        // Search with :missing=true — should find the one without active field
        let resp = client
            .get(format!("{}/Patient?active:missing=true", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 1);
        assert_eq!(
            body["entry"][0]["resource"]["name"][0]["family"],
            "Inactive"
        );

        // Search with :missing=false — should find the one with active field
        let resp = client
            .get(format!("{}/Patient?active:missing=false", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 1);
        assert_eq!(body["entry"][0]["resource"]["name"][0]["family"], "Active");
    }

    #[tokio::test]
    async fn test_search_with_not_modifier() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create resources
        for (i, family) in ["Smith", "Jones", "Brown"].iter().enumerate() {
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

        // Search with :not modifier — should exclude Smith
        let resp = client
            .get(format!("{}/Patient?family:not=Smith", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 2);
    }

    #[tokio::test]
    async fn test_search_with_prefix() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        // Create resources with different ages
        for (i, age) in [20, 30, 40].iter().enumerate() {
            client
                .post(format!("{}/Patient", base_url))
                .header("Content-Type", "application/fhir+json")
                .json(&serde_json::json!({
                    "resourceType": "Patient",
                    "id": format!("pat-{}", i),
                    "name": [{"family": format!("Test{}", i)}],
                    "age": age
                }))
                .send()
                .await
                .unwrap();
        }

        // Search with gt prefix — should find age > 25 (30 and 40)
        let resp = client
            .get(format!("{}/Patient?age=gt25", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 2);

        // Search with lt prefix — should find age < 35 (20 and 30)
        let resp = client
            .get(format!("{}/Patient?age=lt35", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["total"], 2);
    }

    #[tokio::test]
    async fn test_operation_handler_export() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{}/$export", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["resourceType"], "Bundle");
        assert_eq!(body["type"], "collection");
    }

    #[tokio::test]
    async fn test_operation_handler_validate() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{}/Patient/$validate", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["resourceType"], "OperationOutcome");
    }

    #[tokio::test]
    async fn test_operation_handler_unknown() {
        let base_url = setup_server().await;
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{}/$unknown-op", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["resourceType"], "Parameters");
    }
}
