use crate::model::*;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Resolve the full parent chain for all StructureDefinitions in a package.
///
/// For each profile with a `baseDefinition`, check if the parent is already
/// loaded; if not, download it from the FHIR package registry or HL7 base.
/// Merges parent snapshot elements into the child's snapshot so that slice
/// definitions with their pattern values are available during resource generation.
pub fn resolve_parent_chain(profiles: &mut Vec<StructureDefinition>) -> Result<()> {
    // Build URL → index map for quick lookup
    let mut url_map: HashMap<String, usize> = HashMap::new();
    for (i, p) in profiles.iter().enumerate() {
        url_map.insert(p.url.clone(), i);
    }

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
            // Download the parent profile
            let parent = download_profile(&base_url)
                .with_context(|| format!("Failed to download parent profile: {}", base_url))?;

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

        // Merge: for each element in the child's snapshot, if it's a slice
        // (has sliceName), keep it. For non-slice elements, prefer the child's
        // version (it may have tighter constraints). Add any parent elements
        // that don't exist in the child.
        merge_snapshot_elements(&mut profiles[i], &parent_elements);

        i += 1;
    }

    Ok(())
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

/// Download a StructureDefinition from the FHIR package registry.
///
/// Tries multiple URL patterns:
/// 1. https://packages.fhir.org/StructureDefinition/<name>
/// 2. https://hl7.org/fhir/<name>.json (for base FHIR definitions)
fn download_profile(url: &str) -> Result<StructureDefinition> {
    // Strip FHIR version suffix (e.g. "|4.0.1") if present
    let clean_url = url.split('|').next().unwrap_or(url);
    // Extract the profile name from the URL
    let name = clean_url
        .rsplit('/')
        .next()
        .context("Cannot extract profile name from URL")?;

    // Try the FHIR package registry first
    let registry_url = format!("https://packages.fhir.org/StructureDefinition/{}", name);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client
        .get(&registry_url)
        .header("Accept", "application/fhir+json")
        .send();

    match response {
        Ok(resp) if resp.status().is_success() => {
            let sd: StructureDefinition = resp.json()?;
            tracing::info!("Downloaded parent profile: {} ({})", sd.name, sd.url);
            return Ok(sd);
        }
        Ok(resp) => {
            tracing::debug!(
                "Registry returned {} for {}, trying FHIR base",
                resp.status(),
                url
            );
        }
        Err(e) => {
            tracing::debug!("Registry request failed for {}: {}", url, e);
        }
    }

    // Fallback: try the HL7 FHIR base URL
    let base_url = format!("https://hl7.org/fhir/{}.json", name);
    let response = client
        .get(&base_url)
        .header("Accept", "application/fhir+json")
        .send()?;

    if response.status().is_success() {
        let sd: StructureDefinition = response.json()?;
        tracing::info!(
            "Downloaded parent profile from HL7: {} ({})",
            sd.name,
            sd.url
        );
        Ok(sd)
    } else {
        anyhow::bail!(
            "Failed to download parent profile {} from registry or HL7 (status: {})",
            url,
            response.status()
        )
    }
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
