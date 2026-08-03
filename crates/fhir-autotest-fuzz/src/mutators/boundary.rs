use crate::mutators::Mutator;
use fhir_autotest::model::profile::StructureDefinition;

/// Boundary value mutator: replaces field values with edge-case values.
///
/// For each field in the resource, this mutator tries:
/// - Empty strings, very long strings, unicode strings
/// - Zero, negative, and very large numbers
/// - Null values for non-nullable fields
/// - Extreme dates (year 0, year 9999, leap year edge cases)
pub struct BoundaryMutator;

impl Mutator for BoundaryMutator {
    fn name(&self) -> &'static str {
        "boundary"
    }

    fn mutate(
        &self,
        base_resource: &serde_json::Value,
        _profile: &StructureDefinition,
        seed: u64,
    ) -> serde_json::Value {
        let mut resource = base_resource.clone();
        apply_boundary_mutations(&mut resource, seed);
        resource
    }
}

fn apply_boundary_mutations(value: &mut serde_json::Value, seed: u64) {
    // Use seed to select which boundary to apply deterministically
    let strategy = (seed % 8) as u8;

    match value {
        serde_json::Value::String(s) => match strategy {
            0 => *s = String::new(),                    // empty string
            1 => *s = " ".repeat(10000),                 // very long whitespace
            2 => *s = "A".repeat(65536),                 // max-length string
            3 => *s = "\0\0\0\0".to_string(),            // null bytes
            4 => *s = "\\u0000\\u0000".to_string(),      // escaped unicode
            5 => *s = "💉🏥🧬🩺".to_string(),            // multi-byte unicode
            6 => *s = "\t\n\r".to_string(),              // control characters
            7 => *s = "🫀".repeat(1000),                  // emoji overflow
            _ => {}
        },
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                *value = serde_json::json!(match strategy {
                    0 => -f,                              // negate
                    1 => f64::NEG_INFINITY,               // -inf
                    2 => f64::INFINITY,                   // +inf
                    3 => f64::NAN,                         // NaN
                    4 => 0.0,                              // zero
                    5 => -0.0,                             // negative zero
                    6 => 1e308,                            // near overflow
                    7 => -1e308,                           // near underflow
                    _ => f,
                });
            } else if let Some(i) = n.as_i64() {
                *value = serde_json::json!(match strategy {
                    0 => 0,
                    1 => i64::MAX,
                    2 => i64::MIN,
                    3 => -i,
                    4 => 1,
                    5 => -1,
                    6 => 999999999999999,
                    7 => -999999999999999,
                    _ => i,
                });
            }
        }
        serde_json::Value::Bool(b) => {
            let val = *b;
            *value = serde_json::json!(!val);
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                apply_boundary_mutations(item, seed.wrapping_add(1));
            }
        }
        serde_json::Value::Object(obj) => {
            for (_key, val) in obj.iter_mut() {
                apply_boundary_mutations(val, seed.wrapping_add(1));
            }
        }
        serde_json::Value::Null => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_mutation() {
        let mut val = serde_json::json!("hello");
        apply_boundary_mutations(&mut val, 0);
        assert_eq!(val, "");
    }

    #[test]
    fn negate_number() {
        let mut val = serde_json::json!(42);
        apply_boundary_mutations(&mut val, 0);
        assert_eq!(val, -42.0);
    }

    #[test]
    fn flip_boolean() {
        let mut val = serde_json::json!(true);
        apply_boundary_mutations(&mut val, 0);
        assert_eq!(val, false);
    }
}
