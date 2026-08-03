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

    #[test]
    fn string_becomes_null() {
        let mut val = serde_json::json!("hello");
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, serde_json::Value::Null);
    }

    #[test]
    fn number_becomes_string() {
        let mut val = serde_json::json!(42);
        apply_type_mismatches(&mut val, 0);
        assert_eq!(val, "not_a_number");
    }
}
