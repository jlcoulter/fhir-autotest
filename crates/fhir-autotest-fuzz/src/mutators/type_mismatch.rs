use crate::mutators::Mutator;
use fhir_autotest::model::profile::StructureDefinition;

/// Type mismatch mutator: replaces field values with values of the wrong type.
///
/// For each field, this mutator tries:
/// - String where number expected, number where string expected
/// - Array where object expected, object where array expected
/// - Null for required fields
/// - Nested objects where primitives expected
pub struct TypeMismatchMutator;

impl Mutator for TypeMismatchMutator {
    fn name(&self) -> &'static str {
        "type_mismatch"
    }

    fn mutate(
        &self,
        base_resource: &serde_json::Value,
        _profile: &StructureDefinition,
        seed: u64,
    ) -> serde_json::Value {
        let mut resource = base_resource.clone();
        apply_type_mismatches(&mut resource, seed);
        resource
    }
}

fn apply_type_mismatches(value: &mut serde_json::Value, seed: u64) {
    let strategy = (seed % 6) as u8;

    match value {
        serde_json::Value::String(_) => match strategy {
            0 => *value = serde_json::json!(null),
            1 => *value = serde_json::json!(42),
            2 => *value = serde_json::json!(true),
            3 => *value = serde_json::json!([1, 2, 3]),
            4 => *value = serde_json::json!({"value": "wrapped"}),
            5 => *value = serde_json::json!(std::f64::consts::PI),
            _ => {}
        },
        serde_json::Value::Number(_) => match strategy {
            0 => *value = serde_json::json!("not_a_number"),
            1 => *value = serde_json::json!(null),
            2 => *value = serde_json::json!(true),
            3 => *value = serde_json::json!([1]),
            4 => *value = serde_json::json!({"key": "value"}),
            5 => *value = serde_json::json!(""),
            _ => {}
        },
        serde_json::Value::Bool(_) => match strategy {
            0 => *value = serde_json::json!("true"),
            1 => *value = serde_json::json!(null),
            2 => *value = serde_json::json!(1),
            3 => *value = serde_json::json!([true]),
            4 => *value = serde_json::json!({"bool": true}),
            5 => *value = serde_json::json!(0),
            _ => {}
        },
        serde_json::Value::Array(arr) => {
            if strategy == 0 && !arr.is_empty() {
                // Replace array with its first element
                *value = arr[0].clone();
            } else if strategy == 1 {
                *value = serde_json::json!({"array_wrapped": "object"});
            } else {
                for item in arr.iter_mut() {
                    apply_type_mismatches(item, seed.wrapping_add(1));
                }
            }
        }
        serde_json::Value::Object(obj) => {
            if strategy == 0 {
                *value = serde_json::json!(["array_wrapped"]);
            } else {
                for (_key, val) in obj.iter_mut() {
                    apply_type_mismatches(val, seed.wrapping_add(1));
                }
            }
        }
        serde_json::Value::Null => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── String strategies (seeds 0-5) ─────────────────────────────────────

    #[test]
    fn string_becomes_null() {
        let mut val = serde_json::json!("hello");
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, serde_json::Value::Null);
    }

    #[test]
    fn string_becomes_number() {
        let mut val = serde_json::json!("hello");
        apply_type_mismatches(&mut val, 1);
        assert_eq!(val, 42);
    }

    #[test]
    fn string_becomes_bool() {
        let mut val = serde_json::json!("hello");
        apply_type_mismatches(&mut val, 2);
        assert_eq!(val, true);
    }

    #[test]
    fn string_becomes_array() {
        let mut val = serde_json::json!("hello");
        apply_type_mismatches(&mut val, 3);
        assert_eq!(val, serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn string_becomes_object() {
        let mut val = serde_json::json!("hello");
        apply_type_mismatches(&mut val, 4);
        assert_eq!(val, serde_json::json!({"value": "wrapped"}));
    }

    #[test]
    fn string_becomes_pi() {
        let mut val = serde_json::json!("hello");
        apply_type_mismatches(&mut val, 5);
        assert_eq!(val, serde_json::json!(std::f64::consts::PI));
    }

    // ── Number strategies (seeds 0-5) ────────────────────────────────────

    #[test]
    fn number_becomes_string() {
        let mut val = serde_json::json!(42);
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, "not_a_number");
    }

    #[test]
    fn number_becomes_null() {
        let mut val = serde_json::json!(42);
        apply_type_mismatches(&mut val, 1);
        assert_eq!(val, serde_json::Value::Null);
    }

    #[test]
    fn number_becomes_bool() {
        let mut val = serde_json::json!(42);
        apply_type_mismatches(&mut val, 2);
        assert_eq!(val, true);
    }

    #[test]
    fn number_becomes_array() {
        let mut val = serde_json::json!(42);
        apply_type_mismatches(&mut val, 3);
        assert_eq!(val, serde_json::json!([1]));
    }

    #[test]
    fn number_becomes_object() {
        let mut val = serde_json::json!(42);
        apply_type_mismatches(&mut val, 4);
        assert_eq!(val, serde_json::json!({"key": "value"}));
    }

    #[test]
    fn number_becomes_empty_string() {
        let mut val = serde_json::json!(42);
        apply_type_mismatches(&mut val, 5);
        assert_eq!(val, "");
    }

    // ── Bool strategies (seeds 0-5) ──────────────────────────────────────

    #[test]
    fn bool_becomes_string() {
        let mut val = serde_json::json!(true);
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, "true");
    }

    #[test]
    fn bool_becomes_null() {
        let mut val = serde_json::json!(true);
        apply_type_mismatches(&mut val, 1);
        assert_eq!(val, serde_json::Value::Null);
    }

    #[test]
    fn bool_becomes_number() {
        let mut val = serde_json::json!(true);
        apply_type_mismatches(&mut val, 2);
        assert_eq!(val, 1);
    }

    #[test]
    fn bool_becomes_array() {
        let mut val = serde_json::json!(true);
        apply_type_mismatches(&mut val, 3);
        assert_eq!(val, serde_json::json!([true]));
    }

    #[test]
    fn bool_becomes_object() {
        let mut val = serde_json::json!(true);
        apply_type_mismatches(&mut val, 4);
        assert_eq!(val, serde_json::json!({"bool": true}));
    }

    #[test]
    fn bool_becomes_zero() {
        let mut val = serde_json::json!(true);
        apply_type_mismatches(&mut val, 5);
        assert_eq!(val, 0);
    }

    // ── Array strategies ─────────────────────────────────────────────────

    #[test]
    fn array_becomes_first_element() {
        let mut val = serde_json::json!(["hello", "world"]);
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, "hello");
    }

    #[test]
    fn array_becomes_object() {
        let mut val = serde_json::json!(["hello", "world"]);
        apply_type_mismatches(&mut val, 1);
        assert_eq!(val, serde_json::json!({"array_wrapped": "object"}));
    }

    #[test]
    fn array_recursion() {
        let mut val = serde_json::json!(["hello", 42]);
        // Strategy 2+ recurses into array items
        apply_type_mismatches(&mut val, 2);
        // "hello" (string) with strategy 3 (seed 2 + 1 = 3) becomes array [1,2,3]
        assert_eq!(val[0], serde_json::json!([1, 2, 3]));
        // 42 (number) with strategy 3 (seed 2 + 1 = 3) becomes array [1]
        assert_eq!(val[1], serde_json::json!([1]));
    }

    #[test]
    fn empty_array_no_panic() {
        let mut val = serde_json::json!([]);
        apply_type_mismatches(&mut val, 0);
        // Empty array with strategy 0: arr.is_empty() is true, so it falls through
        // to the else branch which recurses (no items, so no-op)
        assert_eq!(val, serde_json::json!([]));
    }

    #[test]
    fn empty_array_strategy_one() {
        let mut val = serde_json::json!([]);
        apply_type_mismatches(&mut val, 1);
        assert_eq!(val, serde_json::json!({"array_wrapped": "object"}));
    }

    // ── Object strategies ────────────────────────────────────────────────

    #[test]
    fn object_becomes_array() {
        let mut val = serde_json::json!({"key": "value"});
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, serde_json::json!(["array_wrapped"]));
    }

    #[test]
    fn object_recursion() {
        let mut val = serde_json::json!({
            "name": "hello",
            "age": 42
        });
        // Strategy 1+ recurses into object values
        apply_type_mismatches(&mut val, 1);
        // "hello" (string) with strategy 2 (seed 1 + 1 = 2) becomes true
        assert_eq!(val["name"], true);
        // 42 (number) with strategy 2 (seed 1 + 1 = 2) becomes true
        assert_eq!(val["age"], true);
    }

    #[test]
    fn empty_object_no_panic() {
        let mut val = serde_json::json!({});
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, serde_json::json!(["array_wrapped"]));
    }

    #[test]
    fn empty_object_recursion_no_panic() {
        let mut val = serde_json::json!({});
        apply_type_mismatches(&mut val, 1);
        assert_eq!(val, serde_json::json!({}));
    }

    // ── Null handling ────────────────────────────────────────────────────

    #[test]
    fn null_value_no_panic() {
        let mut val = serde_json::Value::Null;
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, serde_json::Value::Null);
    }

    // ── Nested structure ─────────────────────────────────────────────────

    #[test]
    fn nested_object_in_array() {
        let mut val = serde_json::json!([
            {"name": "hello"}
        ]);
        // Strategy 2 recurses into array items
        apply_type_mismatches(&mut val, 2);
        // The object at index 0 with strategy 3 (seed 2 + 1 = 3) recurses into values
        // "hello" (string) with strategy 4 (seed 3 + 1 = 4) becomes object
        assert_eq!(val[0]["name"], serde_json::json!({"value": "wrapped"}));
    }

    // ── Mutator trait ────────────────────────────────────────────────────

    #[test]
    fn type_mismatch_mutator_name() {
        let m = TypeMismatchMutator;
        assert_eq!(m.name(), "type_mismatch");
    }

    #[test]
    fn type_mismatch_mutator_mutate_returns_clone() {
        let m = TypeMismatchMutator;
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
        // The "name" string should become null (seed 0, strategy 0)
        assert_eq!(result["name"], serde_json::Value::Null);
        // Original should be unchanged
        assert_eq!(resource["name"], "hello");
    }
}
