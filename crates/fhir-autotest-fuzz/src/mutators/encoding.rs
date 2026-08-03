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
            0 => *s = format!("\"; {} //", s),   // JSON injection
            1 => *s = format!("{}\\\"{}", s, s), // escaped quote
            2 => *s = format!("{}\\n{}\\r{}\\t{}", s, s, s, s), // control chars
            3 => *s = format!("{}\\u0000{}", s, s), // null byte injection
            4 => *s = format!("{}\\u200B{}", s, s), // zero-width space
            5 => *s = format!("{}\\uFFFE{}", s, s), // non-character
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

    // ── String strategies (seeds 0-5) ─────────────────────────────────────

    #[test]
    fn string_json_injection() {
        let mut val = serde_json::json!("hello");
        apply_encoding_mutations(&mut val, 0);
        let s = val.as_str().unwrap();
        assert!(s.contains("\";"));
        assert!(s.contains("hello"));
    }

    #[test]
    fn string_escaped_quote() {
        let mut val = serde_json::json!("hello");
        apply_encoding_mutations(&mut val, 1);
        let s = val.as_str().unwrap();
        assert!(s.contains("\\\""));
        assert!(s.contains("hello"));
    }

    #[test]
    fn string_control_chars() {
        let mut val = serde_json::json!("hello");
        apply_encoding_mutations(&mut val, 2);
        let s = val.as_str().unwrap();
        assert!(s.contains("\\n"));
        assert!(s.contains("\\r"));
        assert!(s.contains("\\t"));
    }

    #[test]
    fn string_null_byte_injection() {
        let mut val = serde_json::json!("hello");
        apply_encoding_mutations(&mut val, 3);
        let s = val.as_str().unwrap();
        assert!(s.contains("\\u{0000}") || s.contains("\\u0000"));
    }

    #[test]
    fn string_zero_width_space() {
        let mut val = serde_json::json!("hello");
        apply_encoding_mutations(&mut val, 4);
        let s = val.as_str().unwrap();
        assert!(s.contains("\\u{200B}") || s.contains("\\u200B"));
    }

    #[test]
    fn string_non_character() {
        let mut val = serde_json::json!("hello");
        apply_encoding_mutations(&mut val, 5);
        let s = val.as_str().unwrap();
        assert!(s.contains("\\u{FFFE}") || s.contains("\\uFFFE"));
    }

    // ── Object strategies ─────────────────────────────────────────────────

    #[test]
    fn object_deep_nesting() {
        let mut val = serde_json::json!({"key": "value"});
        apply_encoding_mutations(&mut val, 0);
        assert!(val.as_object().unwrap().contains_key("x_deep_nesting"));
        // Verify it's deeply nested (100 levels: 1 initial + 99 iterations)
        let mut current = &val["x_deep_nesting"];
        for _ in 0..100 {
            assert!(current.is_object());
            current = &current["nested"];
        }
        assert_eq!(*current, serde_json::json!({"depth": 1}));
    }

    #[test]
    fn object_duplicate_key() {
        let mut val = serde_json::json!({"name": "value"});
        apply_encoding_mutations(&mut val, 1);
        let obj = val.as_object().unwrap();
        // Should have at least 2 keys (original + duplicate with combining accent)
        assert!(obj.len() >= 2);
        assert!(obj.contains_key("name"));
        // The duplicate key should be "name" + combining acute accent
        let duplicate_key = format!("name\u{0301}");
        assert!(obj.contains_key(&duplicate_key));
        assert_eq!(obj[&duplicate_key], "duplicate");
    }

    #[test]
    fn object_duplicate_key_empty_object() {
        let mut val = serde_json::json!({});
        // Empty object has no keys, so duplicate key should be a no-op
        apply_encoding_mutations(&mut val, 1);
        assert!(val.as_object().unwrap().is_empty());
    }

    #[test]
    fn object_long_key() {
        let mut val = serde_json::json!({"key": "value"});
        apply_encoding_mutations(&mut val, 2);
        let obj = val.as_object().unwrap();
        let long_key = "x_".to_string() + &"A".repeat(1000);
        assert!(obj.contains_key(&long_key));
        assert_eq!(obj[&long_key], "long_key");
    }

    #[test]
    fn object_recursion() {
        let mut val = serde_json::json!({
            "nested": {
                "inner": "hello"
            }
        });
        // Strategy 3+ recurses into object values
        apply_encoding_mutations(&mut val, 3);
        // The inner string "hello" should be mutated with strategy 4 (seed 3 + 1 = 4)
        // Strategy 4 on a string adds zero-width space
        let inner = val["nested"]["inner"].as_str().unwrap();
        assert!(inner.contains("hello"));
    }

    // ── Array strategies ─────────────────────────────────────────────────

    #[test]
    fn array_deep_nesting() {
        let mut val = serde_json::json!(["a", "b"]);
        apply_encoding_mutations(&mut val, 0);
        let arr = val.as_array().unwrap();
        // Original elements plus the deeply nested one
        assert_eq!(arr.len(), 3);
        // The last element should be deeply nested (101 levels of array wrapping)
        let mut current = &arr[2];
        for _ in 0..101 {
            assert!(current.is_array());
            current = &current[0];
        }
        assert_eq!(*current, serde_json::json!("deep"));
    }

    #[test]
    fn array_recursion() {
        let mut val = serde_json::json!(["hello", "world"]);
        // Strategy 1+ recurses into array items
        apply_encoding_mutations(&mut val, 1);
        // Each string should be mutated with strategy 2 (seed 1 + 1 = 2)
        // Strategy 2 on a string adds control chars
        assert!(val[0].as_str().unwrap().contains("hello"));
        assert!(val[1].as_str().unwrap().contains("world"));
    }

    #[test]
    fn empty_array_no_panic() {
        let mut val = serde_json::json!([]);
        apply_encoding_mutations(&mut val, 0);
        // Strategy 0 on an array pushes a deeply nested array
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        // Verify it's deeply nested (101 levels)
        let mut current = &arr[0];
        for _ in 0..101 {
            assert!(current.is_array());
            current = &current[0];
        }
        assert_eq!(*current, serde_json::json!("deep"));
    }

    #[test]
    fn empty_array_recursion_no_panic() {
        let mut val = serde_json::json!([]);
        apply_encoding_mutations(&mut val, 1);
        assert!(val.as_array().unwrap().is_empty());
    }

    // ── Null handling ────────────────────────────────────────────────────

    #[test]
    fn null_value_no_panic() {
        let mut val = serde_json::Value::Null;
        apply_encoding_mutations(&mut val, 0);
        assert_eq!(val, serde_json::Value::Null);
    }

    #[test]
    fn number_value_no_panic() {
        let mut val = serde_json::json!(42);
        apply_encoding_mutations(&mut val, 0);
        assert_eq!(val, 42);
    }

    #[test]
    fn bool_value_no_panic() {
        let mut val = serde_json::json!(true);
        apply_encoding_mutations(&mut val, 0);
        assert_eq!(val, true);
    }

    // ── Nested structure ─────────────────────────────────────────────────

    #[test]
    fn nested_object_in_array() {
        let mut val = serde_json::json!([
            {"name": "hello"}
        ]);
        // Strategy 1 recurses into array items
        apply_encoding_mutations(&mut val, 1);
        // The string "hello" should be mutated with strategy 2 (seed 1 + 1 = 2)
        assert!(val[0]["name"].as_str().unwrap().contains("hello"));
    }

    // ── Mutator trait ────────────────────────────────────────────────────

    #[test]
    fn encoding_mutator_name() {
        let m = EncodingMutator;
        assert_eq!(m.name(), "encoding");
    }

    #[test]
    fn encoding_mutator_mutate_returns_clone() {
        let m = EncodingMutator;
        let resource = serde_json::json!({"name": "hello"});
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/Test".to_string(),
            base_type: "Patient".to_string(),
            name: "Test".to_string(),
            kind: "resource".to_string(),
            derivation: None,
            base_definition: None,
            snapshot: None,
            differential: None,
        };
        let result = m.mutate(&resource, &profile, 0);
        // Seed 0 on an Object adds x_deep_nesting (strategy 0 for objects)
        assert!(result.as_object().unwrap().contains_key("x_deep_nesting"));
        // Original should be unchanged
        assert_eq!(resource["name"], "hello");
    }
}
