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

    // ── String param type (all 12 strategies) ────────────────────────────

    #[test]
    fn string_empty() {
        assert_eq!(generate_fuzzed_param_value("string", 0), "");
    }

    #[test]
    fn string_long() {
        let v = generate_fuzzed_param_value("string", 1);
        assert_eq!(v.len(), 10000);
        assert!(v.chars().all(|c| c == 'A'));
    }

    #[test]
    fn string_null_bytes() {
        let v = generate_fuzzed_param_value("string", 2);
        assert_eq!(v, "\0\0\0\0");
    }

    #[test]
    fn string_unicode() {
        let v = generate_fuzzed_param_value("string", 3);
        assert_eq!(v, "💉🏥🧬🩺");
    }

    #[test]
    fn string_json_injection() {
        let v = generate_fuzzed_param_value("string", 4);
        assert_eq!(v, "\"; {} //");
    }

    #[test]
    fn string_escaped_unicode() {
        let v = generate_fuzzed_param_value("string", 5);
        assert_eq!(v, "\\u0000");
    }

    #[test]
    fn string_control_chars() {
        let v = generate_fuzzed_param_value("string", 6);
        assert_eq!(v, "\t\n\r");
    }

    #[test]
    fn string_sql_injection() {
        let v = generate_fuzzed_param_value("string", 7);
        assert_eq!(v, "OR 1=1--");
    }

    #[test]
    fn string_xss() {
        let v = generate_fuzzed_param_value("string", 8);
        assert_eq!(v, "<script>alert(1)</script>");
    }

    #[test]
    fn string_path_traversal() {
        let v = generate_fuzzed_param_value("string", 9);
        assert_eq!(v, "../../etc/passwd");
    }

    #[test]
    fn string_url_encoded() {
        let v = generate_fuzzed_param_value("string", 10);
        assert_eq!(v, "%00%00%00");
    }

    #[test]
    fn string_null_literal() {
        let v = generate_fuzzed_param_value("string", 11);
        assert_eq!(v, "null");
    }

    // ── Token param type (all 12 strategies) ──────────────────────────────

    #[test]
    fn token_empty() {
        assert_eq!(generate_fuzzed_param_value("token", 0), "");
    }

    #[test]
    fn token_pipe_only() {
        assert_eq!(generate_fuzzed_param_value("token", 1), "|");
    }

    #[test]
    fn token_system_code() {
        assert_eq!(generate_fuzzed_param_value("token", 2), "system|code");
    }

    #[test]
    fn token_pipe_only_dup() {
        assert_eq!(generate_fuzzed_param_value("token", 3), "|");
    }

    #[test]
    fn token_null_bytes() {
        assert_eq!(generate_fuzzed_param_value("token", 4), "\0|\0");
    }

    #[test]
    fn token_system_only() {
        assert_eq!(
            generate_fuzzed_param_value("token", 5),
            "http://example.org|"
        );
    }

    #[test]
    fn token_value_with_spaces() {
        assert_eq!(
            generate_fuzzed_param_value("token", 6),
            "|value with spaces"
        );
    }

    #[test]
    fn token_sql_injection() {
        assert_eq!(generate_fuzzed_param_value("token", 7), "OR 1=1|");
    }

    #[test]
    fn token_xss() {
        assert_eq!(
            generate_fuzzed_param_value("token", 8),
            "<script>|</script>"
        );
    }

    #[test]
    fn token_path_traversal() {
        assert_eq!(generate_fuzzed_param_value("token", 9), "../../etc|passwd");
    }

    #[test]
    fn token_url_encoded() {
        assert_eq!(generate_fuzzed_param_value("token", 10), "%00|%00");
    }

    #[test]
    fn token_null_literal() {
        assert_eq!(generate_fuzzed_param_value("token", 11), "null|null");
    }

    // ── Date param type (all 12 strategies) ──────────────────────────────

    #[test]
    fn date_empty() {
        assert_eq!(generate_fuzzed_param_value("date", 0), "");
    }

    #[test]
    fn date_year_zero() {
        assert_eq!(generate_fuzzed_param_value("date", 1), "0000-00-00");
    }

    #[test]
    fn date_year_9999() {
        assert_eq!(generate_fuzzed_param_value("date", 2), "9999-12-31");
    }

    #[test]
    fn date_invalid_month() {
        assert_eq!(generate_fuzzed_param_value("date", 3), "2024-13-01");
    }

    #[test]
    fn date_invalid_day() {
        assert_eq!(generate_fuzzed_param_value("date", 4), "2024-01-32");
    }

    #[test]
    fn date_not_a_date() {
        assert_eq!(generate_fuzzed_param_value("date", 5), "not-a-date");
    }

    #[test]
    fn date_year_only() {
        assert_eq!(generate_fuzzed_param_value("date", 6), "2024");
    }

    #[test]
    fn date_year_month() {
        assert_eq!(generate_fuzzed_param_value("date", 7), "2024-01");
    }

    #[test]
    fn date_utc() {
        assert_eq!(
            generate_fuzzed_param_value("date", 8),
            "2024-01-01T00:00:00Z"
        );
    }

    #[test]
    fn date_with_tz() {
        assert_eq!(
            generate_fuzzed_param_value("date", 9),
            "2024-01-01T00:00:00+00:00"
        );
    }

    #[test]
    fn date_prefix_gt() {
        assert_eq!(generate_fuzzed_param_value("date", 10), "gt2024-01-01");
    }

    #[test]
    fn date_prefix_le() {
        assert_eq!(generate_fuzzed_param_value("date", 11), "le2024-01-01");
    }

    // ── Number param type (all 12 strategies) ────────────────────────────

    #[test]
    fn number_empty() {
        assert_eq!(generate_fuzzed_param_value("number", 0), "");
    }

    #[test]
    fn number_zero() {
        assert_eq!(generate_fuzzed_param_value("number", 1), "0");
    }

    #[test]
    fn number_neg_zero() {
        assert_eq!(generate_fuzzed_param_value("number", 2), "-0");
    }

    #[test]
    fn number_large_positive() {
        assert_eq!(generate_fuzzed_param_value("number", 3), "999999999999999");
    }

    #[test]
    fn number_large_negative() {
        assert_eq!(generate_fuzzed_param_value("number", 4), "-999999999999999");
    }

    #[test]
    fn number_pi() {
        assert_eq!(generate_fuzzed_param_value("number", 5), "3.14159265358979");
    }

    #[test]
    fn number_not_a_number() {
        assert_eq!(generate_fuzzed_param_value("number", 6), "not-a-number");
    }

    #[test]
    fn number_overflow() {
        assert_eq!(generate_fuzzed_param_value("number", 7), "1e9999");
    }

    #[test]
    fn number_nan() {
        assert_eq!(generate_fuzzed_param_value("number", 8), "NaN");
    }

    #[test]
    fn number_infinity() {
        assert_eq!(generate_fuzzed_param_value("number", 9), "Infinity");
    }

    #[test]
    fn number_prefix_gt() {
        assert_eq!(generate_fuzzed_param_value("number", 10), "gt100");
    }

    #[test]
    fn number_prefix_le() {
        assert_eq!(generate_fuzzed_param_value("number", 11), "le0.5");
    }

    // ── Quantity param type (all 12 strategies) ──────────────────────────

    #[test]
    fn quantity_empty() {
        assert_eq!(generate_fuzzed_param_value("quantity", 0), "");
    }

    #[test]
    fn quantity_zero() {
        assert_eq!(generate_fuzzed_param_value("quantity", 1), "0");
    }

    #[test]
    fn quantity_valid() {
        assert_eq!(
            generate_fuzzed_param_value("quantity", 2),
            "1000|http://unitsofmeasure.org|mg"
        );
    }

    #[test]
    fn quantity_empty_pipes() {
        assert_eq!(generate_fuzzed_param_value("quantity", 3), "||");
    }

    #[test]
    fn quantity_not_a_quantity() {
        assert_eq!(generate_fuzzed_param_value("quantity", 4), "not-a-quantity");
    }

    #[test]
    fn quantity_overflow() {
        assert_eq!(generate_fuzzed_param_value("quantity", 5), "1e9999||");
    }

    #[test]
    fn quantity_prefix_gt() {
        assert_eq!(
            generate_fuzzed_param_value("quantity", 6),
            "gt100|http://unitsofmeasure.org|mg"
        );
    }

    #[test]
    fn quantity_prefix_le() {
        assert_eq!(generate_fuzzed_param_value("quantity", 7), "le0.5||");
    }

    #[test]
    fn quantity_nan() {
        assert_eq!(generate_fuzzed_param_value("quantity", 8), "NaN||");
    }

    #[test]
    fn quantity_path_traversal() {
        assert_eq!(
            generate_fuzzed_param_value("quantity", 9),
            "../../etc|passwd|"
        );
    }

    #[test]
    fn quantity_url_encoded() {
        assert_eq!(generate_fuzzed_param_value("quantity", 10), "%00|%00|%00");
    }

    #[test]
    fn quantity_null_literal() {
        assert_eq!(
            generate_fuzzed_param_value("quantity", 11),
            "null|null|null"
        );
    }

    // ── URI param type (all 12 strategies) ──────────────────────────────

    #[test]
    fn uri_empty() {
        assert_eq!(generate_fuzzed_param_value("uri", 0), "");
    }

    #[test]
    fn uri_http() {
        assert_eq!(generate_fuzzed_param_value("uri", 1), "http://example.org");
    }

    #[test]
    fn uri_urn() {
        assert_eq!(generate_fuzzed_param_value("uri", 2), "urn:oid:1.2.3.4.5");
    }

    #[test]
    fn uri_path_traversal() {
        assert_eq!(generate_fuzzed_param_value("uri", 3), "../../etc/passwd");
    }

    #[test]
    fn uri_evil() {
        assert_eq!(
            generate_fuzzed_param_value("uri", 4),
            "https://evil.com?q=1"
        );
    }

    #[test]
    fn uri_not_a_uri() {
        assert_eq!(generate_fuzzed_param_value("uri", 5), "not-a-uri");
    }

    #[test]
    fn uri_null_bytes() {
        assert_eq!(generate_fuzzed_param_value("uri", 6), "\0\0\0\0");
    }

    #[test]
    fn uri_javascript() {
        assert_eq!(generate_fuzzed_param_value("uri", 7), "javascript:alert(1)");
    }

    #[test]
    fn uri_file() {
        assert_eq!(generate_fuzzed_param_value("uri", 8), "file:///etc/passwd");
    }

    #[test]
    fn uri_ipv6() {
        assert_eq!(
            generate_fuzzed_param_value("uri", 9),
            "http://[::1]:8080/path"
        );
    }

    #[test]
    fn uri_url_encoded() {
        assert_eq!(
            generate_fuzzed_param_value("uri", 10),
            "%00http://example.org"
        );
    }

    #[test]
    fn uri_null_literal() {
        assert_eq!(generate_fuzzed_param_value("uri", 11), "null");
    }

    // ── Reference param type (all 12 strategies) ─────────────────────────

    #[test]
    fn reference_empty() {
        assert_eq!(generate_fuzzed_param_value("reference", 0), "");
    }

    #[test]
    fn reference_nonexistent() {
        assert_eq!(
            generate_fuzzed_param_value("reference", 1),
            "Patient/nonexistent"
        );
    }

    #[test]
    fn reference_no_id() {
        assert_eq!(generate_fuzzed_param_value("reference", 2), "Patient/");
    }

    #[test]
    fn reference_absolute() {
        assert_eq!(generate_fuzzed_param_value("reference", 3), "/Patient/123");
    }

    #[test]
    fn reference_path_traversal() {
        assert_eq!(
            generate_fuzzed_param_value("reference", 4),
            "../../etc/passwd"
        );
    }

    #[test]
    fn reference_compound_traversal() {
        assert_eq!(
            generate_fuzzed_param_value("reference", 5),
            "Patient/../../etc/passwd"
        );
    }

    #[test]
    fn reference_zero() {
        assert_eq!(generate_fuzzed_param_value("reference", 6), "Patient/0");
    }

    #[test]
    fn reference_null() {
        assert_eq!(generate_fuzzed_param_value("reference", 7), "Patient/null");
    }

    #[test]
    fn reference_url_encoded() {
        assert_eq!(generate_fuzzed_param_value("reference", 8), "Patient/%00");
    }

    #[test]
    fn reference_sql_injection() {
        assert_eq!(
            generate_fuzzed_param_value("reference", 9),
            "Patient/OR 1=1--"
        );
    }

    #[test]
    fn reference_xss() {
        assert_eq!(
            generate_fuzzed_param_value("reference", 10),
            "Patient/<script>"
        );
    }

    #[test]
    fn reference_no_id_dup() {
        assert_eq!(generate_fuzzed_param_value("reference", 11), "Patient/");
    }

    // ── Composite param type (all 12 strategies) ─────────────────────────

    #[test]
    fn composite_empty() {
        assert_eq!(generate_fuzzed_param_value("composite", 0), "");
    }

    #[test]
    fn composite_dollar_only() {
        assert_eq!(generate_fuzzed_param_value("composite", 1), "$");
    }

    #[test]
    fn composite_two_values() {
        assert_eq!(generate_fuzzed_param_value("composite", 2), "value1$value2");
    }

    #[test]
    fn composite_prefix_dollar() {
        assert_eq!(generate_fuzzed_param_value("composite", 3), "$value");
    }

    #[test]
    fn composite_suffix_dollar() {
        assert_eq!(generate_fuzzed_param_value("composite", 4), "value$");
    }

    #[test]
    fn composite_double_dollar() {
        assert_eq!(generate_fuzzed_param_value("composite", 5), "$$");
    }

    #[test]
    fn composite_null_bytes() {
        assert_eq!(generate_fuzzed_param_value("composite", 6), "\0$\0");
    }

    #[test]
    fn composite_path_traversal() {
        assert_eq!(
            generate_fuzzed_param_value("composite", 7),
            "../../etc$passwd"
        );
    }

    #[test]
    fn composite_sql_injection() {
        assert_eq!(generate_fuzzed_param_value("composite", 8), "OR 1=1$--");
    }

    #[test]
    fn composite_url_encoded() {
        assert_eq!(generate_fuzzed_param_value("composite", 9), "%00$%00");
    }

    #[test]
    fn composite_null_literal() {
        assert_eq!(generate_fuzzed_param_value("composite", 10), "null$null");
    }

    #[test]
    fn composite_long_values() {
        let v = generate_fuzzed_param_value("composite", 11);
        assert_eq!(v.len(), 10001); // 5000 + 1 + 5000
        assert!(v.starts_with(&"A".repeat(5000)));
        assert!(v.ends_with(&"B".repeat(5000)));
    }

    // ── Special param type (all 12 strategies) ────────────────────────────

    #[test]
    fn special_empty() {
        assert_eq!(generate_fuzzed_param_value("special", 0), "");
    }

    #[test]
    fn special_near() {
        assert_eq!(generate_fuzzed_param_value("special", 1), "near");
    }

    #[test]
    fn special_near_coords() {
        assert_eq!(
            generate_fuzzed_param_value("special", 2),
            "-33.86|151.21|10|km"
        );
    }

    #[test]
    fn special_empty_pipes() {
        assert_eq!(generate_fuzzed_param_value("special", 3), "|||");
    }

    #[test]
    fn special_not_special() {
        assert_eq!(generate_fuzzed_param_value("special", 4), "not-special");
    }

    #[test]
    fn special_near_zeros() {
        assert_eq!(generate_fuzzed_param_value("special", 5), "near|0|0|0|");
    }

    #[test]
    fn special_near_large() {
        assert_eq!(
            generate_fuzzed_param_value("special", 6),
            "near|999|999|999|km"
        );
    }

    #[test]
    fn special_near_negative() {
        assert_eq!(
            generate_fuzzed_param_value("special", 7),
            "near|-999|-999|-999|km"
        );
    }

    #[test]
    fn special_near_nan() {
        assert_eq!(
            generate_fuzzed_param_value("special", 8),
            "near|NaN|NaN|NaN|km"
        );
    }

    #[test]
    fn special_near_traversal() {
        assert_eq!(
            generate_fuzzed_param_value("special", 9),
            "near|../../etc|passwd|0|km"
        );
    }

    #[test]
    fn special_near_url_encoded() {
        assert_eq!(
            generate_fuzzed_param_value("special", 10),
            "near|%00|%00|%00|km"
        );
    }

    #[test]
    fn special_near_null() {
        assert_eq!(
            generate_fuzzed_param_value("special", 11),
            "near|null|null|null|km"
        );
    }

    // ── Unknown param type (all 12 strategies) ───────────────────────────

    #[test]
    fn unknown_empty() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 0), "");
    }

    #[test]
    fn unknown_long() {
        let v = generate_fuzzed_param_value("unknown_type", 1);
        assert_eq!(v.len(), 1000);
        assert!(v.chars().all(|c| c == 'A'));
    }

    #[test]
    fn unknown_null_bytes() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 2), "\0\0\0\0");
    }

    #[test]
    fn unknown_sql_injection() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 3), "OR 1=1--");
    }

    #[test]
    fn unknown_xss() {
        assert_eq!(
            generate_fuzzed_param_value("unknown_type", 4),
            "<script>alert(1)</script>"
        );
    }

    #[test]
    fn unknown_path_traversal() {
        assert_eq!(
            generate_fuzzed_param_value("unknown_type", 5),
            "../../etc/passwd"
        );
    }

    #[test]
    fn unknown_url_encoded() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 6), "%00%00%00");
    }

    #[test]
    fn unknown_null_literal() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 7), "null");
    }

    #[test]
    fn unknown_nan() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 8), "NaN");
    }

    #[test]
    fn unknown_infinity() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 9), "Infinity");
    }

    #[test]
    fn unknown_undefined() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 10), "undefined");
    }

    #[test]
    fn unknown_true() {
        assert_eq!(generate_fuzzed_param_value("unknown_type", 11), "true");
    }

    // ── URL encoding function ────────────────────────────────────────────

    #[test]
    fn urlencode_normal_text() {
        assert_eq!(urlencoding("hello"), "hello");
    }

    #[test]
    fn urlencode_spaces() {
        assert_eq!(urlencoding("hello world"), "hello%20world");
    }

    #[test]
    fn urlencode_quotes() {
        assert_eq!(urlencoding("say \"hi\""), "say%20%22hi%22");
    }

    #[test]
    fn urlencode_hash() {
        assert_eq!(urlencoding("#1"), "%231");
    }

    #[test]
    fn urlencode_percent() {
        assert_eq!(urlencoding("100%"), "100%25");
    }

    #[test]
    fn urlencode_ampersand() {
        assert_eq!(urlencoding("a&b"), "a%26b");
    }

    #[test]
    fn urlencode_plus() {
        assert_eq!(urlencoding("a+b"), "a%2Bb");
    }

    #[test]
    fn urlencode_comma() {
        assert_eq!(urlencoding("a,b"), "a%2Cb");
    }

    #[test]
    fn urlencode_slash() {
        assert_eq!(urlencoding("a/b"), "a%2Fb");
    }

    #[test]
    fn urlencode_colon() {
        assert_eq!(urlencoding("a:b"), "a%3Ab");
    }

    #[test]
    fn urlencode_semicolon() {
        assert_eq!(urlencoding("a;b"), "a%3Bb");
    }

    #[test]
    fn urlencode_angle_brackets() {
        assert_eq!(urlencoding("a<b>c"), "a%3Cb%3Ec");
    }

    #[test]
    fn urlencode_equals() {
        assert_eq!(urlencoding("a=b"), "a%3Db");
    }

    #[test]
    fn urlencode_question_mark() {
        assert_eq!(urlencoding("a?b"), "a%3Fb");
    }

    #[test]
    fn urlencode_at_sign() {
        assert_eq!(urlencoding("a@b"), "a%40b");
    }

    #[test]
    fn urlencode_backslash() {
        assert_eq!(urlencoding("a\\b"), "a%5Cb");
    }

    #[test]
    fn urlencode_pipe() {
        assert_eq!(urlencoding("a|b"), "a%7Cb");
    }

    #[test]
    fn urlencode_null() {
        assert_eq!(urlencoding("\0"), "%00");
    }

    #[test]
    fn urlencode_tab() {
        assert_eq!(urlencoding("\t"), "%09");
    }

    #[test]
    fn urlencode_newline() {
        assert_eq!(urlencoding("\n"), "%0A");
    }

    #[test]
    fn urlencode_carriage_return() {
        assert_eq!(urlencoding("\r"), "%0D");
    }

    #[test]
    fn urlencode_unicode() {
        assert_eq!(urlencoding("💉"), "💉"); // multi-byte unicode passes through
    }

    #[test]
    fn urlencode_empty() {
        assert_eq!(urlencoding(""), "");
    }

    // ── generate_fuzzed_query_param ──────────────────────────────────────

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
    fn query_param_with_token() {
        let q = generate_fuzzed_query_param("identifier", "token", 2);
        assert_eq!(q, "?identifier=system%7Ccode");
    }

    #[test]
    fn query_param_with_date() {
        let q = generate_fuzzed_query_param("birthdate", "date", 3);
        assert_eq!(q, "?birthdate=2024-13-01");
    }

    #[test]
    fn query_param_with_number() {
        let q = generate_fuzzed_query_param("age", "number", 8);
        assert_eq!(q, "?age=NaN");
    }

    #[test]
    fn query_param_with_reference() {
        let q = generate_fuzzed_query_param("patient", "reference", 1);
        assert_eq!(q, "?patient=Patient%2Fnonexistent");
    }

    #[test]
    fn query_param_with_composite() {
        let q = generate_fuzzed_query_param("code-value", "composite", 2);
        // $ is not URL-encoded by urlencoding, so it passes through
        assert_eq!(q, "?code-value=value1$value2");
    }

    #[test]
    fn query_param_with_special() {
        let q = generate_fuzzed_query_param("near", "special", 2);
        assert_eq!(q, "?near=-33.86%7C151.21%7C10%7Ckm");
    }

    #[test]
    fn query_param_with_unknown() {
        let q = generate_fuzzed_query_param("custom", "unknown_type", 0);
        assert_eq!(q, "?custom=");
    }

    // ── SearchParamMutator ───────────────────────────────────────────────

    #[test]
    fn search_param_mutator_name() {
        let m = SearchParamMutator;
        assert_eq!(m.name(), "search_param");
    }

    #[test]
    fn search_param_mutator_returns_clone() {
        let m = SearchParamMutator;
        let resource = serde_json::json!({"name": "test"});
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
        let result = m.mutate(&resource, &profile, 42);
        assert_eq!(result, resource);
    }
}
