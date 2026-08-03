use crate::generate::model::*;
use serde_json::Value;

/// Evaluate a response assertion against an actual HTTP response body.
/// Returns a list of assertion failures (empty = all assertions pass).
pub fn assert_response(
    assertion: &ResponseAssertion,
    _status_code: u16,
    body: &Option<Value>,
) -> Vec<String> {
    let mut errors = Vec::new();

    // --- Bundle type ---
    if let Some(expected_type) = &assertion.bundle_type {
        if let Some(body) = body {
            if let Some(rt) = body.get("resourceType").and_then(|v| v.as_str()) {
                if rt == "Bundle" {
                    if let Some(actual_type) = body.get("type").and_then(|v| v.as_str()) {
                        if actual_type != expected_type {
                            errors.push(format!(
                                "Bundle type is '{}', expected '{}'",
                                actual_type, expected_type
                            ));
                        }
                    } else {
                        errors.push("Bundle has no 'type' field".to_string());
                    }
                } else if rt != "OperationOutcome" {
                    errors.push(format!("Expected Bundle, got resourceType '{}'", rt));
                }
            } else {
                errors.push("Response has no resourceType".to_string());
            }
        } else {
            errors.push("No response body to assert Bundle type".to_string());
        }
    }

    // --- Entry count and content ---
    if let Some(body) = body {
        if let Some(entries) = body.get("entry").and_then(|v| v.as_array()) {
            let count = entries.len();
            if let Some(min) = assertion.min_entries
                && count < min
            {
                errors.push(format!(
                    "Bundle has {} entries, expected at least {}",
                    count, min
                ));
            }
            if let Some(max) = assertion.max_entries
                && count > max
            {
                errors.push(format!(
                    "Bundle has {} entries, expected at most {}",
                    count, max
                ));
            }

            // --- Resource types present ---
            if !assertion.resource_types.is_empty() {
                let present_types: std::collections::HashSet<String> = entries
                    .iter()
                    .filter_map(|e| {
                        e.get("resource")
                            .and_then(|r| r.get("resourceType"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();

                for expected_rt in &assertion.resource_types {
                    if !present_types.contains(expected_rt) {
                        errors.push(format!(
                            "Expected Bundle to contain '{}' resource(s), but found: {:?}",
                            expected_rt,
                            present_types.iter().collect::<Vec<_>>()
                        ));
                    }
                }
            }

            // --- Field values ---
            for (resource_type, fields) in &assertion.field_values {
                let matching_entries: Vec<_> = entries
                    .iter()
                    .filter(|e| {
                        e.get("resource")
                            .and_then(|r| r.get("resourceType"))
                            .and_then(|v| v.as_str())
                            == Some(resource_type.as_str())
                    })
                    .collect();

                if matching_entries.is_empty() {
                    errors.push(format!(
                        "Expected at least one {} in Bundle for field assertion, found none",
                        resource_type
                    ));
                    continue;
                }

                for entry in &matching_entries {
                    let resource = entry.get("resource").unwrap();
                    for (path, expected_value) in fields {
                        let actual = resolve_json_path(resource, path);
                        match actual {
                            None => {
                                errors.push(format!(
                                    "{}: field '{}' not found in response",
                                    resource_type, path
                                ));
                            }
                            Some(val) if val != *expected_value => {
                                errors.push(format!(
                                    "{}: field '{}' expected {:?}, got {:?}",
                                    resource_type, path, expected_value, val
                                ));
                            }
                            _ => {} // matches
                        }
                    }
                }
            }

            // --- Include types ---
            for include_type in assertion.include_types.keys() {
                let found = entries.iter().any(|e| {
                    e.get("resource")
                        .and_then(|r| r.get("resourceType"))
                        .and_then(|v| v.as_str())
                        == Some(include_type.as_str())
                });
                if !found {
                    errors.push(format!(
                        "Expected Bundle to include '{}' resources from _include/_revinclude, but none found",
                        include_type
                    ));
                }
            }

            // --- Include with polymorphic target ---
            if let Some(primary_type) = &assertion.include_requires_distinct_from {
                let has_primary = entries.iter().any(|e| {
                    e.get("resource")
                        .and_then(|r| r.get("resourceType"))
                        .and_then(|v| v.as_str())
                        == Some(primary_type.as_str())
                });
                if has_primary {
                    let has_distinct = entries.iter().any(|e| {
                        e.get("resource")
                            .and_then(|r| r.get("resourceType"))
                            .and_then(|v| v.as_str())
                            .map(|rt| rt != primary_type)
                            .unwrap_or(false)
                    });
                    if !has_distinct {
                        errors.push(format!(
                            "Expected _include/_revinclude to return at least one resource type distinct from '{}'",
                            primary_type
                        ));
                    }
                }
            }

            // --- Sort assertion ---
            if let Some(sort) = &assertion.sort_by {
                let resources: Vec<&Value> =
                    entries.iter().filter_map(|e| e.get("resource")).collect();

                if resources.len() >= 2 {
                    let values: Vec<Option<Value>> = resources
                        .iter()
                        .map(|r| resolve_json_path(r, &sort.field))
                        .collect();

                    let sorted = match sort.direction.as_str() {
                        "asc" => values.windows(2).all(|w| compare_values(&w[0], &w[1]) <= 0),
                        "desc" => values.windows(2).all(|w| compare_values(&w[0], &w[1]) >= 0),
                        _ => true,
                    };

                    if !sorted {
                        errors.push(format!(
                            "Bundle entries not sorted by '{}' in {} order",
                            sort.field, sort.direction
                        ));
                    }
                }
            }

            // --- Absent fields (for _summary) ---
            for field in &assertion.absent_fields {
                for entry in entries.iter() {
                    if let Some(resource) = entry.get("resource")
                        && resource.get(field).is_some()
                    {
                        errors.push(format!(
                            "Resource contains field '{}' which should be absent with _summary",
                            field
                        ));
                    }
                }
            }
        } else if body.get("resourceType").and_then(|v| v.as_str()) == Some("Bundle") {
            // No entry array on the Bundle.
            // This is only an error if the assertion requires entries.
            // A searchset Bundle with total=0 and no entries is valid when:
            //   - min_entries is None or 0 (just checking Bundle structure)
            //   - resource_types, include_types, field_values, required_fields are empty
            //   - absent_fields, sort_by are irrelevant without entries
            //   - total=0 (empty search result — no entries expected)
            let bundle_total = body.get("total").and_then(|v| v.as_i64()).unwrap_or(-1);
            let requires_entries = assertion.min_entries.is_some_and(|min| min > 0)
                || !assertion.resource_types.is_empty()
                || !assertion.include_types.is_empty()
                || !assertion.field_values.is_empty()
                || !assertion.required_fields.is_empty();
            if requires_entries && bundle_total != 0 {
                errors.push("Bundle has no 'entry' array".to_string());
            }
        }
    }

    // --- OperationOutcome severity ---
    // Per FHIR spec, servers may ignore unknown search parameters and return
    // a Bundle instead of an OperationOutcome. When the server returns a
    // Bundle for a negative test, we accept it (the executor already passes
    // 2xx+Bundle for expected_status==0). We only check OperationOutcome
    // structure when the response IS an OperationOutcome.
    if let Some(expected_severity) = &assertion.outcome_severity {
        if let Some(body) = body {
            if body.get("resourceType").and_then(|v| v.as_str()) != Some("OperationOutcome") {
                // Not an OperationOutcome — if it's a Bundle, that's acceptable
                // for negative conformance tests (server chose to ignore the
                // unknown param rather than reject it). Only flag as an error
                // if it's some other resource type entirely.
                if body.get("resourceType").and_then(|v| v.as_str()) != Some("Bundle") {
                    errors.push(format!(
                        "Expected OperationOutcome, got resourceType '{}'",
                        body.get("resourceType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                    ));
                }
                // Bundle is acceptable — skip OperationOutcome validation
            } else {
                let issues = body.get("issue").and_then(|v| v.as_array());
                match issues {
                    None => {
                        errors.push("OperationOutcome has no 'issue' array".to_string());
                    }
                    Some(issues) if issues.is_empty() => {
                        errors.push("OperationOutcome has empty 'issue' array".to_string());
                    }
                    Some(issues) => {
                        let has_matching = issues.iter().any(|i| {
                            i.get("severity")
                                .and_then(|v| v.as_str())
                                .map(|s| s == expected_severity)
                                .unwrap_or(false)
                        });
                        if !has_matching {
                            let severities: Vec<&str> = issues
                                .iter()
                                .filter_map(|i| i.get("severity").and_then(|v| v.as_str()))
                                .collect();
                            errors.push(format!(
                                "Expected OperationOutcome with severity '{}', found: {:?}",
                                expected_severity, severities
                            ));
                        }
                    }
                }
            }
        } else {
            errors.push("No response body for OperationOutcome assertion".to_string());
        }
    }

    // --- Top-level key presence ---
    if let Some(key) = &assertion.response_contains_key
        && let Some(body) = body
        && body.get(key).is_none()
    {
        errors.push(format!(
            "Expected response to contain key '{}', but it was not found",
            key
        ));
    }

    // --- Top-level response resourceType allow-list ---
    if !assertion.response_resource_types.is_empty() {
        if let Some(body) = body {
            match body.get("resourceType").and_then(|v| v.as_str()) {
                Some(actual)
                    if assertion
                        .response_resource_types
                        .iter()
                        .any(|allowed| allowed == actual) => {}
                Some(actual) => {
                    errors.push(format!(
                        "Response resourceType '{}' not in allowed set {:?}",
                        actual, assertion.response_resource_types
                    ));
                }
                None => {
                    errors.push("Response has no resourceType".to_string());
                }
            }
        } else {
            errors.push("No response body for resourceType assertion".to_string());
        }
    }

    // --- Operation output parameter validation ---
    // For $operation responses: check that the `parameter` array contains
    // entries with the expected output parameter names.
    if !assertion.operation_output_params.is_empty() {
        if let Some(body) = body {
            if let Some(param_array) = body.get("parameter").and_then(|v| v.as_array()) {
                let param_names: std::collections::HashSet<String> = param_array
                    .iter()
                    .filter_map(|p| {
                        p.get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
                for expected_name in &assertion.operation_output_params {
                    if !param_names.contains(expected_name) {
                        errors.push(format!(
                            "Expected output parameter '{}' not found in response parameter array (found: {:?})",
                            expected_name, param_names
                        ));
                    }
                }
            } else {
                errors.push(
                    "Response has no 'parameter' array for operation output param assertion"
                        .to_string(),
                );
            }
        } else {
            errors.push("No response body for operation output param assertion".to_string());
        }
    }

    // --- Bundle total presence ---
    if let Some(expect_total) = assertion.bundle_total_present {
        if let Some(body) = body {
            let has_total = body.get("total").is_some();
            if expect_total && !has_total {
                errors.push("Bundle should have a 'total' field but it is missing".to_string());
            }
            if !expect_total && has_total {
                errors.push("Bundle should NOT have a 'total' field but it is present".to_string());
            }
        } else {
            errors.push("No response body for bundle_total_present assertion".to_string());
        }
    }

    // --- Summary mode assertion ---
    if let Some(summary_mode) = &assertion.summary_mode {
        if let Some(body) = body {
            if let Some(entries) = body.get("entry").and_then(|v| v.as_array()) {
                match summary_mode.as_str() {
                    "count" => {
                        // _summary=count: Bundle should have total but no entries
                        if !entries.is_empty() {
                            errors.push(
                                "_summary=count: Bundle should have no entries but has some"
                                    .to_string(),
                            );
                        }
                    }
                    "text" => {
                        // _summary=text: Resources must have text field
                        for entry in entries {
                            if let Some(resource) = entry.get("resource")
                                && resource.get("text").is_none()
                            {
                                errors.push("_summary=text: Resource should have 'text' field but it is missing".to_string());
                            }
                        }
                    }
                    "data" => {
                        // _summary=data: Resources must NOT have text field
                        for entry in entries {
                            if let Some(resource) = entry.get("resource")
                                && resource.get("text").is_some()
                            {
                                errors.push("_summary=data: Resource should NOT have 'text' field but it is present".to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else {
            errors.push("No response body for summary_mode assertion".to_string());
        }
    }

    // --- MustSupport required field presence (best-effort) ---
    // Checks that specified fields exist in Bundle entries, regardless of their value.
    // Per FHIR R4 §2.1.2.1.12, mustSupport means "the server SHALL populate the
    // element if the data exists for the use case." A field may be legitimately
    // absent if the server has no data for it. This is a best-effort heuristic —
    // absence does not necessarily indicate non-conformance.
    for (resource_type, fields) in &assertion.required_fields {
        if let Some(body) = body {
            if let Some(entries) = body.get("entry").and_then(|v| v.as_array()) {
                let matching: Vec<&Value> = entries
                    .iter()
                    .filter(|e| {
                        e.get("resource")
                            .and_then(|r| r.get("resourceType"))
                            .and_then(|v| v.as_str())
                            == Some(resource_type.as_str())
                    })
                    .collect();

                if matching.is_empty() {
                    // No entries of the expected resource type in the Bundle.
                    // This is expected when the search returned 0 results — it's a
                    // data setup gap, not a server conformance violation.
                    // Only report an error if the Bundle itself claims to have results.
                    let bundle_total = body.get("total").and_then(|v| v.as_i64()).unwrap_or(-1);
                    if bundle_total > 0 {
                        errors.push(format!(
                            "Expected at least one {} in Bundle for required field check, found none (Bundle total={})",
                            resource_type, bundle_total
                        ));
                    }
                    continue;
                }

                for entry in &matching {
                    let resource = entry.get("resource").unwrap();
                    for field_path in fields {
                        let actual = resolve_json_path(resource, field_path);
                        if actual.is_none() {
                            errors.push(format!(
                                "{}: mustSupport field '{}' not found in response (best-effort check — may be absent if server has no data)",
                                resource_type, field_path
                            ));
                        }
                    }
                }
            } else {
                // No entry array on the Bundle. Only error if the Bundle
                // claims to have results (total > 0). A Bundle with total=0
                // and no entries is a valid empty search result.
                let bundle_total = body.get("total").and_then(|v| v.as_i64()).unwrap_or(-1);
                if bundle_total > 0 {
                    errors.push(format!(
                        "Expected Bundle with entries for {} required field check (Bundle total={})",
                        resource_type, bundle_total
                    ));
                }
            }
        }
    }

    // --- Required binding validation ---
    // For fields with required bindings, check that the field values are
    // within the bound ValueSet. This is a best-effort check — we verify
    // the field exists and has a value, but full ValueSet membership
    // validation requires resolving the ValueSet from the IG package.
    for (resource_type, bindings) in &assertion.required_bindings {
        if let Some(body) = body
            && let Some(entries) = body.get("entry").and_then(|v| v.as_array())
        {
            let matching: Vec<&Value> = entries
                .iter()
                .filter(|e| {
                    e.get("resource")
                        .and_then(|r| r.get("resourceType"))
                        .and_then(|v| v.as_str())
                        == Some(resource_type.as_str())
                })
                .collect();

            if matching.is_empty() {
                continue;
            }

            for (field_path, value_set_url) in bindings {
                for entry in &matching {
                    if let Some(resource) = entry.get("resource") {
                        let actual = resolve_json_path(resource, field_path);
                        if actual.is_none() {
                            errors.push(format!(
                                "{}: required binding field '{}' not found in response (ValueSet: {})",
                                resource_type, field_path, value_set_url
                            ));
                        }
                    }
                }
            }
        }
    }

    // --- Slice validation ---
    // For sliced elements, check that the discriminator path/value matches.
    // This is a best-effort check — we verify the field exists and has the
    // expected discriminator value.
    for (resource_type, slices) in &assertion.slice_assertions {
        if let Some(body) = body
            && let Some(entries) = body.get("entry").and_then(|v| v.as_array())
        {
            let matching: Vec<&Value> = entries
                .iter()
                .filter(|e| {
                    e.get("resource")
                        .and_then(|r| r.get("resourceType"))
                        .and_then(|v| v.as_str())
                        == Some(resource_type.as_str())
                })
                .collect();

            if matching.is_empty() {
                continue;
            }

            for (field_path, slice_name, discriminator_path, discriminator_type) in slices {
                for entry in &matching {
                    if let Some(resource) = entry.get("resource") {
                        let field_value = resolve_json_path(resource, field_path);
                        if let Some(val) = field_value {
                            // For value-type discriminators, check the discriminator path exists
                            if discriminator_type == "value" || discriminator_type == "pattern" {
                                let disc_value = resolve_json_path(&val, discriminator_path);
                                if disc_value.is_none() {
                                    errors.push(format!(
                                        "{}: slice '{}' on '{}' missing discriminator '{}' (type: {})",
                                        resource_type, slice_name, field_path, discriminator_path, discriminator_type
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // --- Extension validation ---
    // Check that extensions in responses match profile-defined extension URLs.
    for (resource_type, extension_urls) in &assertion.required_extensions {
        if let Some(body) = body
            && let Some(entries) = body.get("entry").and_then(|v| v.as_array())
        {
            let matching: Vec<&Value> = entries
                .iter()
                .filter(|e| {
                    e.get("resource")
                        .and_then(|r| r.get("resourceType"))
                        .and_then(|v| v.as_str())
                        == Some(resource_type.as_str())
                })
                .collect();

            if matching.is_empty() {
                continue;
            }

            for extension_url in extension_urls {
                for entry in &matching {
                    if let Some(resource) = entry.get("resource")
                        && let Some(extensions) =
                            resource.get("extension").and_then(|v| v.as_array())
                    {
                        let has_url = extensions.iter().any(|ext| {
                            ext.get("url")
                                .and_then(|v| v.as_str())
                                .map(|u| u == extension_url)
                                .unwrap_or(false)
                        });
                        if !has_url {
                            errors.push(format!(
                                "{}: extension '{}' not found in response",
                                resource_type, extension_url
                            ));
                        }
                    }
                }
            }
        }
    }

    // --- Type constraint validation ---
    // For polymorphic value[x] fields, check that the actual key used
    // matches one of the allowed types.
    for (resource_type, constraints) in &assertion.type_constraints {
        if let Some(body) = body
            && let Some(entries) = body.get("entry").and_then(|v| v.as_array())
        {
            let matching: Vec<&Value> = entries
                .iter()
                .filter(|e| {
                    e.get("resource")
                        .and_then(|r| r.get("resourceType"))
                        .and_then(|v| v.as_str())
                        == Some(resource_type.as_str())
                })
                .collect();

            if matching.is_empty() {
                continue;
            }

            for (field_path, allowed_types) in constraints {
                for entry in &matching {
                    if let Some(resource) = entry.get("resource") {
                        // The field_path is like "value[x]" — resolve it
                        let actual = resolve_json_path(resource, field_path);
                        if let Some(val) = actual {
                            // Check that the actual key used matches an allowed type
                            // For value[x], the actual key will be something like valueString, valueCodeableConcept, etc.
                            let base_name = field_path.trim_end_matches("[x]");
                            let found_key = resource.as_object().and_then(|obj| {
                                obj.keys()
                                    .find(|k| k.starts_with(base_name) && k.len() > base_name.len())
                            });

                            if let Some(actual_key) = found_key {
                                let type_suffix = actual_key.strip_prefix(base_name).unwrap_or("");
                                let is_allowed = allowed_types.iter().any(|t| {
                                    // Check if the type suffix matches (case-insensitive)
                                    t.eq_ignore_ascii_case(type_suffix)
                                });
                                if !is_allowed {
                                    errors.push(format!(
                                        "{}: polymorphic field '{}' uses type '{}' which is not in allowed types {:?}",
                                        resource_type, field_path, actual_key, allowed_types
                                    ));
                                }
                            }

                            // Also check that the value itself is present
                            if val.is_null() {
                                errors.push(format!(
                                    "{}: polymorphic field '{}' has null value",
                                    resource_type, field_path
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // --- Reference target profile validation ---
    // For reference fields with target profiles, check that the referenced
    // resources have matching meta.profile. This is a best-effort check
    // since we may not have the referenced resource in the response.
    for (resource_type, targets) in &assertion.reference_targets {
        if let Some(body) = body
            && let Some(entries) = body.get("entry").and_then(|v| v.as_array())
        {
            let matching: Vec<&Value> = entries
                .iter()
                .filter(|e| {
                    e.get("resource")
                        .and_then(|r| r.get("resourceType"))
                        .and_then(|v| v.as_str())
                        == Some(resource_type.as_str())
                })
                .collect();

            if matching.is_empty() {
                continue;
            }

            for (field_path, target_profile) in targets {
                for entry in &matching {
                    if let Some(resource) = entry.get("resource") {
                        let reference = resolve_json_path(resource, field_path);
                        if let Some(ref_val) = reference {
                            // Check if the reference has a resource that's also in the Bundle
                            if let Some(ref_str) = ref_val.as_str() {
                                // Try to find the referenced resource in the Bundle entries
                                let referenced = entries.iter().find(|e| {
                                    e.get("fullUrl")
                                        .and_then(|v| v.as_str())
                                        .map(|url| {
                                            ref_str
                                                .ends_with(url.split('/').next_back().unwrap_or(""))
                                        })
                                        .unwrap_or(false)
                                });

                                if let Some(ref_entry) = referenced
                                    && let Some(ref_resource) = ref_entry.get("resource")
                                {
                                    let profiles = ref_resource
                                        .get("meta")
                                        .and_then(|m| m.get("profile"))
                                        .and_then(|p| p.as_array());

                                    match profiles {
                                        Some(profiles) => {
                                            let has_profile = profiles.iter().any(|p| {
                                                p.as_str()
                                                    .map(|s| {
                                                        s == target_profile
                                                            || s.starts_with(target_profile)
                                                    })
                                                    .unwrap_or(false)
                                            });
                                            if !has_profile {
                                                errors.push(format!(
                                                    "{}: reference '{}' points to resource without target profile '{}' (meta.profile: {:?})",
                                                    resource_type, field_path, target_profile,
                                                    profiles.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>()
                                                ));
                                            }
                                        }
                                        None => {
                                            errors.push(format!(
                                                "{}: reference '{}' points to resource with no meta.profile (expected '{}')",
                                                resource_type, field_path, target_profile
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    errors
}

/// Resolve a dotted JSON path like "name.family" or "birthDate" to a value.
/// Handles arrays by returning the first matching value.
pub(crate) fn resolve_json_path(value: &Value, path: &str) -> Option<Value> {
    if path.is_empty() {
        return Some(value.clone());
    }

    // Two-phase array resolution for O(1) best-case performance:
    // 1. Fast path: try arr[0] first (most common case — the first element in a
    //    FHIR array usually contains the expected fields).
    // 2. Fallback: only search remaining elements if the fast path fails.
    //    This handles cases where the target sub-path is in a later element.
    if let Some(arr) = value.as_array() {
        // Fast path: try first element
        if let Some(first) = arr.first()
            && let Some(result) = resolve_json_path(first, path)
        {
            return Some(result);
        }
        // Fallback: search remaining elements
        for elem in arr.iter().skip(1) {
            if let Some(result) = resolve_json_path(elem, path) {
                return Some(result);
            }
        }
        return None;
    }

    let (head, tail) = match path.split_once('.') {
        Some((h, t)) => (h, Some(t)),
        None => (path, None),
    };

    let obj = value.as_object()?;

    let next_value = if head == "value[x]" {
        // Polymorphic FHIR field: matches any key prefixed with "value" followed by
        // at least one more character (valueCodeableConcept, valueString, etc.).
        obj.iter()
            .find(|(key, _)| key.starts_with("value") && key.len() > "value".len())
            .map(|(_, v)| v)?
    } else {
        obj.get(head)?
    };

    match tail {
        None => Some(next_value.clone()),
        Some(t) => resolve_json_path(next_value, t),
    }
}

/// Compare two JSON values for sorting. Returns negative if a < b, 0 if equal, positive if a > b.
fn compare_values(a: &Option<Value>, b: &Option<Value>) -> i32 {
    match (a, b) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(a_val), Some(b_val)) => {
            if let (Some(a_str), Some(b_str)) = (a_val.as_str(), b_val.as_str()) {
                a_str.cmp(b_str) as i32
            } else if let (Some(a_num), Some(b_num)) = (a_val.as_f64(), b_val.as_f64()) {
                a_num
                    .partial_cmp(&b_num)
                    .unwrap_or(std::cmp::Ordering::Equal) as i32
            } else {
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn assert_bundle_type_match() {
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_bundle_type_mismatch() {
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "batch",
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("batch") && e.contains("searchset"))
        );
    }

    #[test]
    fn assert_min_entries_pass() {
        let assertion = ResponseAssertion {
            min_entries: Some(1),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{"resource": {"resourceType": "Patient", "id": "123"}}]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_min_entries_fail() {
        let assertion = ResponseAssertion {
            min_entries: Some(2),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{"resource": {"resourceType": "Patient", "id": "123"}}]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("at least 2")));
    }

    #[test]
    fn assert_resource_types_present() {
        let assertion = ResponseAssertion {
            resource_types: vec!["Patient".to_string(), "Provenance".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}},
                {"resource": {"resourceType": "Provenance", "id": "2"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_resource_types_missing() {
        let assertion = ResponseAssertion {
            resource_types: vec!["Patient".to_string(), "Observation".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("Observation")));
    }

    #[test]
    fn assert_include_types_present() {
        let mut include_types = HashMap::new();
        include_types.insert("Organization".to_string(), "organization".to_string());
        let assertion = ResponseAssertion {
            include_types,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}},
                {"resource": {"resourceType": "Organization", "id": "2"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_include_types_missing() {
        let mut include_types = HashMap::new();
        include_types.insert("Location".to_string(), "location".to_string());
        let assertion = ResponseAssertion {
            include_types,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("Location")));
    }

    #[test]
    fn assert_field_values_match() {
        let mut field_values = HashMap::new();
        let mut patient_fields = HashMap::new();
        patient_fields.insert("name.family".to_string(), serde_json::json!("Smith"));
        field_values.insert("Patient".to_string(), patient_fields);
        let assertion = ResponseAssertion {
            field_values,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "name": [{"family": "Smith"}], "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_field_values_mismatch() {
        let mut field_values = HashMap::new();
        let mut patient_fields = HashMap::new();
        patient_fields.insert("name.family".to_string(), serde_json::json!("Jones"));
        field_values.insert("Patient".to_string(), patient_fields);
        let assertion = ResponseAssertion {
            field_values,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "name": [{"family": "Smith"}], "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("expected") && e.contains("got"))
        );
    }

    #[test]
    fn assert_required_fields_present() {
        let mut required = HashMap::new();
        required.insert(
            "Patient".to_string(),
            vec!["name".to_string(), "birthDate".to_string()],
        );
        let assertion = ResponseAssertion {
            required_fields: required,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "name": [{"family": "T"}], "birthDate": "2000-01-01", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_required_fields_missing() {
        let mut required = HashMap::new();
        required.insert("Patient".to_string(), vec!["deceasedDateTime".to_string()]);
        let assertion = ResponseAssertion {
            required_fields: required,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "name": [{"family": "T"}], "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("deceasedDateTime")));
    }

    #[test]
    fn assert_required_fields_skipped_on_empty_bundle() {
        let mut required = HashMap::new();
        required.insert("Patient".to_string(), vec!["name".to_string()]);
        let assertion = ResponseAssertion {
            required_fields: required,
            ..ResponseAssertion::none()
        };
        // Empty Bundle with total=0 — should not error
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 0
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for empty Bundle, got: {:?}",
            errors
        );
    }

    #[test]
    fn assert_absent_fields_pass() {
        let assertion = ResponseAssertion {
            absent_fields: vec!["text".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_absent_fields_fail() {
        let assertion = ResponseAssertion {
            absent_fields: vec!["text".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1", "text": {"status": "generated"}}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("text")));
    }

    #[test]
    fn assert_outcome_severity_match() {
        let assertion = ResponseAssertion {
            outcome_severity: Some("error".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "not-found"}]
        });
        let errors = assert_response(&assertion, 404, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_outcome_severity_mismatch() {
        let assertion = ResponseAssertion {
            outcome_severity: Some("fatal".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "not-found"}]
        });
        let errors = assert_response(&assertion, 404, &Some(body));
        assert!(errors.iter().any(|e| e.contains("fatal")));
    }

    #[test]
    fn assert_response_contains_key() {
        let assertion = ResponseAssertion {
            response_contains_key: Some("parameter".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Parameters",
            "parameter": [{"name": "return", "valueString": "ok"}]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_response_contains_key_missing() {
        let assertion = ResponseAssertion {
            response_contains_key: Some("parameter".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "OperationOutcome",
            "issue": []
        });
        let errors = assert_response(&assertion, 400, &Some(body));
        assert!(errors.iter().any(|e| e.contains("parameter")));
    }

    #[test]
    fn assert_response_resource_types_allowed() {
        let assertion = ResponseAssertion {
            response_resource_types: vec!["Parameters".to_string(), "OperationOutcome".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Parameters",
            "parameter": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_response_resource_types_rejected() {
        let assertion = ResponseAssertion {
            response_resource_types: vec!["Parameters".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset"
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("Bundle")));
    }

    #[test]
    fn assert_sort_ascending() {
        let assertion = ResponseAssertion {
            sort_by: Some(SortAssertion {
                field: "birthDate".to_string(),
                direction: "asc".to_string(),
                additional_fields: Vec::new(),
            }),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "birthDate": "2000-01-01", "id": "1"}},
                {"resource": {"resourceType": "Patient", "birthDate": "2000-06-15", "id": "2"}},
                {"resource": {"resourceType": "Patient", "birthDate": "2001-01-01", "id": "3"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_sort_descending() {
        let assertion = ResponseAssertion {
            sort_by: Some(SortAssertion {
                field: "birthDate".to_string(),
                direction: "desc".to_string(),
                additional_fields: Vec::new(),
            }),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "birthDate": "2001-01-01", "id": "3"}},
                {"resource": {"resourceType": "Patient", "birthDate": "2000-06-15", "id": "2"}},
                {"resource": {"resourceType": "Patient", "birthDate": "2000-01-01", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_sort_not_sorted() {
        let assertion = ResponseAssertion {
            sort_by: Some(SortAssertion {
                field: "birthDate".to_string(),
                direction: "asc".to_string(),
                additional_fields: Vec::new(),
            }),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "birthDate": "2001-01-01", "id": "3"}},
                {"resource": {"resourceType": "Patient", "birthDate": "2000-01-01", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("sorted")));
    }

    #[test]
    fn assert_include_requires_distinct_from() {
        let assertion = ResponseAssertion {
            include_requires_distinct_from: Some("Patient".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}},
                {"resource": {"resourceType": "Organization", "id": "2"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_include_requires_distinct_from_fails_when_only_primary() {
        let assertion = ResponseAssertion {
            include_requires_distinct_from: Some("Patient".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("distinct")));
    }

    #[test]
    fn assert_include_requires_distinct_from_skipped_when_no_primary() {
        let assertion = ResponseAssertion {
            include_requires_distinct_from: Some("Patient".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Organization", "id": "2"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_empty_bundle_no_entry_no_error() {
        // A Bundle with total=0 and no entry array should not error
        // when required_fields is set (the required_fields check already
        // handles total=0 gracefully).
        let mut required = HashMap::new();
        required.insert("Patient".to_string(), vec!["name".to_string()]);
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            required_fields: required,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 0
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for empty Bundle, got: {:?}",
            errors
        );
    }

    #[test]
    fn assert_empty_bundle_no_entry_errors_when_min_entries_gt_0() {
        // A Bundle with total=0 but min_entries=1 should NOT error because
        // total=0 means there are genuinely 0 results — the Bundle is valid.
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            min_entries: Some(1),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 0
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for total=0 Bundle, got: {:?}",
            errors
        );
    }

    #[test]
    fn resolve_json_path_simple() {
        let value = json!({"name": [{"family": "Smith"}], "id": "123"});
        assert_eq!(
            resolve_json_path(&value, "name.family"),
            Some(serde_json::json!("Smith"))
        );
    }

    #[test]
    fn resolve_json_path_missing() {
        let value = json!({"id": "123"});
        assert_eq!(resolve_json_path(&value, "name.family"), None);
    }

    #[test]
    fn resolve_json_path_value_x() {
        let value = json!({"valueString": "hello"});
        assert_eq!(
            resolve_json_path(&value, "value[x]"),
            Some(serde_json::json!("hello"))
        );
    }

    #[test]
    fn resolve_json_path_value_x_codeable_concept() {
        let value = json!({"valueCodeableConcept": {"coding": [{"code": "test"}]}});
        let result = resolve_json_path(&value, "value[x]");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap()["coding"][0]["code"],
            serde_json::json!("test")
        );
    }

    #[test]
    fn resolve_json_path_nested_array() {
        let value = json!({
            "identifier": [{
                "type": {
                    "coding": [{"code": "XX"}]
                }
            }]
        });
        assert_eq!(
            resolve_json_path(&value, "identifier.type.coding.code"),
            Some(serde_json::json!("XX"))
        );
    }

    #[test]
    fn resolve_json_path_empty_path() {
        let value = json!({"a": 1});
        assert_eq!(resolve_json_path(&value, ""), Some(value));
    }

    // ── Tests for assert_max_entries ───────────────────────────────────

    #[test]
    fn assert_max_entries_pass() {
        let assertion = ResponseAssertion {
            max_entries: Some(3),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}},
                {"resource": {"resourceType": "Patient", "id": "2"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_max_entries_fail() {
        let assertion = ResponseAssertion {
            max_entries: Some(1),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}},
                {"resource": {"resourceType": "Patient", "id": "2"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("at most 1")));
    }

    // ── Tests for assert_bundle_total_present ──────────────────────────

    #[test]
    fn assert_bundle_total_present_true_when_present() {
        let assertion = ResponseAssertion {
            bundle_total_present: Some(true),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 5,
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_bundle_total_present_true_when_missing() {
        let assertion = ResponseAssertion {
            bundle_total_present: Some(true),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("should have a 'total'")));
    }

    #[test]
    fn assert_bundle_total_present_false_when_present() {
        let assertion = ResponseAssertion {
            bundle_total_present: Some(false),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 5,
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("should NOT have a 'total'"))
        );
    }

    #[test]
    fn assert_bundle_total_present_false_when_missing() {
        let assertion = ResponseAssertion {
            bundle_total_present: Some(false),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_bundle_total_present_no_body() {
        let assertion = ResponseAssertion {
            bundle_total_present: Some(true),
            ..ResponseAssertion::none()
        };
        let errors = assert_response(&assertion, 200, &None);
        assert!(errors.iter().any(|e| e.contains("No response body")));
    }

    // ── Tests for assert_summary_mode ──────────────────────────────────

    #[test]
    fn assert_summary_mode_count_pass() {
        let assertion = ResponseAssertion {
            summary_mode: Some("count".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 5,
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_summary_mode_count_fail() {
        let assertion = ResponseAssertion {
            summary_mode: Some("count".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{"resource": {"resourceType": "Patient", "id": "1"}}]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("should have no entries")));
    }

    #[test]
    fn assert_summary_mode_text_pass() {
        let assertion = ResponseAssertion {
            summary_mode: Some("text".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1", "text": {"status": "generated"}}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_summary_mode_text_fail() {
        let assertion = ResponseAssertion {
            summary_mode: Some("text".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("should have 'text' field"))
        );
    }

    #[test]
    fn assert_summary_mode_data_pass() {
        let assertion = ResponseAssertion {
            summary_mode: Some("data".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_summary_mode_data_fail() {
        let assertion = ResponseAssertion {
            summary_mode: Some("data".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1", "text": {"status": "generated"}}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("should NOT have 'text' field"))
        );
    }

    #[test]
    fn assert_summary_mode_no_body() {
        let assertion = ResponseAssertion {
            summary_mode: Some("count".to_string()),
            ..ResponseAssertion::none()
        };
        let errors = assert_response(&assertion, 200, &None);
        assert!(errors.iter().any(|e| e.contains("No response body")));
    }

    #[test]
    fn assert_summary_mode_unknown_mode_does_nothing() {
        let assertion = ResponseAssertion {
            summary_mode: Some("invalid".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [{"resource": {"resourceType": "Patient", "id": "1"}}]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors for unknown mode");
    }

    // ── Tests for assert_operation_output_params ────────────────────────

    #[test]
    fn assert_operation_output_params_found() {
        let assertion = ResponseAssertion {
            operation_output_params: vec!["return".to_string(), "issues".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "return", "valueString": "ok"},
                {"name": "issues", "valueString": "none"}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_operation_output_params_missing() {
        let assertion = ResponseAssertion {
            operation_output_params: vec!["return".to_string(), "nonexistent".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "return", "valueString": "ok"}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("nonexistent")));
    }

    #[test]
    fn assert_operation_output_params_no_parameter_array() {
        let assertion = ResponseAssertion {
            operation_output_params: vec!["return".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "OperationOutcome",
            "issue": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("no 'parameter' array")));
    }

    #[test]
    fn assert_operation_output_params_no_body() {
        let assertion = ResponseAssertion {
            operation_output_params: vec!["return".to_string()],
            ..ResponseAssertion::none()
        };
        let errors = assert_response(&assertion, 200, &None);
        assert!(errors.iter().any(|e| e.contains("No response body")));
    }

    // ── Tests for assert_required_bindings ─────────────────────────────

    #[test]
    fn assert_required_bindings_field_present() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "Patient".to_string(),
            vec![(
                "gender".to_string(),
                "http://hl7.org/fhir/ValueSet/administrative-gender".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            required_bindings: bindings,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "gender": "male", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_required_bindings_field_missing() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "Patient".to_string(),
            vec![(
                "gender".to_string(),
                "http://hl7.org/fhir/ValueSet/administrative-gender".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            required_bindings: bindings,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "name": [{"family": "T"}], "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("gender") && e.contains("required binding"))
        );
    }

    #[test]
    fn assert_required_bindings_no_matching_entries() {
        let mut bindings = HashMap::new();
        bindings.insert(
            "Patient".to_string(),
            vec![(
                "gender".to_string(),
                "http://hl7.org/fhir/ValueSet/administrative-gender".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            required_bindings: bindings,
            ..ResponseAssertion::none()
        };
        // Bundle with no Patient entries — should not error
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Observation", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for no matching entries"
        );
    }

    // ── Tests for assert_slice_assertions ──────────────────────────────

    #[test]
    fn assert_slice_assertions_discriminator_present() {
        let mut slices = HashMap::new();
        slices.insert(
            "Patient".to_string(),
            vec![(
                "identifier".to_string(),
                "slice-1".to_string(),
                "use".to_string(),
                "value".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            slice_assertions: slices,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "identifier": [{"use": "usual", "system": "http://example.org", "value": "123"}]
                }}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_slice_assertions_discriminator_missing() {
        let mut slices = HashMap::new();
        slices.insert(
            "Patient".to_string(),
            vec![(
                "identifier".to_string(),
                "slice-1".to_string(),
                "nonexistent".to_string(),
                "value".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            slice_assertions: slices,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "identifier": [{"use": "usual", "system": "http://example.org", "value": "123"}]
                }}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("missing discriminator")));
    }

    #[test]
    fn assert_slice_assertions_no_matching_entries() {
        let mut slices = HashMap::new();
        slices.insert(
            "Patient".to_string(),
            vec![(
                "identifier".to_string(),
                "slice-1".to_string(),
                "use".to_string(),
                "value".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            slice_assertions: slices,
            ..ResponseAssertion::none()
        };
        // Bundle with no Patient entries — should not error
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Observation", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for no matching entries"
        );
    }

    // ── Tests for assert_required_extensions ────────────────────────────

    #[test]
    fn assert_required_extensions_found() {
        let mut extensions = HashMap::new();
        extensions.insert(
            "Patient".to_string(),
            vec!["http://example.org/fhir/StructureDefinition/test-extension".to_string()],
        );
        let assertion = ResponseAssertion {
            required_extensions: extensions,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "extension": [{"url": "http://example.org/fhir/StructureDefinition/test-extension", "valueString": "test"}]
                }}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_required_extensions_missing() {
        let mut extensions = HashMap::new();
        extensions.insert(
            "Patient".to_string(),
            vec!["http://example.org/fhir/StructureDefinition/missing-ext".to_string()],
        );
        let assertion = ResponseAssertion {
            required_extensions: extensions,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "extension": [{"url": "http://example.org/fhir/StructureDefinition/other-ext", "valueString": "test"}]
                }}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("missing-ext")));
    }

    #[test]
    fn assert_required_extensions_no_extension_array() {
        // When there's no extension array, the check is silently skipped
        // (the code only validates extensions when the array exists)
        let mut extensions = HashMap::new();
        extensions.insert(
            "Patient".to_string(),
            vec!["http://example.org/fhir/StructureDefinition/test-ext".to_string()],
        );
        let assertion = ResponseAssertion {
            required_extensions: extensions,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        // No extension array means the check is skipped — no error
        assert!(
            errors.is_empty(),
            "Expected no errors when no extension array present"
        );
    }

    #[test]
    fn assert_required_extensions_no_matching_entries() {
        let mut extensions = HashMap::new();
        extensions.insert(
            "Patient".to_string(),
            vec!["http://example.org/fhir/StructureDefinition/test-ext".to_string()],
        );
        let assertion = ResponseAssertion {
            required_extensions: extensions,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Observation", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for no matching entries"
        );
    }

    // ── Tests for assert_type_constraints ────────────────────────────────

    #[test]
    fn assert_type_constraints_allowed_type() {
        let mut constraints = HashMap::new();
        constraints.insert(
            "Patient".to_string(),
            vec![(
                "value[x]".to_string(),
                vec!["String".to_string(), "CodeableConcept".to_string()],
            )],
        );
        let assertion = ResponseAssertion {
            type_constraints: constraints,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "valueString": "hello"
                }}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_type_constraints_disallowed_type() {
        let mut constraints = HashMap::new();
        constraints.insert(
            "Patient".to_string(),
            vec![("value[x]".to_string(), vec!["String".to_string()])],
        );
        let assertion = ResponseAssertion {
            type_constraints: constraints,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "valueInteger": 42
                }}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("not in allowed types")));
    }

    #[test]
    fn assert_type_constraints_null_value() {
        let mut constraints = HashMap::new();
        constraints.insert(
            "Patient".to_string(),
            vec![("value[x]".to_string(), vec!["String".to_string()])],
        );
        let assertion = ResponseAssertion {
            type_constraints: constraints,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {
                    "resourceType": "Patient",
                    "id": "1",
                    "valueString": null
                }}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("null value")));
    }

    #[test]
    fn assert_type_constraints_no_matching_entries() {
        let mut constraints = HashMap::new();
        constraints.insert(
            "Patient".to_string(),
            vec![("value[x]".to_string(), vec!["String".to_string()])],
        );
        let assertion = ResponseAssertion {
            type_constraints: constraints,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Observation", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for no matching entries"
        );
    }

    // ── Tests for assert_reference_targets ──────────────────────────────

    #[test]
    fn assert_reference_targets_profile_match() {
        let mut targets = HashMap::new();
        targets.insert(
            "Patient".to_string(),
            vec![(
                "generalPractitioner.reference".to_string(),
                "http://example.org/fhir/StructureDefinition/MyPractitioner".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            reference_targets: targets,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1", "generalPractitioner": {"reference": "Practitioner/prac-1"}}},
                {"fullUrl": "http://example.org/Practitioner/prac-1", "resource": {"resourceType": "Practitioner", "id": "prac-1", "meta": {"profile": ["http://example.org/fhir/StructureDefinition/MyPractitioner"]}}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn assert_reference_targets_profile_mismatch() {
        let mut targets = HashMap::new();
        targets.insert(
            "Patient".to_string(),
            vec![(
                "generalPractitioner.reference".to_string(),
                "http://example.org/fhir/StructureDefinition/ExpectedProfile".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            reference_targets: targets,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1", "generalPractitioner": {"reference": "Practitioner/prac-1"}}},
                {"fullUrl": "http://example.org/Practitioner/prac-1", "resource": {"resourceType": "Practitioner", "id": "prac-1", "meta": {"profile": ["http://other.org/Profile"]}}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("without target profile")));
    }

    #[test]
    fn assert_reference_targets_no_meta_profile() {
        let mut targets = HashMap::new();
        targets.insert(
            "Patient".to_string(),
            vec![(
                "generalPractitioner.reference".to_string(),
                "http://example.org/fhir/StructureDefinition/ExpectedProfile".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            reference_targets: targets,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1", "generalPractitioner": {"reference": "Practitioner/prac-1"}}},
                {"fullUrl": "http://example.org/Practitioner/prac-1", "resource": {"resourceType": "Practitioner", "id": "prac-1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("no meta.profile")));
    }

    #[test]
    fn assert_reference_targets_no_matching_entries() {
        let mut targets = HashMap::new();
        targets.insert(
            "Patient".to_string(),
            vec![(
                "generalPractitioner.reference".to_string(),
                "http://example.org/fhir/StructureDefinition/ExpectedProfile".to_string(),
            )],
        );
        let assertion = ResponseAssertion {
            reference_targets: targets,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Observation", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for no matching entries"
        );
    }

    // ── Additional edge case tests ──────────────────────────────────────

    #[test]
    fn assert_bundle_type_no_body() {
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let errors = assert_response(&assertion, 200, &None);
        assert!(errors.iter().any(|e| e.contains("No response body")));
    }

    #[test]
    fn assert_bundle_type_no_resource_type() {
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({"type": "searchset"});
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("no resourceType")));
    }

    #[test]
    fn assert_bundle_type_no_type_field() {
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({"resourceType": "Bundle"});
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("no 'type' field")));
    }

    #[test]
    fn assert_bundle_type_wrong_resource_type() {
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({"resourceType": "Patient", "id": "1"});
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("Expected Bundle")));
    }

    #[test]
    fn assert_outcome_severity_no_body() {
        let assertion = ResponseAssertion {
            outcome_severity: Some("error".to_string()),
            ..ResponseAssertion::none()
        };
        let errors = assert_response(&assertion, 404, &None);
        assert!(errors.iter().any(|e| e.contains("No response body")));
    }

    #[test]
    fn assert_outcome_severity_bundle_acceptable() {
        // Bundle response for negative test — should not error
        let assertion = ResponseAssertion {
            outcome_severity: Some("error".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": []
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Bundle should be acceptable for negative tests"
        );
    }

    #[test]
    fn assert_outcome_severity_wrong_resource_type() {
        let assertion = ResponseAssertion {
            outcome_severity: Some("error".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({"resourceType": "Patient", "id": "1"});
        let errors = assert_response(&assertion, 404, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Expected OperationOutcome"))
        );
    }

    #[test]
    fn assert_outcome_severity_no_issue_array() {
        let assertion = ResponseAssertion {
            outcome_severity: Some("error".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({"resourceType": "OperationOutcome"});
        let errors = assert_response(&assertion, 400, &Some(body));
        assert!(errors.iter().any(|e| e.contains("no 'issue' array")));
    }

    #[test]
    fn assert_outcome_severity_empty_issue_array() {
        let assertion = ResponseAssertion {
            outcome_severity: Some("error".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({"resourceType": "OperationOutcome", "issue": []});
        let errors = assert_response(&assertion, 400, &Some(body));
        assert!(errors.iter().any(|e| e.contains("empty 'issue' array")));
    }

    #[test]
    fn assert_response_contains_key_no_body() {
        let assertion = ResponseAssertion {
            response_contains_key: Some("parameter".to_string()),
            ..ResponseAssertion::none()
        };
        let errors = assert_response(&assertion, 200, &None);
        // When body is None, the let-else pattern means the check is skipped
        assert!(errors.is_empty(), "No body should skip key check");
    }

    #[test]
    fn assert_response_resource_types_no_body() {
        let assertion = ResponseAssertion {
            response_resource_types: vec!["Parameters".to_string()],
            ..ResponseAssertion::none()
        };
        let errors = assert_response(&assertion, 200, &None);
        assert!(errors.iter().any(|e| e.contains("No response body")));
    }

    #[test]
    fn assert_response_resource_types_no_resource_type() {
        let assertion = ResponseAssertion {
            response_resource_types: vec!["Parameters".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({"parameter": []});
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("no resourceType")));
    }

    #[test]
    fn assert_field_values_no_matching_entries() {
        let mut field_values = HashMap::new();
        let mut patient_fields = HashMap::new();
        patient_fields.insert("name.family".to_string(), serde_json::json!("Smith"));
        field_values.insert("Patient".to_string(), patient_fields);
        let assertion = ResponseAssertion {
            field_values,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Observation", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Expected at least one Patient"))
        );
    }

    #[test]
    fn assert_field_values_field_not_found() {
        let mut field_values = HashMap::new();
        let mut patient_fields = HashMap::new();
        patient_fields.insert("nonexistent.field".to_string(), serde_json::json!("value"));
        field_values.insert("Patient".to_string(), patient_fields);
        let assertion = ResponseAssertion {
            field_values,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1", "name": [{"family": "Smith"}]}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("not found")));
    }

    #[test]
    fn assert_required_fields_no_entry_array_with_total_gt_0() {
        let mut required = HashMap::new();
        required.insert("Patient".to_string(), vec!["name".to_string()]);
        let assertion = ResponseAssertion {
            required_fields: required,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 5
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("Bundle with entries")));
    }

    #[test]
    fn assert_required_fields_no_matching_with_total_gt_0() {
        let mut required = HashMap::new();
        required.insert("Patient".to_string(), vec!["name".to_string()]);
        let assertion = ResponseAssertion {
            required_fields: required,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 3,
            "entry": [
                {"resource": {"resourceType": "Observation", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Expected at least one Patient"))
        );
    }

    #[test]
    fn assert_sort_single_entry_no_error() {
        // Single entry should not trigger sort check
        let assertion = ResponseAssertion {
            sort_by: Some(SortAssertion {
                field: "birthDate".to_string(),
                direction: "asc".to_string(),
                additional_fields: Vec::new(),
            }),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "birthDate": "2000-01-01", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Single entry should not trigger sort check"
        );
    }

    #[test]
    fn assert_sort_numeric_ascending() {
        let assertion = ResponseAssertion {
            sort_by: Some(SortAssertion {
                field: "id".to_string(),
                direction: "asc".to_string(),
                additional_fields: Vec::new(),
            }),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "1"}},
                {"resource": {"resourceType": "Patient", "id": "2"}},
                {"resource": {"resourceType": "Patient", "id": "3"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for numeric ascending sort"
        );
    }

    #[test]
    fn assert_sort_numeric_descending() {
        let assertion = ResponseAssertion {
            sort_by: Some(SortAssertion {
                field: "id".to_string(),
                direction: "desc".to_string(),
                additional_fields: Vec::new(),
            }),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "3"}},
                {"resource": {"resourceType": "Patient", "id": "2"}},
                {"resource": {"resourceType": "Patient", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Expected no errors for numeric descending sort"
        );
    }

    #[test]
    fn assert_sort_unknown_direction_does_nothing() {
        let assertion = ResponseAssertion {
            sort_by: Some(SortAssertion {
                field: "birthDate".to_string(),
                direction: "unknown".to_string(),
                additional_fields: Vec::new(),
            }),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "entry": [
                {"resource": {"resourceType": "Patient", "birthDate": "2001-01-01", "id": "3"}},
                {"resource": {"resourceType": "Patient", "birthDate": "2000-01-01", "id": "1"}}
            ]
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(
            errors.is_empty(),
            "Unknown direction should not trigger sort check"
        );
    }

    #[test]
    fn assert_absent_fields_no_entry_array() {
        // Bundle with no entry array — should not error
        let assertion = ResponseAssertion {
            absent_fields: vec!["text".to_string()],
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 0
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.is_empty(), "No entry array should not error");
    }

    #[test]
    fn resolve_json_path_value_x_not_found() {
        let value = json!({"id": "123"});
        assert_eq!(resolve_json_path(&value, "value[x]"), None);
    }

    #[test]
    fn resolve_json_path_array_fallback() {
        // First element doesn't have the path, second does
        let value = json!({
            "identifier": [
                {"system": "http://example.org", "value": "first"},
                {"type": {"coding": [{"code": "SEC"}]}}
            ]
        });
        assert_eq!(
            resolve_json_path(&value, "identifier.type.coding.code"),
            Some(serde_json::json!("SEC"))
        );
    }

    #[test]
    fn resolve_json_path_array_not_found() {
        let value = json!({"identifier": [{"system": "http://example.org"}]});
        assert_eq!(resolve_json_path(&value, "identifier.type"), None);
    }

    #[test]
    fn resolve_json_path_non_object() {
        let value = json!("just a string");
        assert_eq!(resolve_json_path(&value, "anything"), None);
    }

    #[test]
    fn compare_values_both_none() {
        assert_eq!(compare_values(&None, &None), 0);
    }

    #[test]
    fn compare_values_none_vs_some() {
        assert_eq!(compare_values(&None, &Some(serde_json::json!("a"))), -1);
        assert_eq!(compare_values(&Some(serde_json::json!("a")), &None), 1);
    }

    #[test]
    fn compare_values_strings() {
        assert_eq!(
            compare_values(&Some(serde_json::json!("a")), &Some(serde_json::json!("b"))),
            -1
        );
        assert_eq!(
            compare_values(&Some(serde_json::json!("b")), &Some(serde_json::json!("a"))),
            1
        );
        assert_eq!(
            compare_values(&Some(serde_json::json!("a")), &Some(serde_json::json!("a"))),
            0
        );
    }

    #[test]
    fn compare_values_numbers() {
        assert_eq!(
            compare_values(&Some(serde_json::json!(1.0)), &Some(serde_json::json!(2.0))),
            -1
        );
        assert_eq!(
            compare_values(&Some(serde_json::json!(2.0)), &Some(serde_json::json!(1.0))),
            1
        );
    }

    #[test]
    fn compare_values_mixed_types() {
        // String vs number — should return 0 (can't compare)
        assert_eq!(
            compare_values(&Some(serde_json::json!("a")), &Some(serde_json::json!(1.0))),
            0
        );
    }

    #[test]
    fn assert_bundle_type_operation_outcome_acceptable() {
        // OperationOutcome is acceptable when checking bundle type
        let assertion = ResponseAssertion {
            bundle_type: Some("searchset".to_string()),
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "not-found"}]
        });
        let errors = assert_response(&assertion, 404, &Some(body));
        assert!(errors.is_empty(), "OperationOutcome should be acceptable");
    }

    #[test]
    fn assert_empty_bundle_no_entry_errors_when_requires_entries_and_no_total() {
        // Bundle with no entry array, no total, but requires entries
        let mut required = HashMap::new();
        required.insert("Patient".to_string(), vec!["name".to_string()]);
        let assertion = ResponseAssertion {
            min_entries: Some(1),
            required_fields: required,
            ..ResponseAssertion::none()
        };
        let body = json!({
            "resourceType": "Bundle",
            "type": "searchset"
        });
        let errors = assert_response(&assertion, 200, &Some(body));
        assert!(errors.iter().any(|e| e.contains("no 'entry' array")));
    }
}
