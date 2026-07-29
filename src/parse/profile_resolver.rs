use crate::model::*;
use crate::parse::parse_package;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

/// Default cache directory for downloaded FHIR packages.
fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let mut dir = PathBuf::from(home);
    dir.push(".cache");
    dir.push("fhir-ig-testgen");
    dir.push("packages");
    dir
}

/// Cache of downloaded FHIR packages, keyed by package ID.
struct PackageCache {
    packages: HashMap<String, Vec<StructureDefinition>>,
    cache_dir: PathBuf,
}

impl PackageCache {
    fn new() -> Self {
        let cache_dir = cache_dir();
        std::fs::create_dir_all(&cache_dir).ok();
        Self {
            packages: HashMap::new(),
            cache_dir,
        }
    }

    /// Get a profile by URL from the cache, downloading the package if needed.
    fn get_profile(&mut self, url: &str) -> Option<&StructureDefinition> {
        for profiles in self.packages.values() {
            if let Some(sd) = profiles.iter().find(|p| p.url == url) {
                return Some(sd);
            }
        }
        None
    }

    /// Ensure a package is loaded, downloading it if not cached.
    async fn ensure_package(&mut self, package_id: &str, version: &str) -> Result<()> {
        let cache_key = format!("{}@{}\n", package_id, version);
        if self.packages.contains_key(&cache_key) {
            return Ok(());
        }

        let tgz_filename = format!("{}-{}.tgz", package_id, version);
        let tgz_path = self.cache_dir.join(&tgz_filename);

        // Check local cache first
        if tgz_path.exists() {
            tracing::info!("Using cached FHIR package: {}@{}", package_id, version);
            let pkg = parse_package(tgz_path.to_str().unwrap())?;
            tracing::info!(
                "Loaded package {}: {} StructureDefinitions",
                package_id,
                pkg.structure_definitions.len()
            );
            self.packages.insert(cache_key, pkg.structure_definitions);
            return Ok(());
        }

        let tgz_url = format!(
            "https://packages.fhir.org/{}/-/{}-{}.tgz",
            package_id, package_id, version
        );

        tracing::info!("Downloading FHIR package: {}@{}", package_id, version);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent("fhir-ig-testgen/0.1")
            .build()?;

        let response = client.get(&tgz_url).send().await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to download package {} (status: {})",
                package_id,
                response.status()
            );
        }

        let bytes = response.bytes().await?;
        std::fs::write(&tgz_path, &bytes)?;

        let pkg = parse_package(tgz_path.to_str().unwrap())?;
        tracing::info!(
            "Loaded package {}: {} StructureDefinitions",
            package_id,
            pkg.structure_definitions.len()
        );

        self.packages.insert(cache_key, pkg.structure_definitions);
        Ok(())
    }
}

/// Resolve the full parent chain for all StructureDefinitions in a package.
///
/// For each profile with a `baseDefinition`, check if the parent is already
/// loaded; if not, download it from the FHIR package registry or HL7 base.
/// Merges parent snapshot elements into the child's snapshot so that slice
/// definitions with their pattern values are available during resource generation.
pub async fn resolve_parent_chain(profiles: &mut Vec<StructureDefinition>) -> Result<()> {
    // Build URL → index map for quick lookup
    let mut url_map: HashMap<String, usize> = HashMap::new();
    for (i, p) in profiles.iter().enumerate() {
        url_map.insert(p.url.clone(), i);
    }

    let mut package_cache = PackageCache::new();

    // Resolve each profile's parent chain
    let mut i = 0;
    while i < profiles.len() {
        let base_url = match profiles[i].base_definition.clone() {
            Some(ref url) if !url.is_empty() => url.clone(),
            _ => {
                i += 1;
                continue;
            }
        };

        // Strip FHIR version suffix (e.g. "|4.0.1") for URL matching
        let base_url_clean = base_url.split('|').next().unwrap_or(&base_url).to_string();

        // Check if parent is already in our list
        if !url_map.contains_key(&base_url_clean) {
            // Check the package cache first
            let parent = if let Some(cached) = package_cache.get_profile(&base_url_clean) {
                cached.clone()
            } else {
                // Try downloading the individual profile
                match download_profile(&base_url).await {
                    Ok(p) => p,
                    Err(_) => {
                        // Individual download failed — try downloading the parent's FHIR package
                        match resolve_via_package(&base_url, &mut package_cache).await {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::warn!(
                                    "Skipping parent resolution for {}: {}",
                                    profiles[i].name,
                                    e
                                );
                                i += 1;
                                continue;
                            }
                        }
                    }
                }
            };

            // Add to our list
            url_map.insert(parent.url.clone(), profiles.len());
            profiles.push(parent);
        }

        // Merge parent snapshot into child
        let parent_idx = url_map[&base_url_clean];
        let parent_elements = match &profiles[parent_idx].snapshot {
            Some(s) => s.element.clone(),
            None => {
                i += 1;
                continue;
            }
        };

        merge_snapshot_elements(&mut profiles[i], &parent_elements);

        i += 1;
    }

    // Second pass: resolve profiled types referenced by slice elements.
    // HCPD profiles define slices that reference profiled Identifier types
    // (e.g. hcpd-hpio, hcpd-local-identifier, au-hpii, au-australianbusinessnumber).
    // These are referenced via the `type[].profile` field on slice elements.
    // They may be in the IG package itself (digitalhealth.gov.au) or in
    // downloaded packages (hl7.org.au).
    let mut failed_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut resolved_any = true;
    while resolved_any {
        resolved_any = false;
        // Collect all profiled type URLs referenced by any profile's elements
        let referenced_urls: Vec<String> = profiles
            .iter()
            .flat_map(|p| {
                p.snapshot
                    .as_ref()
                    .map(|s| &s.element)
                    .into_iter()
                    .flatten()
            })
            .flat_map(|e| {
                e.type_.iter().flat_map(|t| {
                    // Check both `profile` and `targetProfile` fields
                    t.profile
                        .iter()
                        .chain(t.target_profile.iter())
                        .map(|s| s.split('|').next().unwrap_or(s).to_string())
                })
            })
            .filter(|url| !url_map.contains_key(url) && !failed_urls.contains(url))
            .collect();

        for url in &referenced_urls {
            // Skip base FHIR types — they're always available from the base spec
            // and don't need to be resolved as profiles.
            if url.starts_with("http://hl7.org/fhir/StructureDefinition/") {
                failed_urls.insert(url.clone());
                continue;
            }

            // Check if it's already in the profiles list (e.g. HCPD profiled types)
            if url_map.contains_key(url) {
                continue;
            }

            // Check the package cache first
            if let Some(cached) = package_cache.get_profile(url) {
                url_map.insert(cached.url.clone(), profiles.len());
                profiles.push(cached.clone());
                resolved_any = true;
                continue;
            }

            // Try downloading the individual profile
            match download_profile(url).await {
                Ok(p) => {
                    url_map.insert(p.url.clone(), profiles.len());
                    profiles.push(p);
                    resolved_any = true;
                }
                Err(_) => {
                    // Try downloading the parent's FHIR package
                    match resolve_via_package(url, &mut package_cache).await {
                        Ok(p) => {
                            url_map.insert(p.url.clone(), profiles.len());
                            profiles.push(p);
                            resolved_any = true;
                        }
                        Err(e) => {
                            tracing::debug!("Could not resolve profiled type {}: {}", url, e);
                            failed_urls.insert(url.clone());
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Try to resolve a profile URL by downloading its containing FHIR package.
async fn resolve_via_package(url: &str, cache: &mut PackageCache) -> Result<StructureDefinition> {
    // Extract version suffix (e.g. "|2.0.0") and clean URL
    let version = url.split('|').nth(1).unwrap_or("1.0.0");
    let clean_url = url.split('|').next().unwrap_or(url);

    let package_id =
        url_to_package_id(clean_url).context("Cannot determine FHIR package from URL")?;

    cache.ensure_package(&package_id, version).await?;

    cache.get_profile(clean_url).cloned().context(format!(
        "Profile {} not found in package {}@{}",
        clean_url, package_id, version
    ))
}

/// Map a profile URL to its FHIR package ID.
///
/// Examples:
///   http://hl7.org.au/fhir/core/StructureDefinition/au-core-patient
///     → hl7.fhir.au.core
///   http://hl7.org.au/fhir/StructureDefinition/au-hpio
///     → hl7.fhir.au.base
///   http://digitalhealth.gov.au/fhir/hcpd/StructureDefinition/hcpd-hpio
///     → (None — these are in the IG package itself)
fn url_to_package_id(url: &str) -> Option<String> {
    if url.contains("hl7.org.au/fhir/core") {
        Some("hl7.fhir.au.core".to_string())
    } else if url.contains("hl7.org.au/fhir") {
        Some("hl7.fhir.au.base".to_string())
    } else {
        None
    }
}

/// Merge parent snapshot elements into the child's snapshot.
///
/// Strategy:
/// 1. Build a set of child element ids
/// 2. For each parent element, if the child doesn't have it, add it
/// 3. For slice elements (id contains ':'), keep the child's version
///    (the child defines the slice)
/// 4. For non-slice elements the child does have, keep the child's version
///    (it may have tighter constraints)
fn merge_snapshot_elements(child: &mut StructureDefinition, parent_elements: &[ElementDefinition]) {
    let child_elements = match child.snapshot {
        Some(ref mut s) => &mut s.element,
        None => return,
    };

    // Build set of existing child element ids
    let child_ids: std::collections::HashSet<String> =
        child_elements.iter().map(|e| e.id.clone()).collect();

    // Add parent elements that don't exist in the child
    // (these provide the base definitions that the child constrains)
    for parent_el in parent_elements {
        if !child_ids.contains(&parent_el.id) {
            child_elements.push(parent_el.clone());
        }
    }

    // Sort elements by id to maintain FHIR ordering
    child_elements.sort_by(|a, b| a.id.cmp(&b.id));
}

/// Write a downloaded profile to the disk cache.
fn cache_profile(path: &std::path::Path, sd: &StructureDefinition) {
    if let Ok(json) = serde_json::to_string_pretty(sd) {
        std::fs::write(path, &json).ok();
    }
}

/// Download a StructureDefinition from the FHIR package registry or HL7 servers.
///
/// Tries multiple URL patterns:
/// 1. https://packages.fhir.org/StructureDefinition/<name>
/// 2. Domain-specific fallback based on the original URL's host
///
/// Results are cached to disk at ~/.cache/fhir-ig-testgen/packages/<name>.json
/// so subsequent runs don't re-download.
async fn download_profile(url: &str) -> Result<StructureDefinition> {
    // Strip FHIR version suffix (e.g. "|4.0.1") if present
    let clean_url = url.split('|').next().unwrap_or(url);
    // Extract the profile name from the URL
    let name = clean_url
        .rsplit('/')
        .next()
        .context("Cannot extract profile name from URL")?;

    // Check disk cache first
    let cache_path = cache_dir().join(format!("{}.json", name));
    if cache_path.exists() {
        let content = std::fs::read_to_string(&cache_path)?;
        if let Ok(sd) = serde_json::from_str::<StructureDefinition>(&content) {
            tracing::debug!("Using cached profile: {} ({})", sd.name, sd.url);
            return Ok(sd);
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("fhir-ig-testgen/0.1")
        .build()?;

    // Try the FHIR package registry first
    let registry_url = format!("https://packages.fhir.org/StructureDefinition/{}", name);
    let response = client
        .get(&registry_url)
        .header("Accept", "application/fhir+json")
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let sd: StructureDefinition = resp.json().await?;
            tracing::info!("Downloaded parent profile: {} ({})", sd.name, sd.url);
            cache_profile(&cache_path, &sd);
            return Ok(sd);
        }
        Ok(resp) => {
            tracing::debug!(
                "Registry returned {} for {}, trying HL7 fallback",
                resp.status(),
                url
            );
        }
        Err(e) => {
            tracing::debug!("Registry request failed for {}: {}", url, e);
        }
    }

    // Fallback: try domain-specific HL7 URLs
    // hl7.org.au → https://hl7.org.au/fhir/StructureDefinition-{name}.json
    // hl7.org    → try both direct .profile.json and StructureDefinition path
    let fallback_urls: Vec<String> = if clean_url.contains("hl7.org.au") {
        vec![format!(
            "https://hl7.org.au/fhir/StructureDefinition-{}.json",
            name
        )]
    } else {
        vec![
            format!("https://hl7.org/fhir/{}.profile.json", name),
            format!("https://hl7.org/fhir/StructureDefinition/{}", name),
        ]
    };

    let mut last_status = 0u16;
    for fallback_url in &fallback_urls {
        tracing::debug!("Trying HL7 fallback URL: {}", fallback_url);
        let response = client
            .get(fallback_url)
            .header("Accept", "application/fhir+json")
            .send()
            .await?;

        last_status = response.status().as_u16();
        tracing::debug!("HL7 fallback returned status: {}", last_status);

        if !response.status().is_success() {
            continue;
        }

        // Verify the response is actually JSON (some servers return HTML with 200)
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !content_type.contains("json") {
            tracing::debug!(
                "HL7 fallback returned non-JSON content-type: {}",
                content_type
            );
            continue;
        }

        match response.json::<StructureDefinition>().await {
            Ok(sd) => {
                tracing::info!(
                    "Downloaded parent profile from HL7: {} ({})",
                    sd.name,
                    sd.url
                );
                cache_profile(&cache_path, &sd);
                return Ok(sd);
            }
            Err(e) => {
                tracing::debug!("HL7 fallback JSON parse failed: {}", e);
                continue;
            }
        }
    }

    anyhow::bail!(
        "Failed to download parent profile {} from registry or HL7 (status: {})",
        url,
        last_status
    )
}

/// Collect slice definitions from a profile's snapshot.
/// Returns elements that have a sliceName (these are slice definitions).
pub fn collect_slice_definitions(profile: &StructureDefinition) -> Vec<&ElementDefinition> {
    let elements = match &profile.snapshot {
        Some(s) => &s.element,
        None => return Vec::new(),
    };

    elements.iter().filter(|e| e.slice_name.is_some()).collect()
}

/// Find the slicing discriminator for a given field path.
/// Returns the slicing info and matching slice elements.
pub fn find_slicing_info<'a>(
    elements: &'a [ElementDefinition],
    field_path: &str,
) -> Option<(&'a ElementSlicing, Vec<&'a ElementDefinition>)> {
    // Find the slicing element (the one with `slicing` set)
    let slicing_el = elements
        .iter()
        .find(|e| e.path == field_path && e.slicing.is_some())?;
    let slicing = slicing_el.slicing.as_ref()?;

    // Collect all slice elements (those with sliceName under this path)
    let slices: Vec<&ElementDefinition> = elements
        .iter()
        .filter(|e| e.slice_name.is_some() && e.id.starts_with(&format!("{}:", field_path)))
        .collect();

    Some((slicing, slices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_to_package_id() {
        assert_eq!(
            url_to_package_id("http://hl7.org.au/fhir/core/StructureDefinition/au-core-patient"),
            Some("hl7.fhir.au.core".to_string())
        );
        assert_eq!(
            url_to_package_id("http://hl7.org.au/fhir/StructureDefinition/au-hpio"),
            Some("hl7.fhir.au.base".to_string())
        );
        assert_eq!(
            url_to_package_id("http://hl7.org/fhir/StructureDefinition/Patient"),
            None
        );
    }

    #[test]
    fn test_merge_snapshot_elements() {
        let parent = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://hl7.org/fhir/StructureDefinition/AUBasePatient".to_string(),
            name: "AUBasePatient".to_string(),
            base_type: "Patient".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Patient".to_string(),
                        path: "Patient".to_string(),
                        min: Some(0),
                        max: Some("*".to_string()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Patient.identifier".to_string(),
                        path: "Patient.identifier".to_string(),
                        min: Some(0),
                        max: Some("*".to_string()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Patient.identifier:abn".to_string(),
                        path: "Patient.identifier".to_string(),
                        slice_name: Some("abn".to_string()),
                        min: Some(0),
                        max: Some("1".to_string()),
                        pattern_uri: Some("http://hl7.org.au/id/abn".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            differential: None,
        };

        let mut child = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/StructureDefinition/HcpdPatient".to_string(),
            name: "HcpdPatient".to_string(),
            base_type: "Patient".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: Some(
                "http://hl7.org/fhir/StructureDefinition/AUBasePatient".to_string(),
            ),
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Patient".to_string(),
                        path: "Patient".to_string(),
                        min: Some(0),
                        max: Some("*".to_string()),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Patient.identifier".to_string(),
                        path: "Patient.identifier".to_string(),
                        min: Some(1),
                        max: Some("*".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            differential: None,
        };

        merge_snapshot_elements(&mut child, &parent.snapshot.unwrap().element);

        let snapshot = child.snapshot.unwrap();
        // Should have 3 elements: Patient, Patient.identifier, Patient.identifier:abn
        assert_eq!(snapshot.element.len(), 3);
        // The slice should be present
        assert!(snapshot
            .element
            .iter()
            .any(|e| e.id == "Patient.identifier:abn"));
        // The child's identifier should have min=1 (child's constraint preserved)
        let ident = snapshot
            .element
            .iter()
            .find(|e| e.id == "Patient.identifier")
            .unwrap();
        assert_eq!(ident.min, Some(1));
    }

    #[test]
    fn test_collect_slice_definitions() {
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Test".to_string(),
            name: "Test".to_string(),
            base_type: "Patient".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Patient".to_string(),
                        path: "Patient".to_string(),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Patient.identifier".to_string(),
                        path: "Patient.identifier".to_string(),
                        ..Default::default()
                    },
                    ElementDefinition {
                        id: "Patient.identifier:abn".to_string(),
                        path: "Patient.identifier".to_string(),
                        slice_name: Some("abn".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            differential: None,
        };

        let slices = collect_slice_definitions(&profile);
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].slice_name.as_deref(), Some("abn"));
    }

    #[test]
    fn test_find_slicing_info() {
        let elements = vec![
            ElementDefinition {
                id: "Patient.identifier".to_string(),
                path: "Patient.identifier".to_string(),
                slicing: Some(ElementSlicing {
                    discriminator: vec![SlicingDiscriminator {
                        discriminator_type: "value".to_string(),
                        path: "system".to_string(),
                    }],
                    rules: Some("open".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ElementDefinition {
                id: "Patient.identifier:abn".to_string(),
                path: "Patient.identifier".to_string(),
                slice_name: Some("abn".to_string()),
                pattern_uri: Some("http://hl7.org.au/id/abn".to_string()),
                ..Default::default()
            },
        ];

        let result = find_slicing_info(&elements, "Patient.identifier");
        assert!(result.is_some());
        let (slicing, slices) = result.unwrap();
        assert_eq!(slicing.discriminator[0].path, "system");
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].slice_name.as_deref(), Some("abn"));
    }
}
