pub mod generators;
pub mod overlay;
pub mod supplement;
pub mod update;

use crate::generate::hcpd;
use crate::generate::resource_generator::{
    build_code_system_first_code_map, generate_resource_with_value_sets,
};
use crate::model::profile::StructureDefinition;
use anyhow::Result;
use chrono::{Duration, Utc};
use rand::Rng;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

pub use generators::{
    gen_endpoint, gen_generic, gen_healthcare_service, gen_location, gen_organization,
    gen_practitioner, gen_practitioner_role,
};
pub use overlay::overlay_cross_references;
pub use supplement::{generate_supplement_resource, write_supplement_ndjson};
pub use update::generate_update_ndjson;

/// IDs allocated during bulk generation.
/// Maps resource type → list of generated IDs.
pub type IdStore = HashMap<String, Vec<String>>;

/// Generate bulk FHIR resources as NDJSON files.
///
/// Writes one `.ndjson` file per resource type under `output_dir/data/`,
/// plus a `combined.ndjson` containing all resources in dependency order
/// (suitable for bulk import where linked items must resolve).
/// Returns an `IdStore` mapping each resource type to its generated IDs,
/// which is used to resolve cross-references during generation.
pub fn generate_bulk_data(
    counts: &HashMap<String, u64>,
    profile_urls: &HashMap<String, String>,
    profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
    raw_resources: &HashMap<String, serde_json::Value>,
    output_dir: &Path,
) -> Result<IdStore> {
    use std::io::BufWriter;

    let data_dir = output_dir.join("data");
    std::fs::create_dir_all(&data_dir)?;
    let mut rng = rand::rng();

    // Detect whether this is an HCPD/AU IG so we only apply HCPD-specific fixes
    // when appropriate. Any other IG should not have AU-specific identifiers injected.
    let hcpd_ig = hcpd::is_hcpd_ig(profile_urls);

    // Pre-compute CodeSystem first-code map for use in coding lookups
    // (e.g. responsible-party-type for suppressedBy extension).
    let code_system_codes = build_code_system_first_code_map(raw_resources);

    // Determine creation order: dependent types first.
    // Organizations and Locations have no FHIR references, so they go first.
    // Practitioners reference nothing. PractitionerRoles and HealthcareServices
    // reference Practitioners, Organizations, and Locations.
    let order = bulk_data_creation_order(counts);

    // First pass: allocate all IDs so cross-references can be resolved.
    let mut ids: IdStore = HashMap::new();
    for resource_type in &order {
        let count = counts.get(resource_type).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let type_ids: Vec<String> = (0..count)
            .map(|i| format!("{}-{}", resource_type.to_lowercase(), i + 1))
            .collect();
        ids.insert(resource_type.clone(), type_ids);
    }

    // Pre-clone ID vectors for cross-referencing (avoids cloning inside hot loops).
    let org_ids = ids.get("Organization").cloned().unwrap_or_default();
    let prac_ids = ids.get("Practitioner").cloned().unwrap_or_default();
    let loc_ids = ids.get("Location").cloned().unwrap_or_default();
    let hs_ids = ids.get("HealthcareService").cloned().unwrap_or_default();
    let practitioner_role_ids = ids.get("PractitionerRole").cloned().unwrap_or_default();
    let endpoint_ids = ids.get("Endpoint").cloned().unwrap_or_default();

    // Build lookups so generation can prefer the exact profile URL from the
    // CapabilityStatement instead of an arbitrary StructureDefinition that
    // happens to share the same base type.
    let profile_by_url: HashMap<&str, &StructureDefinition> =
        profiles.iter().map(|p| (p.url.as_str(), p)).collect();
    let profile_by_base_type: HashMap<&str, &StructureDefinition> =
        profiles.iter().map(|p| (p.base_type.as_str(), p)).collect();

    // Track practitioner registration numbers so PractitionerRole can re-use
    // the referenced practitioner's registration identifier.
    let mut practitioner_registration_by_id: HashMap<String, String> = HashMap::new();

    // Open combined.ndjson to collect all resources in import order.
    let combined_path = data_dir.join("combined.ndjson");
    let combined_file = std::fs::File::create(&combined_path)?;
    let mut combined_writer = BufWriter::new(combined_file);

    // Second pass: generate and write resources with buffered I/O.
    for resource_type in &order {
        let count = counts.get(resource_type).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let type_ids = ids.get(resource_type).cloned().unwrap_or_default();
        let path = data_dir.join(format!("{}.ndjson", resource_type));
        let file = std::fs::File::create(&path)?;
        let mut writer = BufWriter::new(file);
        let mut written = 0u64;

        // Resolve the profile URL for this resource type: prefer the IG's
        // profile, fall back to the base FHIR profile.
        let profile_url = profile_urls.get(resource_type).cloned().unwrap_or_else(|| {
            format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type)
        });

        for id in type_ids.iter() {
            let selected_profile = profile_urls
                .get(resource_type)
                .and_then(|url| profile_by_url.get(url.as_str()).copied())
                .or_else(|| profile_by_base_type.get(resource_type.as_str()).copied());

            let mut resource = if let Some(profile) = selected_profile {
                // Use profile-aware generation: generates a conformant base from
                // the StructureDefinition, then overlay cross-references.
                let mut r =
                    generate_resource_with_value_sets(profile, profiles, value_set_systems)?;
                r["id"] = serde_json::Value::String(id.clone());
                // Overlay cross-references for types that need them.
                overlay::overlay_cross_references(
                    &mut r,
                    resource_type,
                    id,
                    &org_ids,
                    &prac_ids,
                    &loc_ids,
                    &hs_ids,
                    &practitioner_role_ids,
                    &endpoint_ids,
                    &mut rng,
                );
                if hcpd_ig {
                    hcpd::apply_hcpd_bulk_fixes(
                        &mut r,
                        resource_type,
                        id,
                        &mut practitioner_registration_by_id,
                        value_set_systems,
                        &code_system_codes,
                        &mut rng,
                    );
                }
                r
            } else {
                match resource_type.as_str() {
                    "Organization" => generators::gen_organization(id, &mut rng),
                    "Practitioner" => generators::gen_practitioner(id, &mut rng),
                    "PractitionerRole" => generators::gen_practitioner_role(
                        id, &org_ids, &prac_ids, &loc_ids, &mut rng,
                    ),
                    "Location" => generators::gen_location(id, &mut rng),
                    "HealthcareService" => {
                        generators::gen_healthcare_service(id, &org_ids, &loc_ids, &mut rng)
                    }
                    "Endpoint" => generators::gen_endpoint(id, &org_ids, &mut rng),
                    // Generic fallback for any resource type not explicitly handled
                    _ => generators::gen_generic(resource_type, id, &mut rng),
                }
            };

            // When NOT using profile-aware generation (i.e. falling back to
            // the hardcoded gen_* functions), stamp the profile URL from the
            // IG package to override the hardcoded Plan-Net defaults.
            // When using generate_resource(), the profile URL is already set
            // correctly from the StructureDefinition, so skip the override.
            if selected_profile.is_none() {
                resource["meta"]["profile"] = serde_json::json!([profile_url]);
            }

            // Stamp a random created date within the last 12 months
            stamp_created_date(&mut resource, &mut rng);

            serde_json::to_writer(&mut writer, &resource)?;
            writeln!(writer)?;
            // Also write to combined.ndjson in import order.
            serde_json::to_writer(&mut combined_writer, &resource)?;
            writeln!(combined_writer)?;
            written += 1;
            if written.is_multiple_of(10_000) {
                // Flush progress to disk so external observers see the file growing.
                writer.flush()?;
                tracing::info!(
                    "Generated {}/{} {} resources",
                    written,
                    count,
                    resource_type
                );
            }
        }
        writer.flush()?;
        tracing::info!(
            "Wrote {} {} resources to {}",
            written,
            resource_type,
            path.display()
        );
    }

    combined_writer.flush()?;
    tracing::info!("Wrote all resources to {}", combined_path.display());

    Ok(ids)
}

pub fn bulk_data_creation_order(counts: &HashMap<String, u64>) -> Vec<String> {
    let mut order = Vec::new();

    // Tier 1: root resources
    for t in &["Organization", "Practitioner"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }

    // Tier 2: depends on Organization
    for t in &["Endpoint"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }

    // Tier 3: depends on Organization and Endpoint
    for t in &["Location"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }

    // Tier 4: depends on Organization, Endpoint, and Location
    for t in &["HealthcareService"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }

    // Tier 5: depends on Practitioner, Organization, Endpoint, Location,
    // and HealthcareService
    for t in &["PractitionerRole"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }

    // Tier 6: may reference several resource pools and should come last.
    for t in &["Provenance"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }
    // Anything else not yet ordered
    for t in counts.keys() {
        if !order.contains(t) {
            order.push(t.clone());
        }
    }
    order
}

/// Pick a random ID from a list, returning a FHIR reference string.
pub(crate) fn random_ref(resource_type: &str, ids: &[String], rng: &mut impl Rng) -> String {
    if ids.is_empty() {
        // Fallback if no IDs exist for the referenced type
        format!("{}/placeholder-1", resource_type)
    } else {
        let idx = rng.random_range(0..ids.len());
        format!("{}/{}", resource_type, ids[idx])
    }
}

/// Pick N random IDs from a list (without replacement if possible).
#[allow(dead_code)]
pub(crate) fn random_refs(
    resource_type: &str,
    ids: &[String],
    n: usize,
    rng: &mut impl Rng,
) -> Vec<String> {
    if ids.is_empty() {
        vec![format!("{}/placeholder-1", resource_type)]
    } else {
        let n = n.min(ids.len());
        let mut chosen = Vec::with_capacity(n);
        let mut indices: Vec<usize> = (0..ids.len()).collect();
        // Fisher-Yates shuffle to pick n random indices
        for i in (0..indices.len()).rev().take(n) {
            let j = rng.random_range(0..=i);
            indices.swap(i, j);
        }
        for &idx in &indices[..n] {
            chosen.push(format!("{}/{}", resource_type, ids[idx]));
        }
        chosen
    }
}

/// Stamp a random `meta.lastUpdated` date on a resource, within the last 12 months.
pub(crate) fn stamp_created_date(resource: &mut serde_json::Value, rng: &mut impl Rng) {
    let now = Utc::now();
    let days_ago = rng.random_range(0..365);
    let created = now - Duration::days(days_ago);
    let date_str = created.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    resource["meta"]["lastUpdated"] = serde_json::Value::String(date_str);
}

#[cfg(test)]
mod tests;
