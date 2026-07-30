use crate::generate::hcpd;
use crate::generate::locality::random_au_locality;
use crate::generate::resource_generator::{
    build_code_system_first_code_map, generate_resource_with_value_sets,
};
use crate::model::profile::StructureDefinition;
use anyhow::Result;
use chrono::{Duration, Utc};
use fake::Fake;
use rand::Rng;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

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
                overlay_cross_references(
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
                    "Organization" => gen_organization(id, &mut rng),
                    "Practitioner" => gen_practitioner(id, &mut rng),
                    "PractitionerRole" => {
                        gen_practitioner_role(id, &org_ids, &prac_ids, &loc_ids, &mut rng)
                    }
                    "Location" => gen_location(id, &mut rng),
                    "HealthcareService" => gen_healthcare_service(id, &org_ids, &loc_ids, &mut rng),
                    "Endpoint" => gen_endpoint(id, &org_ids, &mut rng),
                    // Generic fallback for any resource type not explicitly handled
                    _ => gen_generic(resource_type, id, &mut rng),
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

/// Generate a single supplement resource for a resource type that has no bulk data count.
///
/// Used to ensure conformance must_support tests can always find a resource with the
/// expected ID pattern (`{resourcetype}-1`). Works with any FHIR IG by using the
/// profile-aware generator as the primary source and falling back to generic generation.
///
/// Applies IG-specific fixes (e.g. HCPD identifiers) only when the IG is detected as HCPD/AU.
pub fn generate_supplement_resource(
    resource_type: &str,
    profile_urls: &HashMap<String, String>,
    profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
    raw_resources: &HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value> {
    let id = format!("{}-1", resource_type.to_lowercase());
    let mut rng = rand::rng();

    let profile_by_url: HashMap<&str, &StructureDefinition> =
        profiles.iter().map(|p| (p.url.as_str(), p)).collect();
    let profile_by_base_type: HashMap<&str, &StructureDefinition> =
        profiles.iter().map(|p| (p.base_type.as_str(), p)).collect();

    let profile_url = profile_urls
        .get(resource_type)
        .cloned()
        .unwrap_or_else(|| format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type));

    let selected_profile = profile_urls
        .get(resource_type)
        .and_then(|url| profile_by_url.get(url.as_str()).copied())
        .or_else(|| profile_by_base_type.get(resource_type).copied());

    let mut resource = if let Some(profile) = selected_profile {
        let mut r = generate_resource_with_value_sets(profile, profiles, value_set_systems)?;
        r["id"] = serde_json::Value::String(id.clone());
        let mut dummy_reg: HashMap<String, String> = HashMap::new();
        if hcpd::is_hcpd_ig(profile_urls) {
            hcpd::apply_hcpd_bulk_fixes(
                &mut r,
                resource_type,
                &id,
                &mut dummy_reg,
                value_set_systems,
                &build_code_system_first_code_map(raw_resources),
                &mut rng,
            );
        }
        r
    } else {
        match resource_type {
            "Organization" => gen_organization(&id, &mut rng),
            "Practitioner" => gen_practitioner(&id, &mut rng),
            "PractitionerRole" => gen_practitioner_role(&id, &[], &[], &[], &mut rng),
            "Location" => gen_location(&id, &mut rng),
            "HealthcareService" => gen_healthcare_service(&id, &[], &[], &mut rng),
            "Endpoint" => gen_endpoint(&id, &[], &mut rng),
            _ => gen_generic(resource_type, &id, &mut rng),
        }
    };

    if selected_profile.is_none() {
        resource["meta"]["profile"] = serde_json::json!([profile_url]);
    }

    stamp_created_date(&mut resource, &mut rng);

    // Normalize all Reference values to use the predictable {type}-1 pattern.
    // Generated references use random UUIDs that point to non-existent resources;
    // replacing them ensures supplement resources can be uploaded without referential
    // integrity errors.
    normalize_supplement_references(&mut resource);

    Ok(resource)
}

/// Walk a JSON value and replace any `"reference": "ResourceType/some-uuid"` with
/// `"reference": "ResourceType/resourcetype-1"` so supplement resources always
/// point to the predictable IDs used by other supplement resources.
/// The abstract `Resource` base type is mapped to `Organization` as a concrete fallback.
fn normalize_supplement_references(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(obj) => {
            if let Some(ref_val) = obj.get_mut("reference")
                && let Some(s) = ref_val.as_str()
                && let Some((rtype, _id)) = s.split_once('/')
            {
                // Map the abstract FHIR `Resource` base type to a concrete
                // type that is always present from bulk data.
                let concrete_type = if rtype == "Resource" {
                    "Organization"
                } else {
                    rtype
                };
                let new_id = format!("{}-1", concrete_type.to_lowercase());
                *ref_val = serde_json::Value::String(format!("{}/{}", concrete_type, new_id));
            }
            for v in obj.values_mut() {
                normalize_supplement_references(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_supplement_references(v);
            }
        }
        _ => {}
    }
}

/// Generate supplement resources for all resource types in `creation_order` that
/// have no entry in `bulk_counts`, write each to `{output_dir}/data/{Type}.ndjson`,
/// append all to `combined.ndjson`, and return an IdStore so callers can include
/// them in `generate_update_ndjson` and `upload_ndjson_files`.
///
/// FHIR data types (Extension, Identifier, etc.) that are not standalone resources
/// are silently skipped.
pub fn write_supplement_ndjson(
    creation_order: &[String],
    bulk_counts: &HashMap<String, u64>,
    profile_urls: &HashMap<String, String>,
    profiles: &[StructureDefinition],
    value_set_systems: &HashMap<String, String>,
    raw_resources: &HashMap<String, serde_json::Value>,
    output_dir: &Path,
) -> Result<IdStore> {
    use std::io::{BufWriter, Write};

    let data_dir = output_dir.join("data");
    std::fs::create_dir_all(&data_dir)?;

    // Open combined.ndjson in append mode so supplement resources follow the bulk data.
    let combined_path = data_dir.join("combined.ndjson");
    let combined_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&combined_path)?;
    let mut combined_writer = BufWriter::new(combined_file);

    let mut supplement_ids: IdStore = HashMap::new();

    for resource_type in creation_order {
        let count = bulk_counts.get(resource_type).copied().unwrap_or(0);
        if count > 0 || super::NON_RESOURCE_TYPES.contains(&resource_type.as_str()) {
            continue;
        }

        let resource = match generate_supplement_resource(
            resource_type,
            profile_urls,
            profiles,
            value_set_systems,
            raw_resources,
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

        // Write to per-type NDJSON file
        let type_path = data_dir.join(format!("{}.ndjson", resource_type));
        let type_file = std::fs::File::create(&type_path)?;
        let mut type_writer = BufWriter::new(type_file);
        serde_json::to_writer(&mut type_writer, &resource)?;
        writeln!(type_writer)?;
        type_writer.flush()?;

        // Append to combined.ndjson
        serde_json::to_writer(&mut combined_writer, &resource)?;
        writeln!(combined_writer)?;

        tracing::info!("Wrote supplement resource: {}/{}", resource_type, id);
        supplement_ids.insert(resource_type.clone(), vec![id]);
    }

    combined_writer.flush()?;
    Ok(supplement_ids)
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

/// Generate an `update.ndjson` file containing the same resources as the
/// initial bulk data, but with 1–2 randomly updated parameters per resource.
///
/// Each resource retains its original `id` so the update file can be used
/// to test update operations (e.g. PUT /{ResourceType}/{id}) against a
/// server that already has the initial data loaded.
///
/// The update file is written to `{output_dir}/data/update.ndjson`.
pub fn generate_update_ndjson(ids: &IdStore, output_dir: &Path) -> Result<()> {
    use std::io::BufWriter;

    let data_dir = output_dir.join("data");
    let update_path = data_dir.join("update.ndjson");
    let file = std::fs::File::create(&update_path)?;
    let mut writer = BufWriter::new(file);
    let mut rng = rand::rng();
    let mut total = 0u64;

    // Process resource types in the same order as initial generation
    // so the update file has a consistent ordering.
    let order = bulk_data_creation_order(
        &ids.iter()
            .map(|(k, v)| (k.clone(), v.len() as u64))
            .collect(),
    );

    for resource_type in &order {
        if !ids.contains_key(resource_type) || ids[resource_type].is_empty() {
            continue;
        }

        // Read the original NDJSON file for this type
        let ndjson_path = data_dir.join(format!("{}.ndjson", resource_type));
        let contents = match std::fs::read_to_string(&ndjson_path) {
            Ok(c) => c,
            Err(_) => continue, // skip types that weren't written
        };

        for line in contents.lines().filter(|l| !l.is_empty()) {
            let mut resource: serde_json::Value = serde_json::from_str(line)?;
            apply_random_updates(&mut resource, resource_type, &mut rng);
            serde_json::to_writer(&mut writer, &resource)?;
            writeln!(writer)?;
            total += 1;
        }
    }

    writer.flush()?;
    tracing::info!(
        "Wrote {} updated resources to {}",
        total,
        update_path.display()
    );
    Ok(())
}

/// Apply 1–2 random mutations to a resource, keeping the same `id`.
///
/// Works generically for any FHIR resource type by walking the JSON tree
/// to find mutable leaf values (strings, numbers, booleans) and picking
/// 1–2 at random. Skips `resourceType`, `id`, `meta`, and reference fields
/// to keep the resource structurally valid.
fn apply_random_updates(
    resource: &mut serde_json::Value,
    _resource_type: &str,
    rng: &mut rand::rngs::ThreadRng,
) {
    // Discover mutable leaf paths dynamically from the resource JSON.
    let candidates = discover_mutable_paths(resource);
    if candidates.is_empty() {
        return;
    }

    let n_updates = rng.random_range(1..=candidates.len().min(2));
    let mut chosen_indices: Vec<usize> = (0..candidates.len()).collect();
    // Fisher-Yates partial shuffle
    for i in (0..chosen_indices.len()).rev().take(n_updates) {
        let j = rng.random_range(0..=i);
        chosen_indices.swap(i, j);
    }

    // Collect the chosen (path, mutator) pairs as owned data so we can
    // release the immutable borrow on `resource` before mutating it.
    let chosen: Vec<(String, MutatorFn)> = chosen_indices[..n_updates]
        .iter()
        .map(|&idx| {
            let (path, mutator) = &candidates[idx];
            (path.clone(), *mutator)
        })
        .collect();

    for (path, mutator) in &chosen {
        mutator(resource, path, rng);
    }
}

/// Type alias for a mutator function that modifies a resource field.
type MutatorFn = fn(&mut serde_json::Value, &str, &mut rand::rngs::ThreadRng);

/// Walk a resource JSON tree and collect paths to mutable leaf values.
///
/// Skips fields that would break structural validity if changed:
/// - `resourceType`, `id` — identity fields
/// - `meta` — profile/versioning metadata
/// - Any field whose value is a FHIR reference string (e.g. `Organization/123`)
/// - Array indices (we mutate individual elements, not the array itself)
///
/// Returns a list of (dotted path, appropriate mutator) pairs.
fn discover_mutable_paths(resource: &serde_json::Value) -> Vec<(String, MutatorFn)> {
    let mut candidates: Vec<(String, MutatorFn)> = Vec::new();
    let mut prefix = String::new();
    walk_for_mutables(resource, &mut prefix, &mut candidates);
    candidates
}

/// Recursive walker that collects leaf-value paths.
fn walk_for_mutables(
    value: &serde_json::Value,
    prefix: &mut String,
    candidates: &mut Vec<(String, MutatorFn)>,
) {
    match value {
        serde_json::Value::Object(obj) => {
            let saved_len = prefix.len();
            for (key, val) in obj {
                // Skip identity and structural fields
                if key == "resourceType" || key == "id" || key == "meta" {
                    continue;
                }
                // Skip reference fields — changing them would break cross-references
                if is_reference_field(key, val) {
                    continue;
                }

                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(key);
                walk_for_mutables(val, prefix, candidates);
                prefix.truncate(saved_len);
            }
        }
        serde_json::Value::Array(arr) => {
            let saved_len = prefix.len();
            for (i, item) in arr.iter().enumerate() {
                // Only recurse into objects inside arrays (e.g. name[0], telecom[1])
                // Skip primitive arrays (e.g. line: ["100 George St"])
                if item.is_object() {
                    let idx_str = format!("[{}]", i);
                    prefix.push_str(&idx_str);
                    walk_for_mutables(item, prefix, candidates);
                    prefix.truncate(saved_len);
                }
            }
        }
        serde_json::Value::String(s) => {
            // Skip FHIR reference strings (e.g. "Practitioner/org-1")
            if s.contains('/') && !s.starts_with("http") {
                return;
            }
            candidates.push((prefix.clone(), mutate_string));
        }
        serde_json::Value::Number(_) => {
            candidates.push((prefix.clone(), mutate_number));
        }
        serde_json::Value::Bool(_) => {
            candidates.push((prefix.clone(), mutate_bool));
        }
        serde_json::Value::Null => {}
    }
}

/// Check if a field is a FHIR reference (has `reference` key with a value like "ResourceType/id").
fn is_reference_field(key: &str, val: &serde_json::Value) -> bool {
    if key == "reference"
        && let Some(s) = val.as_str()
        && s.contains('/')
        && !s.starts_with("http")
    {
        return true;
    }
    // Also skip managingOrganization, providedBy, practitioner, etc. as whole objects
    // since they contain references — but we still want to mutate their display field.
    false
}

/// Get a reference to a value at a dotted path (e.g. `address[0].city`).
fn get_at_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let parts = path.split('.');
    let mut current = value;
    for part in parts {
        if let Some(idx_str) = part.strip_suffix(']') {
            // Array access: name[0]
            let (array_key, index_str) = idx_str.split_once('[')?;
            let idx: usize = index_str.parse().ok()?;
            current = current.get(array_key)?.get(idx)?;
        } else {
            current = current.get(part)?;
        }
    }
    Some(current)
}

/// Set a value at a dotted path (e.g. `address[0].city`).
fn set_at_path(value: &mut serde_json::Value, path: &str, new_val: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    // Navigate to the parent of the target, then set.
    let mut current = value;
    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;
        if let Some(idx_str) = part.strip_suffix(']') {
            let (array_key, index_str) = idx_str.split_once('[').unwrap();
            let idx: usize = index_str.parse().unwrap();
            let arr = current
                .get_mut(array_key)
                .and_then(|v| v.as_array_mut())
                .expect("array path must exist");
            if is_last {
                arr[idx] = new_val;
                return;
            }
            current = &mut arr[idx];
        } else if is_last {
            current[part] = new_val;
            return;
        } else {
            current = current.get_mut(part).expect("path must exist");
        }
    }
}

// ── Mutator functions ─────────────────────────────────────────────────────

fn mutate_string(value: &mut serde_json::Value, path: &str, _rng: &mut rand::rngs::ThreadRng) {
    let new_val = serde_json::Value::String(fake::faker::lorem::en::Word().fake());
    set_at_path(value, path, new_val);
}

fn mutate_number(value: &mut serde_json::Value, path: &str, rng: &mut rand::rngs::ThreadRng) {
    let current = get_at_path(value, path)
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    // Jitter by ±10%
    let delta = current * 0.1;
    let new_val = current + rng.random_range(-delta..=delta);
    // Keep 2 decimal places for readability
    let rounded = (new_val * 100.0).round() / 100.0;
    set_at_path(value, path, serde_json::json!(rounded));
}

fn mutate_bool(value: &mut serde_json::Value, path: &str, _rng: &mut rand::rngs::ThreadRng) {
    let current = get_at_path(value, path)
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    set_at_path(value, path, serde_json::Value::Bool(!current));
}

/// Pick a random ID from a list, returning a FHIR reference string.
/// Overlay cross-references onto a profile-generated resource.
///
/// When `generate_resource` produces a resource from a StructureDefinition,
/// it creates required fields but cannot know about the IDs of other
/// resources in the bulk data set. This function fills in cross-references
/// (practitioner, organization, location, healthcareService) that the
/// profile may require but which depend on other generated resources.
#[allow(clippy::too_many_arguments)]
fn overlay_cross_references(
    resource: &mut serde_json::Value,
    resource_type: &str,
    _id: &str,
    org_ids: &[String],
    prac_ids: &[String],
    loc_ids: &[String],
    hs_ids: &[String],
    practitioner_role_ids: &[String],
    endpoint_ids: &[String],
    rng: &mut impl Rng,
) {
    let obj = match resource.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    match resource_type {
        "PractitionerRole" => {
            // HCPD requires practitioner, organization, and healthcareService references.
            // practitioner and organization are always required; healthcareService
            // references depend on HealthcareService IDs which may not exist yet.
            // Ensure deterministic coverage for _revinclude=PractitionerRole:practitioner
            // on practitioner-1.
            if !prac_ids.is_empty() {
                let ref_str = if _id == "practitionerrole-1"
                    && prac_ids.iter().any(|id| id == "practitioner-1")
                {
                    "Practitioner/practitioner-1".to_string()
                } else {
                    random_ref("Practitioner", prac_ids, rng)
                };
                obj.insert(
                    "practitioner".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !org_ids.is_empty() {
                let ref_str = random_ref("Organization", org_ids, rng);
                obj.insert(
                    "organization".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !loc_ids.is_empty() {
                let ref_str = random_ref("Location", loc_ids, rng);
                obj.insert(
                    "location".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
            if !hs_ids.is_empty() {
                let ref_str = random_ref("HealthcareService", hs_ids, rng);
                obj.insert(
                    "healthcareService".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
            if !endpoint_ids.is_empty() {
                let ref_str = random_ref("Endpoint", endpoint_ids, rng);
                obj.insert(
                    "endpoint".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
        }
        "Location" if !org_ids.is_empty() => {
            // Ensure at least one deterministic reverse include path for
            // Organization/_revinclude=Location:organization on organization-1.
            let ref_str = if _id == "location-1" && org_ids.iter().any(|id| id == "organization-1")
            {
                "Organization/organization-1".to_string()
            } else {
                random_ref("Organization", org_ids, rng)
            };
            obj.insert(
                "managingOrganization".to_string(),
                serde_json::json!({ "reference": ref_str }),
            );
            // Ensure deterministic coverage for _revinclude=Location:endpoint
            // on endpoint-1.
            if !endpoint_ids.is_empty() {
                let endpoint_ref =
                    if _id == "location-1" && endpoint_ids.iter().any(|id| id == "endpoint-1") {
                        "Endpoint/endpoint-1".to_string()
                    } else {
                        random_ref("Endpoint", endpoint_ids, rng)
                    };
                obj.insert(
                    "endpoint".to_string(),
                    serde_json::json!([{ "reference": endpoint_ref }]),
                );
            }
        }
        "HealthcareService" => {
            if !org_ids.is_empty() {
                let ref_str = random_ref("Organization", org_ids, rng);
                obj.insert(
                    "providedBy".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !loc_ids.is_empty() {
                let ref_str = random_ref("Location", loc_ids, rng);
                obj.insert(
                    "location".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
            if !endpoint_ids.is_empty() {
                let ref_str = random_ref("Endpoint", endpoint_ids, rng);
                obj.insert(
                    "endpoint".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
            // Replace coverageArea references — the profile-aware generator
            // extracts the resource type from the target profile URL, but
            // profile names like "hcpd-service-coverage-area" are not valid
            // FHIR resource types. coverageArea should reference Location.
            if !loc_ids.is_empty() {
                let ref_str = random_ref("Location", loc_ids, rng);
                obj.insert(
                    "coverageArea".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            } else {
                obj.remove("coverageArea");
            }
        }
        "Endpoint" if !org_ids.is_empty() => {
            let ref_str = random_ref("Organization", org_ids, rng);
            obj.insert(
                "managingOrganization".to_string(),
                serde_json::json!({ "reference": ref_str }),
            );
        }
        "Organization" if !org_ids.is_empty() => {
            // Replace partOf with a valid reference to another Organization.
            // If there's only one Organization, remove partOf entirely since
            // there's no parent to reference — HAPI will reject a UUID that
            // doesn't exist on the server.
            if org_ids.len() > 1 {
                let ref_str = random_ref("Organization", org_ids, rng);
                obj.insert(
                    "partOf".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            } else {
                obj.remove("partOf");
            }
        }
        "Provenance" => {
            let target_ref = provenance_target_for_id(
                _id,
                org_ids,
                prac_ids,
                loc_ids,
                hs_ids,
                practitioner_role_ids,
                endpoint_ids,
                rng,
            );

            if let Some(ref_str) = target_ref.as_deref() {
                obj.insert(
                    "target".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }

            if !org_ids.is_empty() {
                obj.insert(
                    "agent".to_string(),
                    serde_json::json!([
                        {
                            "who": {
                                "reference": random_ref("Organization", org_ids, rng)
                            }
                        }
                    ]),
                );
            }

            if let Some(ref_str) = target_ref {
                obj.insert(
                    "entity".to_string(),
                    serde_json::json!([
                        {
                            "role": "source",
                            "what": {
                                "reference": ref_str
                            }
                        }
                    ]),
                );
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn provenance_target_for_id(
    provenance_id: &str,
    org_ids: &[String],
    prac_ids: &[String],
    loc_ids: &[String],
    hs_ids: &[String],
    practitioner_role_ids: &[String],
    endpoint_ids: &[String],
    rng: &mut impl Rng,
) -> Option<String> {
    // Deterministically seed coverage for _revinclude=Provenance:target checks
    // that query with _id={type}-1.
    let seeded_targets = [
        ("Organization", org_ids),
        ("Practitioner", prac_ids),
        ("Location", loc_ids),
        ("HealthcareService", hs_ids),
        ("PractitionerRole", practitioner_role_ids),
        ("Endpoint", endpoint_ids),
    ];
    if let Some(seed_index) = provenance_sequence_number(provenance_id) {
        let mut available: Vec<(&str, &str)> = Vec::new();
        for (rtype, ids) in seeded_targets {
            if let Some(first_id) = ids.first() {
                available.push((rtype, first_id.as_str()));
            }
        }
        if !available.is_empty() {
            let idx = (seed_index - 1) % available.len();
            let (rtype, id) = available[idx];
            return Some(format!("{}/{}", rtype, id));
        }
    }

    // After deterministic seed coverage is established, spread remaining
    // references randomly across all available resource types.
    let mut candidates: Vec<String> = Vec::new();
    candidates.extend(org_ids.iter().map(|id| format!("Organization/{}", id)));
    candidates.extend(prac_ids.iter().map(|id| format!("Practitioner/{}", id)));
    candidates.extend(loc_ids.iter().map(|id| format!("Location/{}", id)));
    candidates.extend(hs_ids.iter().map(|id| format!("HealthcareService/{}", id)));
    candidates.extend(
        practitioner_role_ids
            .iter()
            .map(|id| format!("PractitionerRole/{}", id)),
    );
    candidates.extend(endpoint_ids.iter().map(|id| format!("Endpoint/{}", id)));

    if candidates.is_empty() {
        None
    } else {
        Some(candidates[rng.random_range(0..candidates.len())].clone())
    }
}

fn provenance_sequence_number(provenance_id: &str) -> Option<usize> {
    provenance_id
        .rsplit_once('-')
        .and_then(|(_, n)| n.parse::<usize>().ok())
        .filter(|n| *n > 0)
}

fn random_ref(resource_type: &str, ids: &[String], rng: &mut impl Rng) -> String {
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
fn random_refs(resource_type: &str, ids: &[String], n: usize, rng: &mut impl Rng) -> Vec<String> {
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

// ── FHIR Resource Generators ──────────────────────────────────────────────

#[derive(Serialize)]
struct FhirOrganization {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifier: Vec<Identifier>,
    #[serde(rename = "type")]
    org_type: Vec<CodeableConcept>,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    telecom: Option<Vec<ContactPoint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<Vec<Address>>,
}

#[derive(Serialize)]
struct FhirPractitioner {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    name: Vec<HumanName>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifier: Vec<Identifier>,
    active: bool,
    #[serde(rename = "birthDate", skip_serializing_if = "Option::is_none")]
    birth_date: Option<String>,
    #[serde(rename = "gender", skip_serializing_if = "Option::is_none")]
    gender: Option<String>,
}

#[derive(Serialize)]
struct FhirPractitionerRole {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    active: bool,
    practitioner: Reference,
    organization: Reference,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    code: Vec<CodeableConcept>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    specialty: Vec<CodeableConcept>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    location: Vec<Reference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    telecom: Vec<ContactPoint>,
    #[serde(rename = "availableTime", skip_serializing_if = "Vec::is_empty")]
    available_time: Vec<AvailableTime>,
}

#[derive(Serialize)]
struct FhirLocation {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    status: String,
    name: String,
    #[serde(rename = "type", skip_serializing_if = "Vec::is_empty")]
    loc_type: Vec<CodeableConcept>,
    #[serde(rename = "physicalType", skip_serializing_if = "Option::is_none")]
    physical_type: Option<CodeableConcept>,
    position: Position,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<Address>,
    #[serde(
        rename = "managingOrganization",
        skip_serializing_if = "Option::is_none"
    )]
    managing_organization: Option<Reference>,
}

#[derive(Serialize)]
struct FhirHealthcareService {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    active: bool,
    #[serde(rename = "providedBy")]
    provided_by: Reference,
    #[serde(rename = "type", skip_serializing_if = "Vec::is_empty")]
    svc_type: Vec<CodeableConcept>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    specialty: Vec<CodeableConcept>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    location: Vec<Reference>,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
}

#[derive(Serialize)]
struct FhirEndpoint {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    status: String,
    #[serde(rename = "connectionType")]
    connection_type: Coding,
    name: String,
    #[serde(rename = "payloadType")]
    payload_type: Vec<CodeableConcept>,
    address: String,
    #[serde(
        rename = "managingOrganization",
        skip_serializing_if = "Option::is_none"
    )]
    managing_organization: Option<Reference>,
}

// ── FHIR Datatype Helpers ─────────────────────────────────────────────────

#[derive(Serialize)]
struct Meta {
    profile: Vec<String>,
}

#[derive(Serialize)]
struct HumanName {
    family: String,
    #[serde(rename = "given")]
    given: Vec<String>,
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    name_use: Option<String>,
}

#[derive(Serialize)]
struct Identifier {
    system: String,
    value: String,
}

#[derive(Serialize)]
struct CodeableConcept {
    coding: Vec<Coding>,
}

#[derive(Serialize)]
struct Coding {
    system: String,
    code: String,
    #[serde(rename = "display", skip_serializing_if = "Option::is_none")]
    display: Option<String>,
}

#[derive(Serialize)]
struct Reference {
    reference: String,
    #[serde(rename = "display", skip_serializing_if = "Option::is_none")]
    display: Option<String>,
}

#[derive(Serialize)]
struct ContactPoint {
    system: String,
    value: String,
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    cp_use: Option<String>,
}

#[derive(Serialize)]
struct Address {
    #[serde(rename = "type")]
    addr_type: String,
    line: Vec<String>,
    city: String,
    state: String,
    #[serde(rename = "postalCode")]
    postal_code: String,
    country: String,
}

#[derive(Serialize)]
struct Position {
    latitude: f64,
    longitude: f64,
}

#[derive(Serialize)]
struct AvailableTime {
    #[serde(rename = "daysOfWeek")]
    days_of_week: Vec<String>,
    #[serde(rename = "availableStartTime")]
    available_start_time: String,
    #[serde(rename = "availableEndTime")]
    available_end_time: String,
}

// ── Generator Functions ────────────────────────────────────────────────────

fn gen_organization(id: &str, rng: &mut impl Rng) -> serde_json::Value {
    let name: String = fake::faker::company::en::CompanyName().fake();
    let npi = format!(
        "{}{}",
        rng.random_range(100..999),
        rng.random_range(1000000..9999999)
    );
    let org_types = [
        "prov", "dept", "team", "govt", "ins", "pay", "edu", "reli", "crs",
    ];
    let org_type = org_types[rng.random_range(0..org_types.len())];
    let locality = random_au_locality(rng);

    let org = FhirOrganization {
        resource_type: "Organization".to_string(),
        id: id.to_string(),
        meta: Meta { profile: vec!["http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Organization".to_string()] },
        name,
        identifier: vec![Identifier {
            system: "http://hl7.org/fhir/sid/us-npi".to_string(),
            value: npi,
        }],
        org_type: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/organization-type".to_string(),
                code: org_type.to_string(),
                display: Some(match org_type {
                    "prov" => "Healthcare Provider",
                    "dept" => "Hospital Department",
                    "team" => "Organizational team",
                    "govt" => "Government",
                    "ins" => "Insurance Company",
                    "pay" => "Payer",
                    "edu" => "Educational Institute",
                    _ => "Religious Institution",
                }.to_string()),
            }],
        }],
        active: true,
        telecom: Some(vec![ContactPoint {
            system: "phone".to_string(),
            value: fake::faker::phone_number::en::PhoneNumber().fake(),
            cp_use: Some("work".to_string()),
        }]),
        address: Some(vec![Address {
            addr_type: "physical".to_string(),
            line: vec![fake::faker::address::en::StreetName().fake()],
            city: locality.city.to_string(),
            state: locality.state.to_string(),
            postal_code: locality.postcode.to_string(),
            country: "AU".to_string(),
        }]),
    };
    serde_json::to_value(org).unwrap()
}

fn gen_practitioner(id: &str, rng: &mut impl Rng) -> serde_json::Value {
    let family: String = fake::faker::name::en::LastName().fake();
    let given: String = fake::faker::name::en::FirstName().fake();
    let npi = format!(
        "{}{}",
        rng.random_range(100..999),
        rng.random_range(1000000..9999999)
    );
    let year: u32 = rng.random_range(1950..=2000);
    let month: u32 = rng.random_range(1..=12);
    let day: u32 = rng.random_range(1..=28);
    let genders = ["male", "female", "other", "unknown"];
    let gender = genders[rng.random_range(0..genders.len())];

    let prac = FhirPractitioner {
        resource_type: "Practitioner".to_string(),
        id: id.to_string(),
        meta: Meta { profile: vec!["http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Practitioner".to_string()] },
        name: vec![HumanName {
            family,
            given: vec![given],
            name_use: Some("official".to_string()),
        }],
        identifier: vec![Identifier {
            system: "http://hl7.org/fhir/sid/us-npi".to_string(),
            value: npi,
        }],
        active: true,
        birth_date: Some(format!("{:04}-{:02}-{:02}", year, month, day)),
        gender: Some(gender.to_string()),
    };
    serde_json::to_value(prac).unwrap()
}

fn gen_practitioner_role(
    id: &str,
    org_ids: &[String],
    prac_ids: &[String],
    loc_ids: &[String],
    rng: &mut impl Rng,
) -> serde_json::Value {
    let role_codes = [
        ("doctor", "Doctor"),
        ("nurse", "Nurse"),
        ("pharmacist", "Pharmacist"),
        ("physicaltherapist", "Physical Therapist"),
        ("socialworker", "Social Worker"),
        ("psychologist", "Psychologist"),
        ("dietitian", "Dietitian"),
        ("optometrist", "Optometrist"),
    ];
    let specialties = [
        ("394577000", "Anesthesiology"),
        ("394583001", "Dermatology"),
        ("394579002", "Cardiology"),
        ("394582007", "Dermatology"),
        ("408467006", "Emergency medicine"),
        ("394597006", "Oncology"),
        ("394580004", "General practice"),
        ("394609004", "Orthopaedics"),
        ("394610002", "Otolaryngology"),
        ("394612008", "Paediatrics"),
        ("394600006", "Neurology"),
        ("394585009", "Psychiatry"),
        ("394591006", "Ophthalmology"),
        ("394584008", "Respiratory"),
    ];
    let spec = specialties[rng.random_range(0..specialties.len())];
    let role = role_codes[rng.random_range(0..role_codes.len())];

    let n_specialties = rng.random_range(1..3);
    let mut spec_list = vec![CodeableConcept {
        coding: vec![Coding {
            system: "http://snomed.info/sct".to_string(),
            code: spec.0.to_string(),
            display: Some(spec.1.to_string()),
        }],
    }];
    for _ in 1..n_specialties {
        let s = specialties[rng.random_range(0..specialties.len())];
        spec_list.push(CodeableConcept {
            coding: vec![Coding {
                system: "http://snomed.info/sct".to_string(),
                code: s.0.to_string(),
                display: Some(s.1.to_string()),
            }],
        });
    }

    let days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let n_days = rng.random_range(2..6);
    let start_day = rng.random_range(0..days.len() - n_days + 1);
    let work_days: Vec<String> = days[start_day..start_day + n_days]
        .iter()
        .map(|d| d.to_string())
        .collect();

    let mut locations = Vec::new();
    if !loc_ids.is_empty() {
        let loc_ref = random_ref("Location", loc_ids, rng);
        locations.push(Reference {
            reference: loc_ref,
            display: None,
        });
    }

    let role = FhirPractitionerRole {
        resource_type: "PractitionerRole".to_string(),
        id: id.to_string(),
        meta: Meta { profile: vec!["http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-PractitionerRole".to_string()] },
        active: true,
        practitioner: Reference {
            reference: random_ref("Practitioner", prac_ids, rng),
            display: None,
        },
        organization: Reference {
            reference: random_ref("Organization", org_ids, rng),
            display: None,
        },
        code: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/v3-RoleCode".to_string(),
                code: role.0.to_string(),
                display: Some(role.1.to_string()),
            }],
        }],
        specialty: spec_list,
        location: locations,
        telecom: vec![ContactPoint {
            system: "phone".to_string(),
            value: fake::faker::phone_number::en::PhoneNumber().fake(),
            cp_use: Some("work".to_string()),
        }],
        available_time: vec![AvailableTime {
            days_of_week: work_days,
            available_start_time: "08:00:00".to_string(),
            available_end_time: "17:00:00".to_string(),
        }],
    };
    serde_json::to_value(role).unwrap()
}

fn gen_location(id: &str, rng: &mut impl Rng) -> serde_json::Value {
    let loc_types = [
        ("si", "Site"),
        ("bu", "Building"),
        ("wi", "Wing"),
        ("wa", "Ward"),
        ("lvl", "Level"),
        ("co", "Corner"),
    ];
    let phys_types = [
        ("si", "Site"),
        ("bu", "Building"),
        ("wi", "Wing"),
        ("wa", "Ward"),
        ("lvl", "Level"),
        ("co", "Corner"),
        ("ho", "House"),
        ("ca", "Room"),
        ("ve", "Vehicle"),
        ("ho", "House"),
    ];
    let loc_type = loc_types[rng.random_range(0..loc_types.len())];
    let phys_type = phys_types[rng.random_range(0..phys_types.len())];

    // Spread locations around Australian localities for realistic near searches
    let locality = random_au_locality(rng);
    let lat = locality.lat + (rng.random_range(-50..50) as f64 / 1000.0);
    let lon = locality.lon + (rng.random_range(-50..50) as f64 / 1000.0);

    let statuses = ["active", "suspended", "inactive"];
    let status = statuses[rng.random_range(0..2)]; // weight toward active

    let loc = FhirLocation {
        resource_type: "Location".to_string(),
        id: id.to_string(),
        meta: Meta {
            profile: vec![
                "http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Location"
                    .to_string(),
            ],
        },
        status: status.to_string(),
        name: format!(
            "{} Clinic - {}",
            fake::faker::name::en::LastName().fake::<String>(),
            locality.suburb
        ),
        loc_type: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/v3-RoleCode".to_string(),
                code: loc_type.0.to_string(),
                display: Some(loc_type.1.to_string()),
            }],
        }],
        physical_type: Some(CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/location-physical-type".to_string(),
                code: phys_type.0.to_string(),
                display: Some(phys_type.1.to_string()),
            }],
        }),
        position: Position {
            latitude: lat,
            longitude: lon,
        },
        address: Some(Address {
            addr_type: "physical".to_string(),
            line: vec![format!(
                "{} {} St",
                rng.random_range(100..9999),
                fake::faker::name::en::LastName().fake::<String>()
            )],
            city: locality.city.to_string(),
            state: locality.state.to_string(),
            postal_code: locality.postcode.to_string(),
            country: "AU".to_string(),
        }),
        managing_organization: None, // filled in bulk loader if org IDs available
    };
    serde_json::to_value(loc).unwrap()
}

fn gen_healthcare_service(
    id: &str,
    org_ids: &[String],
    loc_ids: &[String],
    rng: &mut impl Rng,
) -> serde_json::Value {
    let svc_types = [
        ("1", "Emergency department"),
        ("2", "Hospital clinic"),
        ("3", "Hospital service"),
        ("4", "Outpatient clinic"),
        ("5", "Specialist clinic"),
        ("6", "Rehabilitation"),
        ("7", "Pharmacy"),
        ("8", "Laboratory"),
        ("9", "Imaging"),
        ("10", "Mental health"),
        ("11", "Dental"),
        ("12", "Home health"),
        ("13", "Hospice"),
        ("14", "Telehealth"),
        ("15", "Urgent care"),
    ];
    let specialties = [
        ("394577000", "Anesthesiology"),
        ("394583001", "Dermatology"),
        ("394579002", "Cardiology"),
        ("394580004", "General practice"),
        ("394597006", "Oncology"),
        ("394600006", "Neurology"),
        ("394609004", "Orthopaedics"),
        ("394612008", "Paediatrics"),
        ("394584008", "Respiratory"),
    ];

    let svc = svc_types[rng.random_range(0..svc_types.len())];
    let spec = specialties[rng.random_range(0..specialties.len())];

    let mut locations = Vec::new();
    if !loc_ids.is_empty() {
        locations.push(Reference {
            reference: random_ref("Location", loc_ids, rng),
            display: None,
        });
    }

    let svc = FhirHealthcareService {
        resource_type: "HealthcareService".to_string(),
        id: id.to_string(),
        meta: Meta { profile: vec!["http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-HealthcareService".to_string()] },
        active: true,
        provided_by: Reference {
            reference: random_ref("Organization", org_ids, rng),
            display: None,
        },
        svc_type: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/service-type".to_string(),
                code: svc.0.to_string(),
                display: Some(svc.1.to_string()),
            }],
        }],
        specialty: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://snomed.info/sct".to_string(),
                code: spec.0.to_string(),
                display: Some(spec.1.to_string()),
            }],
        }],
        location: locations,
        name: format!("{} Service", svc.1),
        comment: Some(format!("Provides {} services", svc.1.to_lowercase())),
    };
    serde_json::to_value(svc).unwrap()
}

fn gen_endpoint(id: &str, org_ids: &[String], rng: &mut impl Rng) -> serde_json::Value {
    let endpoint = FhirEndpoint {
        resource_type: "Endpoint".to_string(),
        id: id.to_string(),
        meta: Meta {
            profile: vec![
                "http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Endpoint"
                    .to_string(),
            ],
        },
        status: "active".to_string(),
        connection_type: Coding {
            system: "http://terminology.hl7.org/CodeSystem/endpoint-connection-type".to_string(),
            code: "hl7-fhir-rest".to_string(),
            display: Some("HL7 FHIR REST".to_string()),
        },
        name: format!(
            "{} FHIR Endpoint",
            fake::faker::company::en::CompanyName().fake::<String>()
        ),
        payload_type: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/endpoint-payload-type".to_string(),
                code: "none".to_string(),
                display: Some("None".to_string()),
            }],
        }],
        address: format!(
            "https://{}.example.org/fhir",
            fake::faker::internet::en::DomainSuffix().fake::<String>()
        ),
        managing_organization: if org_ids.is_empty() {
            None
        } else {
            Some(Reference {
                reference: random_ref("Organization", org_ids, rng),
                display: None,
            })
        },
    };
    serde_json::to_value(endpoint).unwrap()
}

/// Generic fallback generator for resource types not explicitly handled.
/// Produces a minimal resource with `resourceType`, `id`, `meta`, and `status`.
fn gen_generic(resource_type: &str, id: &str, _rng: &mut impl Rng) -> serde_json::Value {
    let mut resource = serde_json::json!({
        "resourceType": resource_type,
        "id": id,
        "meta": {
            "profile": [format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type)]
        },
        "status": "active"
    });
    // Add a name field if the resource type typically has one
    if matches!(
        resource_type,
        "Patient" | "Person" | "Group" | "List" | "Library"
    ) {
        resource["name"] = serde_json::json!(fake::faker::name::en::Name().fake::<String>());
    }
    resource
}

/// Stamp a random `meta.lastUpdated` date on a resource, within the last 12 months.
fn stamp_created_date(resource: &mut serde_json::Value, rng: &mut impl Rng) {
    let now = Utc::now();
    let days_ago = rng.random_range(0..365);
    let created = now - Duration::days(days_ago);
    let date_str = created.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    resource["meta"]["lastUpdated"] = serde_json::Value::String(date_str);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_creates_ndjson_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 10);
        counts.insert("Practitioner".to_string(), 50);
        counts.insert("PractitionerRole".to_string(), 100);
        counts.insert("Location".to_string(), 20);
        counts.insert("HealthcareService".to_string(), 50);

        let profile_urls = HashMap::new();
        let ids = generate_bulk_data(
            &counts,
            &profile_urls,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        // Each type should have the right number of IDs
        assert_eq!(ids.get("Organization").unwrap().len(), 10);
        assert_eq!(ids.get("Practitioner").unwrap().len(), 50);
        assert_eq!(ids.get("PractitionerRole").unwrap().len(), 100);
        assert_eq!(ids.get("Location").unwrap().len(), 20);
        assert_eq!(ids.get("HealthcareService").unwrap().len(), 50);

        // NDJSON files should exist and have the right line counts
        for (resource_type, count) in &counts {
            let path = dir
                .path()
                .join("data")
                .join(format!("{}.ndjson", resource_type));
            assert!(path.exists(), "{}.ndjson should exist", resource_type);
            let contents = std::fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
            assert_eq!(
                lines.len(),
                *count as usize,
                "{} should have {} lines",
                resource_type,
                count
            );

            // Each line should be valid JSON
            for line in &lines {
                let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(parsed["resourceType"], *resource_type);
                assert!(!parsed["id"].as_str().unwrap().is_empty());
            }
        }
    }

    #[test]
    fn cross_references_are_valid() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 5);
        counts.insert("Practitioner".to_string(), 10);
        counts.insert("PractitionerRole".to_string(), 20);
        counts.insert("Location".to_string(), 5);
        counts.insert("HealthcareService".to_string(), 10);

        let profile_urls = HashMap::new();
        let ids = generate_bulk_data(
            &counts,
            &profile_urls,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        // Check PractitionerRole references
        let pr_path = dir.path().join("data/PractitionerRole.ndjson");
        let pr_contents = std::fs::read_to_string(&pr_path).unwrap();
        let org_ids = ids.get("Organization").unwrap();
        let prac_ids = ids.get("Practitioner").unwrap();

        for line in pr_contents.lines().filter(|l| !l.is_empty()) {
            let pr: serde_json::Value = serde_json::from_str(line).unwrap();
            let prac_ref = pr["practitioner"]["reference"].as_str().unwrap();
            assert!(prac_ref.starts_with("Practitioner/"));
            let prac_id = prac_ref.strip_prefix("Practitioner/").unwrap();
            assert!(
                prac_ids.contains(&prac_id.to_string()),
                "Practitioner reference {} should exist",
                prac_id
            );

            let org_ref = pr["organization"]["reference"].as_str().unwrap();
            assert!(org_ref.starts_with("Organization/"));
            let org_id = org_ref.strip_prefix("Organization/").unwrap();
            assert!(
                org_ids.contains(&org_id.to_string()),
                "Organization reference {} should exist",
                org_id
            );
        }

        // Check HealthcareService references
        let hs_path = dir.path().join("data/HealthcareService.ndjson");
        let hs_contents = std::fs::read_to_string(&hs_path).unwrap();
        for line in hs_contents.lines().filter(|l| !l.is_empty()) {
            let hs: serde_json::Value = serde_json::from_str(line).unwrap();
            let org_ref = hs["providedBy"]["reference"].as_str().unwrap();
            assert!(org_ref.starts_with("Organization/"));
        }
    }

    #[test]
    fn location_has_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Location".to_string(), 100);

        generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        let loc_path = dir.path().join("data/Location.ndjson");
        let contents = std::fs::read_to_string(&loc_path).unwrap();
        for line in contents.lines().filter(|l| !l.is_empty()) {
            let loc: serde_json::Value = serde_json::from_str(line).unwrap();
            let lat = loc["position"]["latitude"].as_f64().unwrap();
            let lon = loc["position"]["longitude"].as_f64().unwrap();
            // Generated localities are AU-based.
            assert!(
                (-45.0..=-9.0).contains(&lat),
                "Latitude {} should be in AU range",
                lat
            );
            assert!(
                (110.0..=156.0).contains(&lon),
                "Longitude {} should be in AU range",
                lon
            );
        }
    }

    #[test]
    fn creation_order_respects_dependencies() {
        let mut counts = HashMap::new();
        counts.insert("PractitionerRole".to_string(), 10);
        counts.insert("Organization".to_string(), 5);
        counts.insert("Endpoint".to_string(), 5);
        counts.insert("Location".to_string(), 5);

        let order = bulk_data_creation_order(&counts);

        // Organization, Endpoint, and Location should come before PractitionerRole.
        let org_idx = order.iter().position(|t| t == "Organization").unwrap();
        let endpoint_idx = order.iter().position(|t| t == "Endpoint").unwrap();
        let loc_idx = order.iter().position(|t| t == "Location").unwrap();
        let pr_idx = order.iter().position(|t| t == "PractitionerRole").unwrap();
        assert!(
            org_idx < pr_idx,
            "Organization should come before PractitionerRole"
        );
        assert!(
            endpoint_idx < loc_idx,
            "Endpoint should come before Location"
        );
        assert!(
            loc_idx < pr_idx,
            "Location should come before PractitionerRole"
        );
    }

    #[test]
    fn generic_fallback_works() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Patient".to_string(), 5);

        let ids = generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();
        assert_eq!(ids.get("Patient").unwrap().len(), 5);

        let path = dir.path().join("data/Patient.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();
        let first_line = contents.lines().next().unwrap();
        let patient: serde_json::Value = serde_json::from_str(first_line).unwrap();
        assert_eq!(patient["resourceType"], "Patient");
        assert_eq!(patient["status"], "active");
    }

    #[test]
    fn profile_urls_override_meta_profile() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 3);

        let mut profile_urls = HashMap::new();
        profile_urls.insert(
            "Organization".to_string(),
            "http://example.org/fhir/StructureDefinition/MyOrg".to_string(),
        );

        let ids = generate_bulk_data(
            &counts,
            &profile_urls,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();
        assert_eq!(ids.get("Organization").unwrap().len(), 3);

        let path = dir.path().join("data/Organization.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();
        for line in contents.lines().filter(|l| !l.is_empty()) {
            let org: serde_json::Value = serde_json::from_str(line).unwrap();
            let profiles = org["meta"]["profile"].as_array().unwrap();
            assert_eq!(
                profiles[0].as_str().unwrap(),
                "http://example.org/fhir/StructureDefinition/MyOrg",
                "meta.profile should use the IG profile URL, not the hardcoded Plan-Net URL"
            );
        }
    }

    #[test]
    fn profile_urls_fallback_to_base_fhir() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 2);

        // No profile_urls provided — should fall back to base FHIR profile
        let profile_urls = HashMap::new();
        let ids = generate_bulk_data(
            &counts,
            &profile_urls,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();
        assert_eq!(ids.get("Organization").unwrap().len(), 2);

        let path = dir.path().join("data/Organization.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();
        for line in contents.lines().filter(|l| !l.is_empty()) {
            let org: serde_json::Value = serde_json::from_str(line).unwrap();
            let profiles = org["meta"]["profile"].as_array().unwrap();
            assert_eq!(
                profiles[0].as_str().unwrap(),
                "http://hl7.org/fhir/StructureDefinition/Organization",
                "meta.profile should fall back to base FHIR profile when no IG profile is provided"
            );
        }
    }

    #[test]
    fn profile_aware_generation_uses_structure_definition() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Patient".to_string(), 2);

        // Create a minimal StructureDefinition for Patient
        let profile = crate::model::profile::StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/fhir/StructureDefinition/MyPatient".to_string(),
            name: "MyPatient".to_string(),
            base_type: "Patient".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            snapshot: None,
            differential: None,
            base_definition: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
        };

        let ids = generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[profile],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();
        assert_eq!(ids.get("Patient").unwrap().len(), 2);

        // When a StructureDefinition is provided, resources should be generated
        // via generate_resource (profile-aware) rather than gen_generic.
        // The profile URL in meta.profile should match the StructureDefinition.
        let path = dir.path().join("data/Patient.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();
        for line in contents.lines().filter(|l| !l.is_empty()) {
            let patient: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(patient["resourceType"], "Patient");
            let profiles = patient["meta"]["profile"].as_array().unwrap();
            assert_eq!(
                profiles[0].as_str().unwrap(),
                "http://example.org/fhir/StructureDefinition/MyPatient",
                "Profile-aware generation should use the StructureDefinition URL"
            );
        }
    }

    #[test]
    fn provenance_overlay_uses_existing_ids() {
        let mut provenance = serde_json::json!({
            "resourceType": "Provenance",
            "id": "provenance-1",
            "target": [{ "reference": "Organization/random-uuid" }],
            "agent": [{ "who": { "reference": "Organization/random-uuid" } }],
            "entity": [{ "role": "source", "what": { "reference": "Resource/random-uuid" } }]
        });

        let org_ids = vec!["organization-1".to_string(), "organization-2".to_string()];
        let prac_ids = vec!["practitioner-1".to_string()];
        let mut rng = rand::rng();

        overlay_cross_references(
            &mut provenance,
            "Provenance",
            "provenance-1",
            &org_ids,
            &prac_ids,
            &[],
            &[],
            &[],
            &[],
            &mut rng,
        );

        let target_ref = provenance["target"][0]["reference"].as_str().unwrap();
        let agent_ref = provenance["agent"][0]["who"]["reference"].as_str().unwrap();
        let entity_ref = provenance["entity"][0]["what"]["reference"]
            .as_str()
            .unwrap();

        assert!(
            [
                "Organization/organization-1",
                "Organization/organization-2",
                "Practitioner/practitioner-1",
            ]
            .contains(&target_ref),
            "target should reference an existing resource ID"
        );
        assert!(
            agent_ref == "Organization/organization-1"
                || agent_ref == "Organization/organization-2",
            "agent.who should reference an existing Organization ID"
        );
        assert!(
            [
                "Organization/organization-1",
                "Organization/organization-2",
                "Practitioner/practitioner-1",
            ]
            .contains(&entity_ref),
            "entity.what should reference an existing resource ID"
        );
    }

    #[test]
    fn overlay_adds_endpoint_links_for_include_tests() {
        let mut location = serde_json::json!({ "resourceType": "Location", "id": "location-1" });
        let mut healthcare_service =
            serde_json::json!({ "resourceType": "HealthcareService", "id": "healthcareservice-1" });
        let mut practitioner_role =
            serde_json::json!({ "resourceType": "PractitionerRole", "id": "practitionerrole-1" });

        let org_ids = vec!["organization-1".to_string()];
        let prac_ids = vec!["practitioner-1".to_string()];
        let loc_ids = vec!["location-1".to_string()];
        let hs_ids = vec!["healthcareservice-1".to_string()];
        let endpoint_ids = vec!["endpoint-1".to_string()];
        let mut rng = rand::rng();

        overlay_cross_references(
            &mut location,
            "Location",
            "location-1",
            &org_ids,
            &prac_ids,
            &loc_ids,
            &hs_ids,
            &[],
            &endpoint_ids,
            &mut rng,
        );
        overlay_cross_references(
            &mut healthcare_service,
            "HealthcareService",
            "healthcareservice-1",
            &org_ids,
            &prac_ids,
            &loc_ids,
            &hs_ids,
            &[],
            &endpoint_ids,
            &mut rng,
        );
        overlay_cross_references(
            &mut practitioner_role,
            "PractitionerRole",
            "practitionerrole-1",
            &org_ids,
            &prac_ids,
            &loc_ids,
            &hs_ids,
            &[],
            &endpoint_ids,
            &mut rng,
        );

        assert_eq!(
            location["endpoint"][0]["reference"].as_str().unwrap(),
            "Endpoint/endpoint-1"
        );
        assert_eq!(
            healthcare_service["endpoint"][0]["reference"]
                .as_str()
                .unwrap(),
            "Endpoint/endpoint-1"
        );
        assert_eq!(
            practitioner_role["endpoint"][0]["reference"]
                .as_str()
                .unwrap(),
            "Endpoint/endpoint-1"
        );
    }

    #[test]
    fn location_one_links_to_organization_one_when_present() {
        let mut location = serde_json::json!({ "resourceType": "Location", "id": "location-1" });
        let org_ids = vec!["organization-1".to_string(), "organization-2".to_string()];
        let mut rng = rand::rng();

        overlay_cross_references(
            &mut location,
            "Location",
            "location-1",
            &org_ids,
            &[],
            &[],
            &[],
            &[],
            &[],
            &mut rng,
        );

        assert_eq!(
            location["managingOrganization"]["reference"]
                .as_str()
                .unwrap(),
            "Organization/organization-1"
        );
    }

    #[test]
    fn provenance_overlay_seeds_id_one_targets_for_revinclude_coverage() {
        let org_ids = vec!["organization-1".to_string()];
        let prac_ids = vec!["practitioner-1".to_string()];
        let loc_ids = vec!["location-1".to_string()];
        let hs_ids = vec!["healthcareservice-1".to_string()];
        let practitioner_role_ids = vec!["practitionerrole-1".to_string()];
        let mut rng = rand::rng();

        let mut p1 = serde_json::json!({ "resourceType": "Provenance", "id": "provenance-1" });
        let mut p2 = serde_json::json!({ "resourceType": "Provenance", "id": "provenance-2" });
        let mut p3 = serde_json::json!({ "resourceType": "Provenance", "id": "provenance-3" });
        let mut p4 = serde_json::json!({ "resourceType": "Provenance", "id": "provenance-4" });
        let mut p5 = serde_json::json!({ "resourceType": "Provenance", "id": "provenance-5" });

        for p in [&mut p1, &mut p2, &mut p3, &mut p4, &mut p5] {
            let id = p["id"].as_str().unwrap().to_string();
            overlay_cross_references(
                p,
                "Provenance",
                &id,
                &org_ids,
                &prac_ids,
                &loc_ids,
                &hs_ids,
                &practitioner_role_ids,
                &[],
                &mut rng,
            );
        }

        assert_eq!(
            p1["target"][0]["reference"].as_str().unwrap(),
            "Organization/organization-1"
        );
        assert_eq!(
            p2["target"][0]["reference"].as_str().unwrap(),
            "Practitioner/practitioner-1"
        );
        assert_eq!(
            p3["target"][0]["reference"].as_str().unwrap(),
            "Location/location-1"
        );
        assert_eq!(
            p4["target"][0]["reference"].as_str().unwrap(),
            "HealthcareService/healthcareservice-1"
        );
        assert_eq!(
            p5["target"][0]["reference"].as_str().unwrap(),
            "PractitionerRole/practitionerrole-1"
        );
    }

    // ── Tests for generate_supplement_resource ─────────────────────────────

    #[test]
    fn supplement_resource_creates_valid_fhir_json() {
        let resource = generate_supplement_resource(
            "Organization",
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(resource["resourceType"], "Organization");
        assert_eq!(resource["id"], "organization-1");
        assert!(resource["meta"]["profile"].as_array().is_some());
        assert!(resource["meta"]["lastUpdated"].as_str().is_some());
    }

    #[test]
    fn supplement_resource_uses_profile_url_when_provided() {
        let mut profile_urls = HashMap::new();
        profile_urls.insert(
            "Organization".to_string(),
            "http://example.org/fhir/StructureDefinition/MyOrg".to_string(),
        );

        let resource = generate_supplement_resource(
            "Organization",
            &profile_urls,
            &[],
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        let profiles = resource["meta"]["profile"].as_array().unwrap();
        assert_eq!(
            profiles[0].as_str().unwrap(),
            "http://example.org/fhir/StructureDefinition/MyOrg"
        );
    }

    #[test]
    fn supplement_resource_normalizes_references() {
        let resource = generate_supplement_resource(
            "PractitionerRole",
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        // All references should use the {type}-1 pattern
        let practitioner_ref = resource["practitioner"]["reference"].as_str().unwrap();
        assert_eq!(practitioner_ref, "Practitioner/practitioner-1");

        let organization_ref = resource["organization"]["reference"].as_str().unwrap();
        assert_eq!(organization_ref, "Organization/organization-1");
    }

    #[test]
    fn supplement_resource_handles_unknown_type() {
        let resource = generate_supplement_resource(
            "UnknownType",
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(resource["resourceType"], "UnknownType");
        assert_eq!(resource["id"], "unknowntype-1");
        assert_eq!(resource["status"], "active");
    }

    // ── Tests for write_supplement_ndjson ──────────────────────────────────

    #[test]
    fn write_supplement_creates_files_for_uncovered_types() {
        let dir = tempfile::tempdir().unwrap();

        // Create bulk data for Organization only
        let mut bulk_counts = HashMap::new();
        bulk_counts.insert("Organization".to_string(), 5);

        // Creation order includes types not in bulk_counts
        let creation_order = bulk_data_creation_order(&bulk_counts);

        let supplement_ids = write_supplement_ndjson(
            &creation_order,
            &bulk_counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        // Supplement IDs should only include types not in bulk_counts
        // (and not in NON_RESOURCE_TYPES)
        assert!(
            !supplement_ids.contains_key("Organization"),
            "Organization has bulk count, should not be in supplement"
        );

        // Each supplement type should have its own NDJSON file
        for (resource_type, ids) in &supplement_ids {
            let path = dir
                .path()
                .join("data")
                .join(format!("{}.ndjson", resource_type));
            assert!(path.exists(), "{}.ndjson should exist", resource_type);
            let contents = std::fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
            assert_eq!(lines.len(), 1, "{} should have 1 line", resource_type);
            assert_eq!(ids.len(), 1, "{} should have 1 ID", resource_type);

            let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
            assert_eq!(parsed["resourceType"], *resource_type);
            assert_eq!(parsed["id"], format!("{}-1", resource_type.to_lowercase()));
        }
    }

    #[test]
    fn write_supplement_skips_non_resource_types() {
        let dir = tempfile::tempdir().unwrap();

        let mut bulk_counts = HashMap::new();
        bulk_counts.insert("Organization".to_string(), 5);

        // Include a non-resource type in the creation order
        let mut creation_order = bulk_data_creation_order(&bulk_counts);
        creation_order.push("Extension".to_string());

        let supplement_ids = write_supplement_ndjson(
            &creation_order,
            &bulk_counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        // Extension should be skipped
        assert!(
            !supplement_ids.contains_key("Extension"),
            "Extension is a non-resource type and should be skipped"
        );
    }

    #[test]
    fn write_supplement_appends_to_combined_ndjson() {
        let dir = tempfile::tempdir().unwrap();

        // First write bulk data for Organization only
        let mut bulk_counts = HashMap::new();
        bulk_counts.insert("Organization".to_string(), 2);

        // Add a type that has no bulk count so it becomes a supplement
        let mut creation_order = bulk_data_creation_order(&bulk_counts);
        creation_order.push("Patient".to_string());

        generate_bulk_data(
            &bulk_counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        // Then write supplements for uncovered types
        write_supplement_ndjson(
            &creation_order,
            &bulk_counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        // combined.ndjson should have bulk + supplement resources
        let combined_path = dir.path().join("data/combined.ndjson");
        let contents = std::fs::read_to_string(&combined_path).unwrap();
        let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();

        // At least 2 bulk lines + supplement lines
        assert!(
            lines.len() > 2,
            "combined.ndjson should have bulk + supplement resources"
        );
    }

    // ── Tests for generate_update_ndjson ───────────────────────────────────

    #[test]
    fn update_ndjson_creates_file_with_same_count() {
        let dir = tempfile::tempdir().unwrap();

        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 5);
        counts.insert("Practitioner".to_string(), 10);

        let ids = generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        generate_update_ndjson(&ids, dir.path()).unwrap();

        let update_path = dir.path().join("data/update.ndjson");
        assert!(update_path.exists(), "update.ndjson should exist");

        let contents = std::fs::read_to_string(&update_path).unwrap();
        let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();

        // Should have same number of resources as bulk data
        let total_bulk: usize = counts.values().sum::<u64>() as usize;
        assert_eq!(
            lines.len(),
            total_bulk,
            "update.ndjson should have {} lines",
            total_bulk
        );
    }

    #[test]
    fn update_ndjson_resources_differ_from_originals() {
        let dir = tempfile::tempdir().unwrap();

        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 3);

        let ids = generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        // Read original resources
        let orig_path = dir.path().join("data/Organization.ndjson");
        let orig_contents = std::fs::read_to_string(&orig_path).unwrap();
        let orig_lines: Vec<&str> = orig_contents.lines().filter(|l| !l.is_empty()).collect();

        generate_update_ndjson(&ids, dir.path()).unwrap();

        // Read updated resources
        let update_path = dir.path().join("data/update.ndjson");
        let update_contents = std::fs::read_to_string(&update_path).unwrap();
        let update_lines: Vec<&str> = update_contents.lines().filter(|l| !l.is_empty()).collect();

        assert_eq!(orig_lines.len(), update_lines.len());

        // Each updated resource should have the same id but different content
        for (orig_line, update_line) in orig_lines.iter().zip(update_lines.iter()) {
            let orig: serde_json::Value = serde_json::from_str(orig_line).unwrap();
            let updated: serde_json::Value = serde_json::from_str(update_line).unwrap();

            // Same id
            assert_eq!(orig["id"], updated["id"]);

            // Different content (at least one field should have changed)
            assert_ne!(
                orig, updated,
                "Updated resource should differ from original"
            );
        }
    }

    #[test]
    fn update_ndjson_preserves_resource_type_and_id() {
        let dir = tempfile::tempdir().unwrap();

        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 2);
        counts.insert("Practitioner".to_string(), 2);

        let ids = generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        generate_update_ndjson(&ids, dir.path()).unwrap();

        let update_path = dir.path().join("data/update.ndjson");
        let contents = std::fs::read_to_string(&update_path).unwrap();

        for line in contents.lines().filter(|l| !l.is_empty()) {
            let resource: serde_json::Value = serde_json::from_str(line).unwrap();
            let rtype = resource["resourceType"].as_str().unwrap();
            let id = resource["id"].as_str().unwrap();

            // resourceType and id should be preserved
            assert!(!rtype.is_empty());
            assert!(!id.is_empty());

            // id should match the pattern {type}-{n}
            assert!(id.starts_with(&rtype.to_lowercase()));
        }
    }

    #[test]
    fn update_ndjson_handles_empty_ids() {
        let dir = tempfile::tempdir().unwrap();
        let ids = IdStore::new();

        // Create the data directory so the function can write to it
        std::fs::create_dir_all(dir.path().join("data")).unwrap();

        // Should not error when there are no IDs
        generate_update_ndjson(&ids, dir.path()).unwrap();

        let update_path = dir.path().join("data/update.ndjson");
        assert!(update_path.exists(), "update.ndjson should exist");
        let contents = std::fs::read_to_string(&update_path).unwrap();
        assert!(contents.trim().is_empty(), "update.ndjson should be empty");
    }

    // ── Additional generate_bulk_data tests ────────────────────────────────

    #[test]
    fn bulk_data_handles_empty_counts() {
        let dir = tempfile::tempdir().unwrap();
        let counts = HashMap::new();

        let ids = generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        assert!(
            ids.is_empty(),
            "No resources should be generated for empty counts"
        );
    }

    #[test]
    fn bulk_data_creates_combined_ndjson() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 3);
        counts.insert("Practitioner".to_string(), 2);

        generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        let combined_path = dir.path().join("data/combined.ndjson");
        assert!(combined_path.exists(), "combined.ndjson should exist");

        let contents = std::fs::read_to_string(&combined_path).unwrap();
        let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
        let total: usize = counts.values().sum::<u64>() as usize;
        assert_eq!(
            lines.len(),
            total,
            "combined.ndjson should have all resources"
        );
    }

    #[test]
    fn bulk_data_stamps_created_date() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 5);

        generate_bulk_data(
            &counts,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
            dir.path(),
        )
        .unwrap();

        let path = dir.path().join("data/Organization.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();

        for line in contents.lines().filter(|l| !l.is_empty()) {
            let org: serde_json::Value = serde_json::from_str(line).unwrap();
            let last_updated = org["meta"]["lastUpdated"].as_str().unwrap();
            assert!(!last_updated.is_empty(), "meta.lastUpdated should be set");
            // Should be an ISO timestamp
            assert!(
                last_updated.contains('T'),
                "meta.lastUpdated should be an ISO timestamp, got: {}",
                last_updated
            );
        }
    }

    #[test]
    fn bulk_data_creation_order_includes_all_types() {
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 5);
        counts.insert("Practitioner".to_string(), 5);
        counts.insert("Endpoint".to_string(), 5);
        counts.insert("Location".to_string(), 5);
        counts.insert("HealthcareService".to_string(), 5);
        counts.insert("PractitionerRole".to_string(), 5);
        counts.insert("Provenance".to_string(), 5);
        counts.insert("Patient".to_string(), 5);

        let order = bulk_data_creation_order(&counts);

        // All types should be in the order
        for t in counts.keys() {
            assert!(order.contains(t), "{} should be in creation order", t);
        }

        // Order should respect dependencies
        let org_idx = order.iter().position(|t| t == "Organization").unwrap();
        let prac_idx = order.iter().position(|t| t == "Practitioner").unwrap();
        let endpoint_idx = order.iter().position(|t| t == "Endpoint").unwrap();
        let loc_idx = order.iter().position(|t| t == "Location").unwrap();
        let hs_idx = order.iter().position(|t| t == "HealthcareService").unwrap();
        let pr_idx = order.iter().position(|t| t == "PractitionerRole").unwrap();
        let prov_idx = order.iter().position(|t| t == "Provenance").unwrap();

        assert!(org_idx < endpoint_idx, "Organization before Endpoint");
        assert!(endpoint_idx < loc_idx, "Endpoint before Location");
        assert!(loc_idx < hs_idx, "Location before HealthcareService");
        assert!(hs_idx < pr_idx, "HealthcareService before PractitionerRole");
        assert!(prac_idx < pr_idx, "Practitioner before PractitionerRole");
        assert!(pr_idx < prov_idx, "PractitionerRole before Provenance");
    }
}
