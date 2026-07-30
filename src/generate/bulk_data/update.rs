use anyhow::Result;
use fake::Fake;
use rand::Rng;
use std::io::Write;
use std::path::Path;

use super::IdStore;
use super::bulk_data_creation_order;

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
