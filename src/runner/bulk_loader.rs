use crate::config::models::WriteEndpoint;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

/// HL7 R5 extension StructureDefinitions that the HCPD profile references for slicing.
/// These must be available in the HAPI validator's registry or validation of Practitioner
/// resources will fail with "Slicing cannot be evaluated" errors.
///
/// These are minimal but valid StructureDefinitions sufficient for the HAPI validator
/// to resolve profile URIs in extension slicing discriminators.
const R5_EXTENSION_PROFILES: &[(&str, &str, &str)] = &[
    (
        "individual-recordedSexOrGender",
        "http://hl7.org/fhir/StructureDefinition/individual-recordedSexOrGender",
        r#"{
  "resourceType": "StructureDefinition",
  "id": "individual-recordedSexOrGender",
  "url": "http://hl7.org/fhir/StructureDefinition/individual-recordedSexOrGender",
  "version": "5.3.0",
  "name": "IndividualRecordedSexOrGender",
  "title": "Individual Recorded Sex Or Gender",
  "status": "active",
  "kind": "complex-type",
  "abstract": false,
  "context": [{"type": "element", "expression": "DomainResource"}],
  "type": "Extension",
  "baseDefinition": "http://hl7.org/fhir/StructureDefinition/Extension",
  "derivation": "constraint",
  "snapshot": {
    "element": [
      {"id": "Extension", "path": "Extension", "min": 0, "max": "*",
       "type": [{"code": "Extension"}]},
      {"id": "Extension.extension", "path": "Extension.extension", "min": 0, "max": "*",
       "slicing": {"discriminator": [{"type": "value", "path": "url"}], "rules": "open"},
       "type": [{"code": "Extension"}]},
      {"id": "Extension.extension:value", "path": "Extension.extension",
       "sliceName": "value", "min": 0, "max": "1",
       "type": [{"code": "Extension"}]},
      {"id": "Extension.extension:value.url", "path": "Extension.extension.url",
       "min": 1, "max": "1", "fixedUri": "value"},
      {"id": "Extension.extension:value.value[x]", "path": "Extension.extension.value[x]",
       "min": 1, "max": "1", "type": [{"code": "CodeableConcept"}]},
      {"id": "Extension.url", "path": "Extension.url", "min": 1, "max": "1",
       "fixedUri": "http://hl7.org/fhir/StructureDefinition/individual-recordedSexOrGender"},
      {"id": "Extension.value[x]", "path": "Extension.value[x]", "min": 0, "max": "0"}
    ]
  }
}"#,
    ),
    (
        "individual-genderIdentity",
        "http://hl7.org/fhir/StructureDefinition/individual-genderIdentity",
        r#"{
  "resourceType": "StructureDefinition",
  "id": "individual-genderIdentity",
  "url": "http://hl7.org/fhir/StructureDefinition/individual-genderIdentity",
  "version": "5.3.0",
  "name": "IndividualGenderIdentity",
  "title": "Individual Gender Identity",
  "status": "active",
  "kind": "complex-type",
  "abstract": false,
  "context": [{"type": "element", "expression": "DomainResource"}],
  "type": "Extension",
  "baseDefinition": "http://hl7.org/fhir/StructureDefinition/Extension",
  "derivation": "constraint",
  "snapshot": {
    "element": [
      {"id": "Extension", "path": "Extension", "min": 0, "max": "*",
       "type": [{"code": "Extension"}]},
      {"id": "Extension.extension", "path": "Extension.extension", "min": 0, "max": "*",
       "slicing": {"discriminator": [{"type": "value", "path": "url"}], "rules": "open"},
       "type": [{"code": "Extension"}]},
      {"id": "Extension.url", "path": "Extension.url", "min": 1, "max": "1",
       "fixedUri": "http://hl7.org/fhir/StructureDefinition/individual-genderIdentity"},
      {"id": "Extension.value[x]", "path": "Extension.value[x]", "min": 0, "max": "0"}
    ]
  }
}"#,
    ),
    (
        "individual-pronouns",
        "http://hl7.org/fhir/StructureDefinition/individual-pronouns",
        r#"{
  "resourceType": "StructureDefinition",
  "id": "individual-pronouns",
  "url": "http://hl7.org/fhir/StructureDefinition/individual-pronouns",
  "version": "5.3.0",
  "name": "IndividualPronouns",
  "title": "Individual Pronouns",
  "status": "active",
  "kind": "complex-type",
  "abstract": false,
  "context": [{"type": "element", "expression": "DomainResource"}],
  "type": "Extension",
  "baseDefinition": "http://hl7.org/fhir/StructureDefinition/Extension",
  "derivation": "constraint",
  "snapshot": {
    "element": [
      {"id": "Extension", "path": "Extension", "min": 0, "max": "*",
       "type": [{"code": "Extension"}]},
      {"id": "Extension.extension", "path": "Extension.extension", "min": 0, "max": "*",
       "slicing": {"discriminator": [{"type": "value", "path": "url"}], "rules": "open"},
       "type": [{"code": "Extension"}]},
      {"id": "Extension.url", "path": "Extension.url", "min": 1, "max": "1",
       "fixedUri": "http://hl7.org/fhir/StructureDefinition/individual-pronouns"},
      {"id": "Extension.value[x]", "path": "Extension.value[x]", "min": 0, "max": "0"}
    ]
  }
}"#,
    ),
];

/// Ensure the required HL7 R5 extension StructureDefinitions are present in the
/// FHIR repository. If a profile is missing (404), uploads the embedded minimal
/// StructureDefinition so the HAPI validator can resolve profile URIs used in
/// extension slicing discriminators.
pub async fn ensure_r5_extension_profiles(write_endpoint: &WriteEndpoint) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let base_url = match write_endpoint {
        WriteEndpoint::Repository { base_url, .. } => base_url,
        WriteEndpoint::Server { base_url, .. } => base_url,
    };

    for (id, canonical_url, embedded_json) in R5_EXTENSION_PROFILES {
        let repo_url = format!("{}/StructureDefinition/{}", base_url, id);

        // Always PUT the embedded StructureDefinition to ensure the latest version
        // is present. HAPI handles idempotent updates gracefully.

        // Parse the embedded minimal StructureDefinition
        let sd_json: serde_json::Value = serde_json::from_str(embedded_json)
            .with_context(|| format!("Failed to parse embedded StructureDefinition for {}", id))?;

        // Upload to repository
        let put_req = client
            .put(&repo_url)
            .header("Content-Type", "application/fhir+json")
            .header("Accept", "application/fhir+json")
            .json(&sd_json);
        let put_req = add_write_auth(put_req, write_endpoint);

        match put_req.send().await {
            Ok(r) if r.status().as_u16() < 300 => {
                tracing::info!("Uploaded R5 extension profile: {}", canonical_url);
                println!("  Uploaded R5 profile: {}", canonical_url);
            }
            Ok(r) => {
                tracing::warn!("Failed to upload R5 profile {} (HTTP {})", id, r.status());
            }
            Err(e) => {
                tracing::warn!("Error uploading R5 profile {}: {}", id, e);
            }
        }
    }

    Ok(())
}

/// Upload NDJSON files to the FHIR repository and return IDs per resource type.
///
/// For each resource type in `creation_order`, reads the NDJSON file from
/// `{data_dir}/{ResourceType}.ndjson` and uploads each resource to the repository.
/// Upload one resource per uncovered resource type to the repository.
///
/// For each type in `creation_order` that has no entry in `bulk_counts` (or count = 0),
/// generates a single resource using the profile-aware generator, assigns it the
/// predictable ID `{resourcetype}-1`, and PUTs it to the repository.
///
/// This ensures that conformance must_support tests — which search by `_id={type}-1` —
/// can always find a matching resource regardless of which resource types are configured
/// in `data_generation.counts`. Works with any FHIR IG.
pub async fn upload_supplement_resources(
    creation_order: &[String],
    bulk_counts: &std::collections::HashMap<String, u64>,
    profile_urls: &std::collections::HashMap<String, String>,
    profiles: &[crate::model::StructureDefinition],
    value_set_systems: &std::collections::HashMap<String, String>,
    write_endpoint: &WriteEndpoint,
) -> Result<HashMap<String, Vec<String>>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let base_url = match write_endpoint {
        WriteEndpoint::Repository { base_url, .. } => base_url,
        WriteEndpoint::Server { base_url, .. } => base_url,
    };

    let mut supplement_ids: HashMap<String, Vec<String>> = HashMap::new();
    let mut any_uploaded = false;

    for resource_type in creation_order {
        let count = bulk_counts.get(resource_type).copied().unwrap_or(0);
        if count > 0 {
            continue; // Already covered by bulk data
        }

        // Skip FHIR data types that are not independently creatable resources.
        // Some CapabilityStatements list types like Extension or Identifier which
        // are structural types, not top-level FHIR resources.
        if crate::generate::NON_RESOURCE_TYPES.contains(&resource_type.as_str()) {
            continue;
        }

        // Generate a single resource for this type
        let resource = match crate::generate::generate_supplement_resource(
            resource_type,
            profile_urls,
            profiles,
            value_set_systems,
            &std::collections::HashMap::new(),
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "Could not generate supplement resource for {}: {}",
                    resource_type,
                    e
                );
                continue;
            }
        };

        let id = format!("{}-1", resource_type.to_lowercase());
        let url = format!("{}/{}/{}", base_url, resource_type, id);

        if !any_uploaded {
            println!("\n── Uploading supplement resources (uncovered types) ──");
            any_uploaded = true;
        }

        let req = client
            .put(&url)
            .header("Content-Type", "application/fhir+json")
            .header("Accept", "application/fhir+json")
            .json(&resource);
        let req = add_write_auth(req, write_endpoint);

        match req.send().await {
            Ok(r) if r.status().as_u16() < 300 => {
                tracing::info!("Uploaded supplement {} ({})", resource_type, id);
                println!("  {} {}", resource_type, id);
                supplement_ids
                    .entry(resource_type.clone())
                    .or_default()
                    .push(id);
            }
            Ok(r) => {
                let status = r.status();
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                tracing::warn!(
                    "Failed to upload supplement {} (HTTP {}): {:?}",
                    resource_type,
                    status,
                    body
                );
            }
            Err(e) => {
                tracing::warn!("Error uploading supplement {}: {}", resource_type, e);
            }
        }
    }

    Ok(supplement_ids)
}

/// Upload NDJSON files to the FHIR repository and return IDs per resource type.
///
/// For each resource type in `creation_order`, reads the NDJSON file from
/// `{data_dir}/{ResourceType}.ndjson` and uploads each resource to the repository.
/// Returns a map of resource type → list of server-assigned IDs.
///
/// Uses PUT (update-as-create) by default, or POST if `upload_method` is "POST".
/// Uses concurrency (up to `concurrency` parallel requests) for throughput.
pub async fn upload_ndjson_files(
    data_dir: &Path,
    creation_order: &[String],
    write_endpoint: &WriteEndpoint,
    concurrency: usize,
) -> Result<HashMap<String, Vec<String>>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let base_url = match write_endpoint {
        WriteEndpoint::Repository { base_url, .. } => base_url,
        WriteEndpoint::Server { base_url, .. } => base_url,
    };

    let upload_method = match write_endpoint {
        WriteEndpoint::Repository { upload_method, .. }
        | WriteEndpoint::Server { upload_method, .. } => upload_method.to_uppercase(),
    };

    let mut all_ids: HashMap<String, Vec<String>> = HashMap::new();

    for resource_type in creation_order {
        let ndjson_path = data_dir.join(format!("{}.ndjson", resource_type));
        if !ndjson_path.exists() {
            tracing::warn!("No NDJSON file for {}, skipping upload", resource_type);
            continue;
        }

        let file = std::fs::File::open(&ndjson_path)
            .with_context(|| format!("Failed to open {}", ndjson_path.display()))?;
        let reader = std::io::BufReader::new(file);
        let lines: Vec<String> = reader
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .collect();

        let total = lines.len();
        if total == 0 {
            tracing::warn!("Empty NDJSON file for {}", resource_type);
            continue;
        }

        println!("  Uploading {} {} resources ...", total, resource_type);
        tracing::info!(
            "Uploading {} {} resources to {}",
            total,
            resource_type,
            base_url
        );

        let mut ids: Vec<String> = Vec::with_capacity(total);
        let mut uploaded = 0usize;
        let mut errors = 0usize;

        // Process in batches for concurrency control
        let batch_size = concurrency.max(1);
        for chunk in lines.chunks(batch_size) {
            let mut handles = Vec::new();

            for line in chunk {
                let resource: serde_json::Value = serde_json::from_str(line)
                    .with_context(|| format!("Invalid JSON in {}.ndjson", resource_type))?;
                let mut resource = resource;
                let client_id = resource
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                // POST: remove client id — let the server assign one
                if upload_method != "PUT" {
                    resource.as_object_mut().map(|o| o.remove("id"));
                }

                let url = if upload_method == "PUT" {
                    // PUT /{rtype}/{id} — update-as-create with client-assigned ID
                    format!("{}/{}/{}", base_url, resource_type, client_id)
                } else {
                    // POST /{rtype} — server-assigned ID
                    format!("{}/{}", base_url, resource_type)
                };

                let client = client.clone();
                let write_endpoint = write_endpoint.clone();
                let upload_method = upload_method.clone();

                handles.push(tokio::spawn(async move {
                    let req = if upload_method == "PUT" {
                        client
                            .put(&url)
                            .header("Content-Type", "application/fhir+json")
                            .header("Accept", "application/fhir+json")
                            .json(&resource)
                    } else {
                        client
                            .post(&url)
                            .header("Content-Type", "application/fhir+json")
                            .header("Accept", "application/fhir+json")
                            .json(&resource)
                    };
                    let req = add_write_auth(req, &write_endpoint);
                    let resp = req.send().await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    anyhow::Ok((client_id, status.as_u16(), body))
                }));
            }

            for handle in handles {
                match handle.await {
                    Ok(Ok((client_id, status, body))) => {
                        if status == 201 || status == 200 {
                            let server_id = body
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&client_id)
                                .to_string();
                            ids.push(server_id);
                        } else {
                            tracing::warn!(
                                "Failed to create {} resource: HTTP {} — {:?}",
                                resource_type,
                                status,
                                body
                            );
                            errors += 1;
                        }
                        uploaded += 1;
                        if uploaded.is_multiple_of(1000) {
                            println!("    {}/{} {} uploaded", uploaded, total, resource_type);
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Request error for {}: {}", resource_type, e);
                        errors += 1;
                        uploaded += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Task error for {}: {}", resource_type, e);
                        errors += 1;
                        uploaded += 1;
                    }
                }
            }
        }

        println!(
            "  → {}/{} {} created ({} errors)",
            ids.len(),
            total,
            resource_type,
            errors
        );
        all_ids.insert(resource_type.clone(), ids);
    }

    Ok(all_ids)
}

/// Delete all resources in `ids` from the repository, in reverse creation order.
///
/// Uses concurrency for throughput. Errors are logged but not fatal —
/// best-effort cleanup.
pub async fn delete_all_resources(
    ids: &HashMap<String, Vec<String>>,
    creation_order: &[String],
    write_endpoint: &WriteEndpoint,
    concurrency: usize,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let base_url = match write_endpoint {
        WriteEndpoint::Repository { base_url, .. } => base_url,
        WriteEndpoint::Server { base_url, .. } => base_url,
    };

    // Delete in reverse creation order
    for resource_type in creation_order.iter().rev() {
        if let Some(type_ids) = ids.get(resource_type) {
            if type_ids.is_empty() {
                continue;
            }
            let total = type_ids.len();
            println!("  Deleting {} {} resources ...", total, resource_type);

            let mut deleted = 0usize;
            let mut errors = 0usize;
            let batch_size = concurrency.max(1);

            for chunk in type_ids.chunks(batch_size) {
                let mut handles = Vec::new();

                for id in chunk {
                    let url = format!("{}/{}/{}", base_url, resource_type, id);
                    let client = client.clone();
                    let write_endpoint = write_endpoint.clone();

                    handles.push(tokio::spawn(async move {
                        let req = client
                            .delete(&url)
                            .header("Accept", "application/fhir+json");
                        let req = add_write_auth(req, &write_endpoint);
                        let resp = req.send().await?;
                        Ok::<u16, anyhow::Error>(resp.status().as_u16())
                    }));
                }

                for handle in handles {
                    match handle.await {
                        Ok(Ok(200 | 204 | 410)) => {
                            deleted += 1;
                        }
                        _ => {
                            errors += 1;
                        }
                    }
                }

                if deleted > 0 && deleted.is_multiple_of(1000) {
                    println!("    {}/{} {} deleted", deleted, total, resource_type);
                }
            }

            println!(
                "  → {}/{} {} deleted ({} errors)",
                deleted, total, resource_type, errors
            );
        }
    }

    Ok(())
}

/// Add write auth headers to a request based on the endpoint config.
fn add_write_auth(
    req: reqwest::RequestBuilder,
    endpoint: &WriteEndpoint,
) -> reqwest::RequestBuilder {
    match endpoint {
        WriteEndpoint::Repository {
            username, password, ..
        } => req.basic_auth(username.clone(), Some(password.clone())),
        WriteEndpoint::Server { headers, .. } => {
            let mut r = req;
            for (key, value) in headers {
                r = r.header(key.as_str(), value.as_str());
            }
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::Request, http::StatusCode, routing::any, Router};
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[derive(Default, Debug)]
    struct RequestLog {
        requests: Vec<(String, String, Option<serde_json::Value>)>, // (method, url, body)
    }

    async fn setup_test_server() -> (String, Arc<Mutex<RequestLog>>) {
        let log: Arc<Mutex<RequestLog>> = Arc::default();
        let log_clone = log.clone();

        let app = Router::new().route("/{*path}", any(move |req: Request<Body>| {
            let log = log_clone.clone();
            async move {
                let method = req.method().to_string();
                let uri = req.uri().to_string();

                // Read the body
                let bytes = axum::body::to_bytes(req.into_body(), 1024 * 1024).await.unwrap();
                let body: Option<serde_json::Value> = if bytes.is_empty() {
                    None
                } else {
                    serde_json::from_slice(&bytes).ok()
                };

                let mut log = log.lock().unwrap();
                log.requests.push((method, uri, body));

                (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "resourceType": "OperationOutcome",
                        "issue": [{"severity": "information", "code": "success", "diagnostics": "ok"}]
                    })),
                )
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (format!("http://{}", addr), log)
    }

    async fn setup_test_server_with_status(status: StatusCode) -> (String, Arc<Mutex<RequestLog>>) {
        let log: Arc<Mutex<RequestLog>> = Arc::default();
        let log_clone = log.clone();

        let app = Router::new().route("/{*path}", any(move |req: Request<Body>| {
            let log = log_clone.clone();
            async move {
                let method = req.method().to_string();
                let uri = req.uri().to_string();
                let bytes = axum::body::to_bytes(req.into_body(), 1024 * 1024).await.unwrap();
                let body: Option<serde_json::Value> = if bytes.is_empty() {
                    None
                } else {
                    serde_json::from_slice(&bytes).ok()
                };

                let mut log = log.lock().unwrap();
                log.requests.push((method, uri, body));

                (status, axum::Json(serde_json::json!({
                    "resourceType": "OperationOutcome",
                    "issue": [{"severity": "error", "code": "processing", "diagnostics": "server error"}]
                })))
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (format!("http://{}", addr), log)
    }

    async fn setup_test_server_with_echo() -> (String, Arc<Mutex<RequestLog>>) {
        let log: Arc<Mutex<RequestLog>> = Arc::default();
        let log_clone = log.clone();

        let app = Router::new().route(
            "/{*path}",
            any(move |req: Request<Body>| {
                let log = log_clone.clone();
                async move {
                    let method = req.method().to_string();
                    let uri = req.uri().to_string();
                    let bytes = axum::body::to_bytes(req.into_body(), 1024 * 1024)
                        .await
                        .unwrap();
                    let body: Option<serde_json::Value> = if bytes.is_empty() {
                        None
                    } else {
                        serde_json::from_slice(&bytes).ok()
                    };

                    let mut log = log.lock().unwrap();
                    log.requests.push((method, uri, body.clone()));

                    // Echo back the body with an id so the uploader can extract it
                    let response_body = body
                        .unwrap_or_else(|| serde_json::json!({"resourceType": "OperationOutcome"}));

                    (StatusCode::OK, axum::Json(response_body))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (format!("http://{}", addr), log)
    }

    // ── upload_ndjson_files tests ──

    #[tokio::test]
    async fn upload_ndjson_single_type() {
        let (base_url, log) = setup_test_server_with_echo().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write a test NDJSON file with 3 resources
        let mut file = std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();
        for i in 0..3 {
            writeln!(
                file,
                "{}",
                serde_json::json!({
                    "resourceType": "Patient",
                    "id": format!("patient-{}", i + 1),
                    "name": [{"family": format!("Test{}", i + 1)}]
                })
            )
            .unwrap();
        }

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let ids = upload_ndjson_files(&data_dir, &["Patient".to_string()], &endpoint, 1)
            .await
            .unwrap();

        assert!(ids.contains_key("Patient"));
        assert_eq!(ids["Patient"].len(), 3);

        let log = log.lock().unwrap();
        assert_eq!(log.requests.len(), 3);
        assert_eq!(log.requests[0].0, "PUT");
        assert!(log.requests[0].1.contains("/Patient/patient-1"));
        assert_eq!(log.requests[1].0, "PUT");
        assert!(log.requests[1].1.contains("/Patient/patient-2"));
        assert_eq!(log.requests[2].0, "PUT");
        assert!(log.requests[2].1.contains("/Patient/patient-3"));
    }

    #[tokio::test]
    async fn upload_ndjson_multiple_types() {
        let (base_url, log) = setup_test_server_with_echo().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write Patient.ndjson
        let mut file = std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "resourceType": "Patient", "id": "patient-1", "name": [{"family": "Test"}]
            })
        )
        .unwrap();

        // Write Observation.ndjson
        let mut file = std::fs::File::create(data_dir.join("Observation.ndjson")).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "resourceType": "Observation", "id": "obs-1", "status": "final"
            })
        )
        .unwrap();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let ids = upload_ndjson_files(
            &data_dir,
            &["Patient".to_string(), "Observation".to_string()],
            &endpoint,
            1,
        )
        .await
        .unwrap();

        assert!(ids.contains_key("Patient"));
        assert!(ids.contains_key("Observation"));
        assert_eq!(ids["Patient"].len(), 1);
        assert_eq!(ids["Observation"].len(), 1);

        let log = log.lock().unwrap();
        assert_eq!(log.requests.len(), 2);
        assert!(log.requests[0].1.contains("/Patient/patient-1"));
        assert!(log.requests[1].1.contains("/Observation/obs-1"));
    }

    #[tokio::test]
    async fn upload_ndjson_with_post_method() {
        let (base_url, log) = setup_test_server_with_echo().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write a test NDJSON file
        let mut file = std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "resourceType": "Patient",
                "id": "patient-1",
                "name": [{"family": "Test"}]
            })
        )
        .unwrap();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "POST".to_string(),
            concurrency: 1,
        };

        let ids = upload_ndjson_files(&data_dir, &["Patient".to_string()], &endpoint, 1)
            .await
            .unwrap();

        assert!(ids.contains_key("Patient"));

        let log = log.lock().unwrap();
        assert_eq!(log.requests.len(), 1);
        // POST should be used instead of PUT
        assert_eq!(log.requests[0].0, "POST");
        // POST URL should be /Patient (no id in path)
        assert!(log.requests[0].1.ends_with("/Patient"));
        // id should be removed from body for POST
        if let Some(ref body) = log.requests[0].2 {
            assert!(body.get("id").is_none(), "id should be removed for POST");
        }
    }

    #[tokio::test]
    async fn upload_ndjson_handles_missing_file() {
        let (base_url, _log) = setup_test_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // No NDJSON files written — the file doesn't exist

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        // Should not crash — should return empty map
        let ids = upload_ndjson_files(&data_dir, &["Patient".to_string()], &endpoint, 1)
            .await
            .unwrap();

        // Patient should not be in the map since no file was found
        assert!(
            !ids.contains_key("Patient"),
            "missing file should be skipped"
        );
    }

    #[tokio::test]
    async fn upload_ndjson_handles_server_error() {
        let (base_url, log) =
            setup_test_server_with_status(StatusCode::INTERNAL_SERVER_ERROR).await;
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write a test NDJSON file
        let mut file = std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "resourceType": "Patient",
                "id": "patient-1",
                "name": [{"family": "Test"}]
            })
        )
        .unwrap();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        // Should not crash — should log the error and continue
        let ids = upload_ndjson_files(&data_dir, &["Patient".to_string()], &endpoint, 1)
            .await
            .unwrap();

        // The request was made but the server returned 500, so no IDs should be recorded
        assert!(ids.contains_key("Patient"));
        assert!(
            ids["Patient"].is_empty(),
            "no IDs should be recorded on server error"
        );

        let log = log.lock().unwrap();
        assert_eq!(log.requests.len(), 1);
        assert_eq!(log.requests[0].0, "PUT");
    }

    // ── upload_supplement_resources tests ──

    #[tokio::test]
    async fn test_upload_supplement_resources() {
        let (base_url, log) = setup_test_server_with_echo().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write a Patient.ndjson so bulk_counts has Patient covered
        let mut file = std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "resourceType": "Patient", "id": "patient-1"
            })
        )
        .unwrap();

        let mut bulk_counts = HashMap::new();
        bulk_counts.insert("Patient".to_string(), 1u64);
        // Organization is NOT in bulk_counts — should get a supplement

        let profile_urls = HashMap::new();
        let profiles = Vec::new();
        let value_set_systems = HashMap::new();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let ids = upload_supplement_resources(
            &["Patient".to_string(), "Organization".to_string()],
            &bulk_counts,
            &profile_urls,
            &profiles,
            &value_set_systems,
            &endpoint,
        )
        .await
        .unwrap();

        // Patient should be skipped (in bulk_counts), Organization should be uploaded
        assert!(
            !ids.contains_key("Patient"),
            "Patient is in bulk_counts, should be skipped"
        );
        assert!(
            ids.contains_key("Organization"),
            "Organization should get a supplement"
        );
        assert_eq!(ids["Organization"].len(), 1);
        assert_eq!(ids["Organization"][0], "organization-1");

        let log = log.lock().unwrap();
        // Only Organization should have been uploaded
        assert_eq!(log.requests.len(), 1);
        assert!(log.requests[0].1.contains("/Organization/organization-1"));
    }

    #[tokio::test]
    async fn test_upload_supplement_skips_non_resource_types() {
        let (base_url, log) = setup_test_server_with_echo().await;

        let bulk_counts = HashMap::new(); // Nothing is covered
        let profile_urls = HashMap::new();
        let profiles = Vec::new();
        let value_set_systems = HashMap::new();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let ids = upload_supplement_resources(
            &[
                "Extension".to_string(),
                "Identifier".to_string(),
                "Organization".to_string(),
            ],
            &bulk_counts,
            &profile_urls,
            &profiles,
            &value_set_systems,
            &endpoint,
        )
        .await
        .unwrap();

        // Extension and Identifier should be skipped (NON_RESOURCE_TYPES)
        assert!(!ids.contains_key("Extension"));
        assert!(!ids.contains_key("Identifier"));
        // Organization should be uploaded
        assert!(ids.contains_key("Organization"));

        let log = log.lock().unwrap();
        assert_eq!(log.requests.len(), 1);
        assert!(log.requests[0].1.contains("/Organization/organization-1"));
    }

    #[tokio::test]
    async fn test_upload_supplement_skips_types_in_bulk_counts() {
        let (base_url, log) = setup_test_server_with_echo().await;

        let mut bulk_counts = HashMap::new();
        bulk_counts.insert("Patient".to_string(), 5u64);
        bulk_counts.insert("Organization".to_string(), 3u64);

        let profile_urls = HashMap::new();
        let profiles = Vec::new();
        let value_set_systems = HashMap::new();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let ids = upload_supplement_resources(
            &["Patient".to_string(), "Organization".to_string()],
            &bulk_counts,
            &profile_urls,
            &profiles,
            &value_set_systems,
            &endpoint,
        )
        .await
        .unwrap();

        // Both types are in bulk_counts with count > 0, so nothing should be uploaded
        assert!(
            ids.is_empty(),
            "no supplements should be uploaded when all types are covered"
        );

        let log = log.lock().unwrap();
        assert_eq!(log.requests.len(), 0, "no requests should be made");
    }

    // ── delete_all_resources tests ──

    #[tokio::test]
    async fn test_delete_all_resources() {
        let (base_url, log) = setup_test_server().await;

        let mut ids = HashMap::new();
        ids.insert(
            "Patient".to_string(),
            vec!["patient-1".to_string(), "patient-2".to_string()],
        );
        ids.insert("Organization".to_string(), vec!["org-1".to_string()]);

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        delete_all_resources(
            &ids,
            &["Patient".to_string(), "Organization".to_string()],
            &endpoint,
            1,
        )
        .await
        .unwrap();

        let log = log.lock().unwrap();
        // Should delete in reverse creation order: Organization first, then Patient
        assert_eq!(log.requests.len(), 3);
        assert_eq!(log.requests[0].0, "DELETE");
        assert!(log.requests[0].1.contains("/Organization/org-1"));
        assert_eq!(log.requests[1].0, "DELETE");
        assert!(log.requests[1].1.contains("/Patient/patient-1"));
        assert_eq!(log.requests[2].0, "DELETE");
        assert!(log.requests[2].1.contains("/Patient/patient-2"));
    }

    #[tokio::test]
    async fn test_delete_all_resources_handles_errors() {
        let (base_url, log) = setup_test_server_with_status(StatusCode::NOT_FOUND).await;

        let mut ids = HashMap::new();
        ids.insert("Patient".to_string(), vec!["patient-1".to_string()]);

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        // Should not crash — 404 is logged and processing continues
        let result = delete_all_resources(&ids, &["Patient".to_string()], &endpoint, 1).await;
        assert!(result.is_ok(), "delete should not fail on 404");

        let log = log.lock().unwrap();
        assert_eq!(log.requests.len(), 1);
        assert_eq!(log.requests[0].0, "DELETE");
    }

    // ── ensure_r5_extension_profiles tests ──

    #[tokio::test]
    async fn test_ensure_r5_extension_profiles() {
        let (base_url, log) = setup_test_server().await;

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        ensure_r5_extension_profiles(&endpoint).await.unwrap();

        let log = log.lock().unwrap();
        // Should upload all 3 R5 extension profiles
        assert_eq!(log.requests.len(), 3);
        for req in log.requests.iter() {
            assert_eq!(req.0, "PUT");
            assert!(req.1.contains("/StructureDefinition/"));
        }
        // Verify specific profile URLs
        assert!(log.requests[0].1.contains("individual-recordedSexOrGender"));
        assert!(log.requests[1].1.contains("individual-genderIdentity"));
        assert!(log.requests[2].1.contains("individual-pronouns"));
    }

    // ── add_write_auth tests ──

    #[tokio::test]
    async fn add_write_auth_basic() {
        let endpoint = WriteEndpoint::Repository {
            base_url: "http://repo.test/fhir".to_string(),
            username: "admin".to_string(),
            password: "s3cret".to_string(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let client = reqwest::Client::new();
        let req = client.put("http://repo.test/fhir/Patient/1");
        let req = add_write_auth(req, &endpoint);

        // We can't easily inspect the auth header on a RequestBuilder,
        // but we can verify it compiles and the function doesn't panic.
        // The actual auth is verified by sending a request to a server
        // that checks the Authorization header.
        let _ = req;
    }

    #[tokio::test]
    async fn add_write_auth_headers() {
        let mut headers = HashMap::new();
        headers.insert("X-API-Key".to_string(), "test-key-123".to_string());
        headers.insert("Authorization".to_string(), "Bearer test-token".to_string());

        let endpoint = WriteEndpoint::Server {
            base_url: "http://server.test/fhir".to_string(),
            headers,
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let client = reqwest::Client::new();
        let req = client.put("http://server.test/fhir/Patient/1");
        let req = add_write_auth(req, &endpoint);

        // Verify it compiles and doesn't panic
        let _ = req;
    }

    #[allow(clippy::type_complexity)]
    #[tokio::test]
    async fn add_write_auth_basic_sends_correct_header() {
        let log: Arc<Mutex<Vec<(String, Vec<(String, String)>)>>> = Arc::default();
        let log_clone = log.clone();

        let app = Router::new().route(
            "/{*path}",
            any(move |req: Request<Body>| {
                let log = log_clone.clone();
                async move {
                    let method = req.method().to_string();
                    let headers: Vec<(String, String)> = req
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect();
                    let mut log = log.lock().unwrap();
                    log.push((method, headers));
                    (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({"resourceType": "OperationOutcome"})),
                    )
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base_url = format!("http://{}", addr);

        let endpoint = WriteEndpoint::Repository {
            base_url: base_url.clone(),
            username: "admin".to_string(),
            password: "s3cret".to_string(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let client = reqwest::Client::new();
        let req = client.put(format!("{}/Patient/1", base_url));
        let req = add_write_auth(req, &endpoint);
        req.send().await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        let auth_header = log[0].1.iter().find(|(k, _)| k == "authorization");
        assert!(
            auth_header.is_some(),
            "Authorization header should be present"
        );
        if let Some((_, val)) = auth_header {
            assert!(val.starts_with("Basic "), "Auth should be Basic");
        }
    }

    #[allow(clippy::type_complexity)]
    #[tokio::test]
    async fn add_write_auth_headers_sends_correct_headers() {
        let log: Arc<Mutex<Vec<(String, Vec<(String, String)>)>>> = Arc::default();
        let log_clone = log.clone();

        let app = Router::new().route(
            "/{*path}",
            any(move |req: Request<Body>| {
                let log = log_clone.clone();
                async move {
                    let method = req.method().to_string();
                    let headers: Vec<(String, String)> = req
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect();
                    let mut log = log.lock().unwrap();
                    log.push((method, headers));
                    (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({"resourceType": "OperationOutcome"})),
                    )
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base_url = format!("http://{}", addr);

        let mut headers = HashMap::new();
        headers.insert("X-API-Key".to_string(), "test-key-123".to_string());

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers,
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        let client = reqwest::Client::new();
        let req = client.put(format!("{}/Patient/1", base_url));
        let req = add_write_auth(req, &endpoint);
        req.send().await.unwrap();

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        let api_key_header = log[0].1.iter().find(|(k, _)| k == "x-api-key");
        assert!(
            api_key_header.is_some(),
            "X-API-Key header should be present"
        );
        if let Some((_, val)) = api_key_header {
            assert_eq!(val, "test-key-123");
        }
    }

    // ── End-to-end tests using mock_server ──

    #[tokio::test]
    async fn e2e_upload_verify_and_delete() {
        let addr = crate::mock_server::start_mock_server(0).await.unwrap();
        let base_url = format!("http://{}/fhir", addr);

        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write Patient.ndjson with 2 resources
        let mut file = std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();
        for i in 0..2 {
            writeln!(
                file,
                "{}",
                serde_json::json!({
                    "resourceType": "Patient",
                    "id": format!("e2e-patient-{}", i + 1),
                    "name": [{"family": format!("E2E{}", i + 1)}]
                })
            )
            .unwrap();
        }

        // Write Observation.ndjson with 1 resource
        let mut file = std::fs::File::create(data_dir.join("Observation.ndjson")).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "resourceType": "Observation",
                "id": "e2e-obs-1",
                "status": "final",
                "code": {"coding": [{"code": "test"}]}
            })
        )
        .unwrap();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        // Step 1: Upload
        let ids = upload_ndjson_files(
            &data_dir,
            &["Patient".to_string(), "Observation".to_string()],
            &endpoint,
            1,
        )
        .await
        .unwrap();

        assert!(ids.contains_key("Patient"));
        assert!(ids.contains_key("Observation"));
        assert_eq!(ids["Patient"].len(), 2);
        assert_eq!(ids["Observation"].len(), 1);

        // Step 2: Verify resources are accessible via GET
        let client = reqwest::Client::new();
        for id in &ids["Patient"] {
            let resp = client
                .get(format!("{}/Patient/{}", base_url, id))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 200, "Patient {} should exist", id);
        }
        for id in &ids["Observation"] {
            let resp = client
                .get(format!("{}/Observation/{}", base_url, id))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 200, "Observation {} should exist", id);
        }

        // Step 3: Delete all resources
        delete_all_resources(
            &ids,
            &["Patient".to_string(), "Observation".to_string()],
            &endpoint,
            1,
        )
        .await
        .unwrap();

        // Step 4: Verify resources are gone
        for id in &ids["Patient"] {
            let resp = client
                .get(format!("{}/Patient/{}", base_url, id))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 404, "Patient {} should be deleted", id);
        }
        for id in &ids["Observation"] {
            let resp = client
                .get(format!("{}/Observation/{}", base_url, id))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 404, "Observation {} should be deleted", id);
        }
    }

    #[tokio::test]
    async fn e2e_upload_with_concurrency() {
        let addr = crate::mock_server::start_mock_server(0).await.unwrap();
        let base_url = format!("http://{}/fhir", addr);

        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write 10 Patient resources
        let mut file = std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();
        for i in 0..10 {
            writeln!(
                file,
                "{}",
                serde_json::json!({
                    "resourceType": "Patient",
                    "id": format!("concurrent-patient-{}", i + 1),
                    "name": [{"family": format!("Concurrent{}", i + 1)}]
                })
            )
            .unwrap();
        }

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 4,
        };

        // Upload with concurrency = 4
        let ids = upload_ndjson_files(&data_dir, &["Patient".to_string()], &endpoint, 4)
            .await
            .unwrap();

        assert!(ids.contains_key("Patient"));
        assert_eq!(
            ids["Patient"].len(),
            10,
            "all 10 resources should be uploaded"
        );

        // Verify all resources exist
        let client = reqwest::Client::new();
        for id in &ids["Patient"] {
            let resp = client
                .get(format!("{}/Patient/{}", base_url, id))
                .send()
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                200,
                "Patient {} should exist after concurrent upload",
                id
            );
        }
    }

    #[tokio::test]
    async fn upload_ndjson_empty_file() {
        let (base_url, _log) = setup_test_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write an empty NDJSON file (no content)
        std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        // Should not crash — should skip the empty file
        let ids = upload_ndjson_files(&data_dir, &["Patient".to_string()], &endpoint, 1)
            .await
            .unwrap();

        assert!(
            !ids.contains_key("Patient"),
            "empty NDJSON file should be skipped"
        );
    }

    #[tokio::test]
    async fn upload_ndjson_empty_lines_only() {
        let (base_url, _log) = setup_test_server().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Write an NDJSON file with only blank lines
        let mut file = std::fs::File::create(data_dir.join("Patient.ndjson")).unwrap();
        writeln!(file).unwrap();
        writeln!(file, "   ").unwrap();
        writeln!(file).unwrap();

        let endpoint = WriteEndpoint::Server {
            base_url: base_url.clone(),
            headers: HashMap::new(),
            upload_method: "PUT".to_string(),
            concurrency: 1,
        };

        // Should not crash — should skip the empty file
        let ids = upload_ndjson_files(&data_dir, &["Patient".to_string()], &endpoint, 1)
            .await
            .unwrap();

        assert!(
            !ids.contains_key("Patient"),
            "NDJSON with only blank lines should be skipped"
        );
    }
}
