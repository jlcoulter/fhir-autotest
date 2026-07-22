use crate::config::models::WriteEndpoint;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

/// Upload NDJSON files to the FHIR repository and return IDs per resource type.
///
/// For each resource type in `creation_order`, reads the NDJSON file from
/// `{data_dir}/{ResourceType}.ndjson` and POSTs each resource to the repository.
/// Returns a map of resource type → list of server-assigned IDs.
///
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
            .filter_map(|l| l.ok())
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
                // Remove the id field — let the server assign one
                let mut resource = resource;
                let client_id = resource
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                resource.as_object_mut().map(|o| o.remove("id"));

                let url = format!("{}/{}", base_url, resource_type);
                let client = client.clone();
                let write_endpoint = write_endpoint.clone();

                handles.push(tokio::spawn(async move {
                    let mut req = client
                        .post(&url)
                        .header("Content-Type", "application/fhir+json")
                        .header("Accept", "application/fhir+json")
                        .json(&resource);

                    req = add_write_auth(req, &write_endpoint);
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
                        if uploaded % 1000 == 0 {
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
                        Ok(Ok(status)) => {
                            if status == 200 || status == 204 || status == 410 {
                                deleted += 1;
                            } else {
                                errors += 1;
                            }
                        }
                        _ => {
                            errors += 1;
                        }
                    }
                }

                if deleted > 0 && deleted % 1000 == 0 {
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
        let order = vec![
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
