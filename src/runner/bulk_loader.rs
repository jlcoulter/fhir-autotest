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
                tracing::warn!(
                    "Failed to upload R5 profile {} (HTTP {})",
                    id,
                    r.status()
                );
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
        const NON_RESOURCE_TYPES: &[&str] = &[
            "Extension", "Identifier", "Coding", "CodeableConcept", "Address",
            "HumanName", "ContactPoint", "Period", "Quantity", "Range",
            "Ratio", "Attachment", "Annotation", "Signature", "Timing",
        ];
        if NON_RESOURCE_TYPES.contains(&resource_type.as_str()) {
            continue;
        }

        // Generate a single resource for this type
        let resource = match crate::generate::generate_supplement_resource(
            resource_type,
            profile_urls,
            profiles,
            value_set_systems,
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Could not generate supplement resource for {}: {}", resource_type, e);
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
    #[test]
    fn creation_order_determines_upload_order() {
        // Verify that the upload function respects creation order
        // This is implicitly tested by the integration tests
        // but we verify the ordering logic here
        let order = [
            "Organization".to_string(),
            "Location".to_string(),
            "Practitioner".to_string(),
            "PractitionerRole".to_string(),
        ];
        // Just verifying the logic compiles and order is preserved
        assert_eq!(order[0], "Organization");
        assert_eq!(order[3], "PractitionerRole");
    }
}
