use crate::mutators::Mutator;
use fhir_autotest::model::profile::StructureDefinition;

/// Search parameter fuzzer: generates fuzzed query string values for FHIR search params.
///
/// For each search parameter declared in the CapabilityStatement, this produces
/// boundary, encoding, and type-mismatch values appropriate to the param type
/// (string, token, date, number, quantity, uri, reference, composite, special).
pub struct SearchParamMutator;

impl Mutator for SearchParamMutator {
    fn name(&self) -> &'static str {
        "search_param"
    }

    fn mutate(
        &self,
        _base_resource: &serde_json::Value,
        _profile: &StructureDefinition,
        seed: u64,
    ) -> serde_json::Value {
        // Not used for search params — we use generate_fuzzed_params instead.
        let _ = seed;
        _base_resource.clone()
    }
}

/// Generate a fuzzed query parameter value for a given search param type.
///
/// `param_type` is one of: string, token, date, number, quantity, uri, reference, composite, special
/// `seed` selects which fuzz strategy to apply.
pub fn generate_fuzzed_param_value(param_type: &str, seed: u64) -> String {
    let strategy = (seed % 12) as u8;

    match param_type {
        "string" => match strategy {
            0 => String::new(),
            1 => "A".repeat(10000),
            2 => "\0\0\0\0".to_string(),
            3 => "💉🏥🧬🩺".to_string(),
            4 => "\"; {} //".to_string(),
            5 => "\\u0000".to_string(),
            6 => "\t\n\r".to_string(),
            7 => "OR 1=1--".to_string(),
            8 => "<script>alert(1)</script>".to_string(),
            9 => "../../etc/passwd".to_string(),
            10 => "%00%00%00".to_string(),
            11 => "null".to_string(),
            _ => "x".to_string(),
        },
        "token" => match strategy {
            0 => String::new(),
            1 => "|".to_string(),
            2 => "system|code".to_string(),
            3 => "|".to_string(),
            4 => "\0|\0".to_string(),
            5 => "http://example.org|".to_string(),
            6 => "|value with spaces".to_string(),
            7 => "OR 1=1|".to_string(),
            8 => "<script>|</script>".to_string(),
            9 => "../../etc|passwd".to_string(),
            10 => "%00|%00".to_string(),
            11 => "null|null".to_string(),
            _ => "unknown".to_string(),
        },
        "date" => match strategy {
            0 => String::new(),
            1 => "0000-00-00".to_string(),
            2 => "9999-12-31".to_string(),
            3 => "2024-13-01".to_string(),
            4 => "2024-01-32".to_string(),
            5 => "not-a-date".to_string(),
            6 => "2024".to_string(),
            7 => "2024-01".to_string(),
            8 => "2024-01-01T00:00:00Z".to_string(),
            9 => "2024-01-01T00:00:00+00:00".to_string(),
            10 => "gt2024-01-01".to_string(),
            11 => "le2024-01-01".to_string(),
            _ => "2024-01-01".to_string(),
        },
        "number" => match strategy {
            0 => String::new(),
            1 => "0".to_string(),
            2 => "-0".to_string(),
            3 => "999999999999999".to_string(),
            4 => "-999999999999999".to_string(),
            5 => "3.14159265358979".to_string(),
            6 => "not-a-number".to_string(),
            7 => "1e9999".to_string(),
            8 => "NaN".to_string(),
            9 => "Infinity".to_string(),
            10 => "gt100".to_string(),
            11 => "le0.5".to_string(),
            _ => "1".to_string(),
        },
        "quantity" => match strategy {
            0 => String::new(),
            1 => "0".to_string(),
            2 => "1000|http://unitsofmeasure.org|mg".to_string(),
            3 => "||".to_string(),
            4 => "not-a-quantity".to_string(),
            5 => "1e9999||".to_string(),
            6 => "gt100|http://unitsofmeasure.org|mg".to_string(),
            7 => "le0.5||".to_string(),
            8 => "NaN||".to_string(),
            9 => "../../etc|passwd|".to_string(),
            10 => "%00|%00|%00".to_string(),
            11 => "null|null|null".to_string(),
            _ => "1||".to_string(),
        },
        "uri" => match strategy {
            0 => String::new(),
            1 => "http://example.org".to_string(),
            2 => "urn:oid:1.2.3.4.5".to_string(),
            3 => "../../etc/passwd".to_string(),
            4 => "https://evil.com?q=1".to_string(),
            5 => "not-a-uri".to_string(),
            6 => "\0\0\0\0".to_string(),
            7 => "javascript:alert(1)".to_string(),
            8 => "file:///etc/passwd".to_string(),
            9 => "http://[::1]:8080/path".to_string(),
            10 => "%00http://example.org".to_string(),
            11 => "null".to_string(),
            _ => "http://example.org".to_string(),
        },
        "reference" => match strategy {
            0 => String::new(),
            1 => "Patient/nonexistent".to_string(),
            2 => "Patient/".to_string(),
            3 => "/Patient/123".to_string(),
            4 => "../../etc/passwd".to_string(),
            5 => "Patient/../../etc/passwd".to_string(),
            6 => "Patient/0".to_string(),
            7 => "Patient/null".to_string(),
            8 => "Patient/%00".to_string(),
            9 => "Patient/OR 1=1--".to_string(),
            10 => "Patient/<script>".to_string(),
            11 => "Patient/".to_string(),
            _ => "Patient/unknown".to_string(),
        },
        "composite" => match strategy {
            0 => String::new(),
            1 => "$".to_string(),
            2 => "value1$value2".to_string(),
            3 => "$value".to_string(),
            4 => "value$".to_string(),
            5 => "$$".to_string(),
            6 => "\0$\0".to_string(),
            7 => "../../etc$passwd".to_string(),
            8 => "OR 1=1$--".to_string(),
            9 => "%00$%00".to_string(),
            10 => "null$null".to_string(),
            11 => "A".repeat(5000) + "$" + &"B".repeat(5000),
            _ => "a$b".to_string(),
        },
        "special" => match strategy {
            0 => String::new(),
            1 => "near".to_string(),
            2 => "-33.86|151.21|10|km".to_string(),
            3 => "|||".to_string(),
            4 => "not-special".to_string(),
            5 => "near|0|0|0|".to_string(),
            6 => "near|999|999|999|km".to_string(),
            7 => "near|-999|-999|-999|km".to_string(),
            8 => "near|NaN|NaN|NaN|km".to_string(),
            9 => "near|../../etc|passwd|0|km".to_string(),
            10 => "near|%00|%00|%00|km".to_string(),
            11 => "near|null|null|null|km".to_string(),
            _ => "near|0|0|0|km".to_string(),
        },
        _ => match strategy {
            0 => String::new(),
            1 => "A".repeat(1000),
            2 => "\0\0\0\0".to_string(),
            3 => "OR 1=1--".to_string(),
            4 => "<script>alert(1)</script>".to_string(),
            5 => "../../etc/passwd".to_string(),
            6 => "%00%00%00".to_string(),
            7 => "null".to_string(),
            8 => "NaN".to_string(),
            9 => "Infinity".to_string(),
            10 => "undefined".to_string(),
            11 => "true".to_string(),
            _ => "x".to_string(),
        },
    }
}

/// Generate a fuzzed query string for a single search parameter.
/// Returns `?param_name=fuzzed_value`.
pub fn generate_fuzzed_query_param(name: &str, param_type: &str, seed: u64) -> String {
    let value = generate_fuzzed_param_value(param_type, seed);
    // URL-encode the value minimally — just spaces and special chars
    let encoded = urlencoding(&value);
    format!("?{}={}", name, encoded)
}

/// Minimal URL encoding for fuzz values.
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => result.push_str("%20"),
            '"' => result.push_str("%22"),
            '#' => result.push_str("%23"),
            '%' => result.push_str("%25"),
            '&' => result.push_str("%26"),
            '\'' => result.push_str("%27"),
            '+' => result.push_str("%2B"),
            ',' => result.push_str("%2C"),
            '/' => result.push_str("%2F"),
            ':' => result.push_str("%3A"),
            ';' => result.push_str("%3B"),
            '<' => result.push_str("%3C"),
            '=' => result.push_str("%3D"),
            '>' => result.push_str("%3E"),
            '?' => result.push_str("%3F"),
            '@' => result.push_str("%40"),
            '\\' => result.push_str("%5C"),
            '|' => result.push_str("%7C"),
            '\0' => result.push_str("%00"),
            '\t' => result.push_str("%09"),
            '\n' => result.push_str("%0A"),
            '\r' => result.push_str("%0D"),
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_boundary_empty() {
        let v = generate_fuzzed_param_value("string", 0);
        assert_eq!(v, "");
    }

    #[test]
    fn string_sql_injection() {
        let v = generate_fuzzed_param_value("string", 7);
        assert!(v.contains("OR 1=1"));
    }

    #[test]
    fn token_system_code() {
        let v = generate_fuzzed_param_value("token", 2);
        assert_eq!(v, "system|code");
    }

    #[test]
    fn date_invalid_month() {
        let v = generate_fuzzed_param_value("date", 3);
        assert_eq!(v, "2024-13-01");
    }

    #[test]
    fn number_nan() {
        let v = generate_fuzzed_param_value("number", 8);
        assert_eq!(v, "NaN");
    }

    #[test]
    fn reference_path_traversal() {
        let v = generate_fuzzed_param_value("reference", 4);
        assert!(v.contains("../../"));
    }

    #[test]
    fn query_param_format() {
        let q = generate_fuzzed_query_param("name", "string", 0);
        assert_eq!(q, "?name=");
    }

    #[test]
    fn query_param_encodes_special_chars() {
        let q = generate_fuzzed_query_param("name", "string", 4);
        assert!(q.contains("%22"), "quotes should be encoded: {}", q);
        assert!(q.contains("%20"), "spaces should be encoded: {}", q);
    }

    #[test]
    fn composite_long_values() {
        let v = generate_fuzzed_param_value("composite", 11);
        assert_eq!(v.len(), 10001); // 5000 + 1 + 5000
    }

    #[test]
    fn unknown_type_falls_back() {
        let v = generate_fuzzed_param_value("unknown_type", 0);
        assert_eq!(v, "");
    }
}
