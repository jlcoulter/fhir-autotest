use crate::config::models::{UploadMethod, WriteEndpoint};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;
use std::sync::Arc;

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
    (
        "targetPath",
        "http://hl7.org/fhir/StructureDefinition/targetPath",
        r#"{
  "resourceType": "StructureDefinition",
  "id": "targetPath",
  "url": "http://hl7.org/fhir/StructureDefinition/targetPath",
  "version": "5.3.0",
  "name": "TargetPath",
  "title": "Target Path",
  "status": "active",
  "kind": "complex-type",
  "abstract": false,
  "context": [{"type": "element", "expression": "Provenance.target"}],
  "type": "Extension",
  "baseDefinition": "http://hl7.org/fhir/StructureDefinition/Extension",
  "derivation": "constraint",
  "snapshot": {
    "element": [
      {"id": "Extension", "path": "Extension", "min": 0, "max": "*",
       "type": [{"code": "Extension"}]},
      {"id": "Extension.extension", "path": "Extension.extension", "min": 0, "max": "0",
       "type": [{"code": "Extension"}]},
      {"id": "Extension.url", "path": "Extension.url", "min": 1, "max": "1",
       "fixedUri": "http://hl7.org/fhir/StructureDefinition/targetPath"},
      {"id": "Extension.value[x]", "path": "Extension.value[x]", "min": 1, "max": "1",
       "type": [{"code": "string"}]}
    ]
  }
}"#,
    ),
];

/// A shared HTTP client for interacting with a FHIR repository.
///
/// Encapsulates the `reqwest::Client`, base URL, upload method, and auth logic
/// so that callers don't need to duplicate client creation, URL building, or
/// auth header injection.
pub struct FhirRepositoryClient {
    client: reqwest::Client,
    base_url: String,
    upload_method: UploadMethod,
    write_endpoint: WriteEndpoint,
}

impl FhirRepositoryClient {
    /// Create a new client from a `WriteEndpoint`.
    pub fn new(write_endpoint: &WriteEndpoint) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let base_url = match write_endpoint {
            WriteEndpoint::Repository { base_url, .. } => base_url.clone(),
            WriteEndpoint::Server { base_url, .. } => base_url.clone(),
        };
        let upload_method = match write_endpoint {
            WriteEndpoint::Repository { upload_method, .. }
            | WriteEndpoint::Server { upload_method, .. } => *upload_method,
        };
        Ok(Self {
            client,
            base_url,
            upload_method,
            write_endpoint: write_endpoint.clone(),
        })
    }

    /// The base URL of the FHIR repository.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The upload method (PUT or POST).
    pub fn upload_method(&self) -> UploadMethod {
        self.upload_method
    }

    /// PUT a resource to `/{resource_type}/{id}`.
    pub async fn put_resource(
        &self,
        resource_type: &str,
        id: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/{}/{}", self.base_url, resource_type, id);
        let req = self
            .client
            .put(&url)
            .header("Content-Type", "application/fhir+json")
            .header("Accept", "application/fhir+json")
            .json(body);
        let req = add_write_auth(req, &self.write_endpoint);
        Ok(req.send().await?)
    }

    /// POST a resource to `/{resource_type}`.
    pub async fn post_resource(
        &self,
        resource_type: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/{}", self.base_url, resource_type);
        let req = self
            .client
            .post(&url)
            .header("Content-Type", "application/fhir+json")
            .header("Accept", "application/fhir+json")
            .json(body);
        let req = add_write_auth(req, &self.write_endpoint);
        Ok(req.send().await?)
    }

    /// DELETE a resource at `/{resource_type}/{id}`.
    pub async fn delete_resource(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/{}/{}", self.base_url, resource_type, id);
        let req = self
            .client
            .delete(&url)
            .header("Accept", "application/fhir+json");
        let req = add_write_auth(req, &self.write_endpoint);
        Ok(req.send().await?)
    }
}

/// Ensure the required HL7 R5 extension StructureDefinitions are present in the
/// FHIR repository. If a profile is missing (404), uploads the embedded minimal
/// StructureDefinition so the HAPI validator can resolve profile URIs used in
/// extension slicing discriminators.
pub async fn ensure_r5_extension_profiles(write_endpoint: &WriteEndpoint) -> Result<()> {
    let client = FhirRepositoryClient::new(write_endpoint)?;

    for (id, canonical_url, embedded_json) in R5_EXTENSION_PROFILES {
        // Parse the embedded minimal StructureDefinition
        let sd_json: serde_json::Value = serde_json::from_str(embedded_json)
            .with_context(|| format!("Failed to parse embedded StructureDefinition for {}", id))?;

        // Upload to repository
        match client
            .put_resource("StructureDefinition", id, &sd_json)
            .await
        {
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
    let client = FhirRepositoryClient::new(write_endpoint)?;

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

        if !any_uploaded {
            println!("\n── Uploading supplement resources (uncovered types) ──");
            any_uploaded = true;
        }

        match client.put_resource(resource_type, &id, &resource).await {
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

/// Order resources of a single type into dependency "waves" for upload.
///
/// Some resource types reference *other resources of the same type* — most
/// notably `Organization.partOf → Organization/{id}`. Because bulk generation
/// links resources into an arbitrary web, a resource may reference a sibling
/// that appears later in the file. Uploading concurrently in file order would
/// then produce forward references the server rejects (HAPI-1094).
///
/// This function parses each line, extracts its id and the ids of same-type
/// resources it references, then partitions the resources into waves such that
/// every wave depends only on resources in earlier waves. Callers upload each
/// wave to completion before starting the next, guaranteeing referenced
/// siblings already exist.
///
/// Resources whose dependencies cannot be satisfied (e.g. a reference cycle, or
/// a reference to an id not present in the file) are placed in a final
/// best-effort wave so they are still attempted. Ordering within a wave is
/// stable (original file order).
fn order_upload_waves<'a>(resource_type: &str, lines: &'a [String]) -> Vec<Vec<&'a String>> {
    // Parse id + same-type dependencies for each line. Lines that fail to parse
    // are kept (with no dependencies) so they are still uploaded.
    let prefix = format!("{}/", resource_type);
    let mut id_of: Vec<Option<String>> = Vec::with_capacity(lines.len());
    let mut deps_of: Vec<Vec<String>> = Vec::with_capacity(lines.len());

    for line in lines {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => {
                let id = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let mut deps = Vec::new();
                collect_same_type_refs(&value, &prefix, &mut deps);
                id_of.push(id);
                deps_of.push(deps);
            }
            Err(_) => {
                id_of.push(None);
                deps_of.push(Vec::new());
            }
        }
    }

    // Set of ids actually present in this file — references to anything outside
    // it cannot be ordered here and are ignored for wave assignment.
    let present: std::collections::HashSet<&str> =
        id_of.iter().filter_map(|o| o.as_deref()).collect();

    let mut committed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut placed = vec![false; lines.len()];
    let mut waves: Vec<Vec<&String>> = Vec::new();

    // Greedily build waves: each pass takes every not-yet-placed resource whose
    // in-file dependencies are all committed.
    loop {
        let mut wave_indices: Vec<usize> = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            let _ = line;
            if placed[idx] {
                continue;
            }
            let ready = deps_of[idx].iter().all(|dep| {
                // Ignore self-references and references to ids not in this file.
                if !present.contains(dep.as_str()) {
                    return true;
                }
                if Some(dep.as_str()) == id_of[idx].as_deref() {
                    return true;
                }
                committed.contains(dep)
            });
            if ready {
                wave_indices.push(idx);
            }
        }

        if wave_indices.is_empty() {
            break;
        }

        let mut wave: Vec<&String> = Vec::with_capacity(wave_indices.len());
        for idx in wave_indices {
            placed[idx] = true;
            if let Some(id) = &id_of[idx] {
                committed.insert(id.clone());
            }
            wave.push(&lines[idx]);
        }
        waves.push(wave);
    }

    // Any remaining resources are caught in a cycle — emit them in one final
    // best-effort wave so they are still attempted.
    let leftover: Vec<&String> = lines
        .iter()
        .enumerate()
        .filter(|(idx, _)| !placed[*idx])
        .map(|(_, line)| line)
        .collect();
    if !leftover.is_empty() {
        waves.push(leftover);
    }

    waves
}

/// Recursively collect the ids of same-type references within a resource.
///
/// Scans every `"reference": "{prefix}{id}"` string in the JSON (where `prefix`
/// is e.g. `"Organization/"`) and records the referenced `id`.
fn collect_same_type_refs(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if key == "reference" {
                    if let Some(s) = val.as_str()
                        && let Some(id) = s.strip_prefix(prefix)
                        && !id.is_empty()
                        && !id.contains('/')
                    {
                        out.push(id.to_string());
                    }
                } else {
                    collect_same_type_refs(val, prefix, out);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_same_type_refs(item, prefix, out);
            }
        }
        _ => {}
    }
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
    let repo_client = Arc::new(FhirRepositoryClient::new(write_endpoint)?);
    let upload_method = repo_client.upload_method();

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
            repo_client.base_url()
        );

        let mut ids: Vec<String> = Vec::with_capacity(total);
        let mut uploaded = 0usize;
        let mut errors = 0usize;

        // Order resources into dependency "waves" so that any resource which
        // references another resource of the *same* type (e.g.
        // Organization.partOf → another Organization) is uploaded only after
        // the resource it depends on has been committed. Because generation
        // only ever points partOf at an earlier-indexed Organization and does
        // so for a small fraction of resources, wave 0 contains the vast
        // majority (all independent resources) and later waves are tiny.
        //
        // Within a wave there are no interdependencies, so we upload it through
        // a continuous, semaphore-bounded pool: `concurrency` requests stay in
        // flight at all times instead of stalling on the slowest request of a
        // fixed batch. We only wait between waves, which is cheap since later
        // waves are small. Any resources caught in a reference cycle are
        // emitted by order_upload_waves in a final best-effort wave.
        let permits = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let waves = order_upload_waves(resource_type, &lines);
        for wave in &waves {
            let mut join_set = tokio::task::JoinSet::new();

            for line in wave {
                let resource: serde_json::Value = serde_json::from_str(line)
                    .with_context(|| format!("Invalid JSON in {}.ndjson", resource_type))?;
                let mut resource = resource;
                let client_id = resource
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                // POST: remove client id — let the server assign one
                if upload_method != UploadMethod::Put {
                    resource.as_object_mut().map(|o| o.remove("id"));
                }

                let repo_client = repo_client.clone();
                let resource_type = resource_type.clone();
                // Acquire a permit before spawning so at most `concurrency`
                // requests are ever in flight; the permit is released when the
                // task finishes, immediately admitting the next one.
                let permit = permits.clone().acquire_owned().await?;

                join_set.spawn(async move {
                    let _permit = permit;
                    let resp = if upload_method == UploadMethod::Put {
                        repo_client
                            .put_resource(&resource_type, &client_id, &resource)
                            .await?
                    } else {
                        repo_client.post_resource(&resource_type, &resource).await?
                    };
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    anyhow::Ok((client_id, status.as_u16(), body))
                });
            }

            while let Some(joined) = join_set.join_next().await {
                match joined {
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
/// Uses concurrency for throughput. Returns an error if any deletes failed.
pub async fn delete_all_resources(
    ids: &HashMap<String, Vec<String>>,
    creation_order: &[String],
    write_endpoint: &WriteEndpoint,
    concurrency: usize,
) -> Result<()> {
    let repo_client = Arc::new(FhirRepositoryClient::new(write_endpoint)?);

    let mut total_errors = 0usize;

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
                let mut handles: Vec<(
                    tokio::task::JoinHandle<Result<u16, anyhow::Error>>,
                    String,
                )> = Vec::new();

                for id in chunk {
                    let repo_client = repo_client.clone();
                    let resource_type = resource_type.clone();
                    let id_clone = id.clone();
                    let id = id.clone();

                    handles.push((
                        tokio::spawn(async move {
                            let resp = repo_client
                                .delete_resource(&resource_type, &id_clone)
                                .await?;
                            Ok::<u16, anyhow::Error>(resp.status().as_u16())
                        }),
                        id,
                    ));
                }

                for (handle, id_for_log) in handles {
                    match handle.await {
                        Ok(Ok(200 | 204 | 404 | 410)) => {
                            deleted += 1;
                        }
                        Ok(Ok(status)) => {
                            errors += 1;
                            tracing::warn!(
                                "Unexpected status {} when deleting {}/{}",
                                status,
                                resource_type,
                                id_for_log,
                            );
                        }
                        Ok(Err(e)) => {
                            errors += 1;
                            tracing::warn!(
                                "Request error when deleting {}/{}: {:#}",
                                resource_type,
                                id_for_log,
                                e,
                            );
                        }
                        Err(e) => {
                            errors += 1;
                            tracing::warn!(
                                "Task join error when deleting {}/{}: {}",
                                resource_type,
                                id_for_log,
                                e,
                            );
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
            total_errors += errors;
        }
    }

    if total_errors > 0 {
        anyhow::bail!(
            "{} resource(s) failed to delete during cleanup",
            total_errors
        );
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
    use axum::{Router, body::Body, extract::Request, http::StatusCode, routing::any};
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    // ── order_upload_waves tests ──

    fn org_line(id: &str, part_of: Option<&str>) -> String {
        let mut obj = serde_json::json!({ "resourceType": "Organization", "id": id });
        if let Some(parent) = part_of {
            obj["partOf"] = serde_json::json!({ "reference": format!("Organization/{parent}") });
        }
        obj.to_string()
    }

    fn wave_ids(waves: &[Vec<&String>]) -> Vec<Vec<String>> {
        waves
            .iter()
            .map(|wave| {
                wave.iter()
                    .map(|line| {
                        serde_json::from_str::<serde_json::Value>(line).unwrap()["id"]
                            .as_str()
                            .unwrap()
                            .to_string()
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn waves_place_parents_before_children() {
        // c → b → a, listed in reverse (child-first) order.
        let lines = vec![
            org_line("c", Some("b")),
            org_line("b", Some("a")),
            org_line("a", None),
        ];
        let waves = order_upload_waves("Organization", &lines);
        let ids = wave_ids(&waves);
        assert_eq!(ids, vec![vec!["a"], vec!["b"], vec!["c"]]);
    }

    #[test]
    fn waves_group_independent_resources_together() {
        // a and b are roots; c and d each point at a root.
        let lines = vec![
            org_line("a", None),
            org_line("b", None),
            org_line("c", Some("a")),
            org_line("d", Some("b")),
        ];
        let waves = order_upload_waves("Organization", &lines);
        let ids = wave_ids(&waves);
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], vec!["a", "b"]);
        assert_eq!(ids[1], vec!["c", "d"]);
    }

    #[test]
    fn waves_ignore_self_reference() {
        let lines = vec![org_line("a", Some("a"))];
        let waves = order_upload_waves("Organization", &lines);
        assert_eq!(wave_ids(&waves), vec![vec!["a"]]);
    }

    #[test]
    fn waves_ignore_reference_outside_file() {
        // Parent "missing" is not in this file — treat as already present.
        let lines = vec![org_line("a", Some("missing"))];
        let waves = order_upload_waves("Organization", &lines);
        assert_eq!(wave_ids(&waves), vec![vec!["a"]]);
    }

    #[test]
    fn waves_emit_cycles_in_final_best_effort_wave() {
        // a ↔ b form a cycle; neither can be ordered, so both land in the
        // final best-effort wave. All resources must still be present.
        let lines = vec![org_line("a", Some("b")), org_line("b", Some("a"))];
        let waves = order_upload_waves("Organization", &lines);
        let ids = wave_ids(&waves);
        let flat: Vec<&String> = ids.iter().flatten().collect();
        assert_eq!(flat.len(), 2);
        assert!(flat.iter().any(|id| *id == "a"));
        assert!(flat.iter().any(|id| *id == "b"));
    }

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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Post,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
            concurrency: 1,
        };

        ensure_r5_extension_profiles(&endpoint).await.unwrap();

        let log = log.lock().unwrap();
        // Should upload all R5 extension profiles
        assert_eq!(log.requests.len(), 4);
        for req in log.requests.iter() {
            assert_eq!(req.0, "PUT");
            assert!(req.1.contains("/StructureDefinition/"));
        }
        // Verify specific profile URLs
        assert!(log.requests[0].1.contains("individual-recordedSexOrGender"));
        assert!(log.requests[1].1.contains("individual-genderIdentity"));
        assert!(log.requests[2].1.contains("individual-pronouns"));
        assert!(log.requests[3].1.contains("targetPath"));
    }

    // ── add_write_auth tests ──

    #[tokio::test]
    async fn add_write_auth_basic() {
        let endpoint = WriteEndpoint::Repository {
            base_url: "http://repo.test/fhir".to_string(),
            username: "admin".to_string(),
            password: "s3cret".to_string(),
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
            upload_method: UploadMethod::Put,
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
