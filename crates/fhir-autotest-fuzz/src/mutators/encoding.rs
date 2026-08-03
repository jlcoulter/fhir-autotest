use crate::mutators::Mutator;
use fhir_autotest::model::profile::StructureDefinition;

/// Encoding attack mutator: injects encoding-level edge cases.
///
/// - JSON injection in string fields (quotes, backslashes, control chars)
/// - Deeply nested JSON (stack overflow attempts)
/// - Duplicate keys in objects
/// - Unicode normalization attacks
/// - Extremely long field names
pub struct EncodingMutator;

impl Mutator for EncodingMutator {
    fn name(&self) -> &'static str {
        "encoding"
    }

    fn mutate(
        &self,
        base_resource: &serde_json::Value,
        _profile: &StructureDefinition,
        seed: u64,
    ) -> serde_json::Value {
        let mut resource = base_resource.clone();
        apply_encoding_mutations(&mut resource, seed);
        resource
    }
}

fn apply_encoding_mutations(value: &mut serde_json::Value, seed: u64) {
    let strategy = (seed % 6) as u8;

    match value {
        serde_json::Value::String(s) => match strategy {
            0 => *s = format!("\"; {} //", s),               // JSON injection
            1 => *s = format!("{}\\\"{}", s, s),             // escaped quote
            2 => *s = format!("{}\n{}\r{}\t{}", s, s, s, s), // control chars
            3 => *s = format!("{}\u{0000}{}", s, s),         // null byte injection
            4 => *s = format!("{}\u{200B}{}", s, s),         // zero-width space
            5 => *s = format!("{}\u{FFFE}{}", s, s),         // non-character
            _ => {}
        },
        serde_json::Value::Object(obj) => {
            if strategy == 0 {
                // Add deeply nested structure
                let mut nested = serde_json::json!({"depth": 1});
                for _ in 0..100 {
                    nested = serde_json::json!({"nested": nested});
                }
                obj.insert("x_deep_nesting".to_string(), nested);
            } else if strategy == 1 {
                // Add duplicate key by inserting a key that looks the same
                // but has different unicode normalization
                if let Some(existing_key) = obj.keys().next().cloned() {
                    let duplicate = format!("{}\u{0301}", existing_key);
                    obj.insert(duplicate, serde_json::json!("duplicate"));
                }
            } else if strategy == 2 {
                // Add very long key
                let long_key = "x_".to_string() + &"A".repeat(1000);
                obj.insert(long_key, serde_json::json!("long_key"));
            } else {
                for (_key, val) in obj.iter_mut() {
                    apply_encoding_mutations(val, seed.wrapping_add(1));
                }
            }
        }
        serde_json::Value::Array(arr) => {
            if strategy == 0 {
                // Add deeply nested array
                let mut nested = serde_json::json!(["deep"]);
                for _ in 0..100 {
                    nested = serde_json::json!([nested]);
                }
                arr.push(nested);
            } else {
                for item in arr.iter_mut() {
                    apply_encoding_mutations(item, seed.wrapping_add(1));
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_injection() {
        let mut val = serde_json::json!("hello");
        apply_encoding_mutations(&mut val, 0);
        assert!(val.as_str().unwrap().contains("\";"));
    }

    #[test]
    fn deep_nesting() {
        let mut val = serde_json::json!({"key": "value"});
        apply_encoding_mutations(&mut val, 0);
        assert!(val.as_object().unwrap().contains_key("x_deep_nesting"));
    }
}
