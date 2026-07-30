use crate::generate::hcpd;
use crate::generate::resource_generator::{
    build_code_system_first_code_map, generate_resource_with_value_sets,
};
use crate::model::profile::StructureDefinition;
use anyhow::Result;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use super::IdStore;
use super::generators::{
    gen_endpoint, gen_generic, gen_healthcare_service, gen_location, gen_organization,
    gen_practitioner, gen_practitioner_role,
};
use super::stamp_created_date;
use crate::generate::NON_RESOURCE_TYPES;

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
    use std::io::BufWriter;

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
        if count > 0 || NON_RESOURCE_TYPES.contains(&resource_type.as_str()) {
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
