use rand::Rng;
use std::collections::HashMap;

/// Return true when the profile URLs in a loaded IG indicate an HCPD/AU implementation
/// guide, so that HCPD-specific identifier and extension overrides are applied only
/// when appropriate.
pub fn is_hcpd_ig(profile_urls: &HashMap<String, String>) -> bool {
    profile_urls.values().any(|url| {
        url.contains("digitalhealth.gov.au") || url.contains("/hcpd/") || url.contains("hl7.org.au")
    })
}

/// Apply HCPD/AU-specific overrides to a generated resource.
///
/// This function is ONLY called when `hcpd_ig` is true (i.e. the loaded IG package
/// is the HCPD/AU IG). For all other IGs, the profile-aware generator produces
/// conformant resources without needing IG-specific identifier augmentation.
pub fn apply_hcpd_bulk_fixes(
    resource: &mut serde_json::Value,
    resource_type: &str,
    id: &str,
    practitioner_registration_by_id: &mut HashMap<String, String>,
    value_set_systems: &HashMap<String, String>,
    code_system_codes: &HashMap<String, (String, Option<String>)>,
    rng: &mut impl Rng,
) {
    match resource_type {
        "Organization" => {
            resource["identifier"] = serde_json::json!([
                {
                    "system": "http://hl7.org.au/id/abn",
                    "type": {
                        "coding": [
                            {
                                "system": "http://terminology.hl7.org/CodeSystem/v2-0203",
                                "code": "TAX"
                            }
                        ]
                    },
                    "value": random_digits(11, rng)
                },
                {
                    "system": "http://ns.electronichealth.net.au/id/hi/hpio/1.0",
                    "type": {
                        "coding": [
                            {
                                "system": "http://terminology.hl7.org.au/CodeSystem/v2-0203",
                                "code": "NOI"
                            }
                        ]
                    },
                    "extension": [
                        {
                            "url": "http://digitalhealth.gov.au/fhir/hcpd/StructureDefinition/hi-org-classification",
                            "valueCodeableConcept": {
                                "coding": [
                                    {
                                        "system": "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/hi-org-classification-cs",
                                        "code": "seed",
                                        "display": "Seed"
                                    }
                                ]
                            }
                        },
                        hi_services_status_extension(code_system_codes)
                    ],
                    "value": luhn_with_prefix("800362", 16, rng)
                }
            ]);

            if resource.get("address").is_none() || !resource["address"].is_array() {
                resource["address"] = serde_json::json!([{}]);
            }
            if let Some(first_addr) = resource
                .get_mut("address")
                .and_then(|a| a.as_array_mut())
                .and_then(|a| a.first_mut())
            {
                if first_addr.get("type").is_none() {
                    first_addr["type"] = serde_json::Value::String("physical".to_string());
                }
                if first_addr.get("line").is_none() {
                    first_addr["line"] = serde_json::json!(["100 George St"]);
                }
                if first_addr.get("city").is_none() {
                    first_addr["city"] = serde_json::Value::String("Sydney".to_string());
                }
                if first_addr.get("state").is_none() {
                    first_addr["state"] = serde_json::Value::String("NSW".to_string());
                }
                if first_addr.get("postalCode").is_none() {
                    first_addr["postalCode"] = serde_json::Value::String("2000".to_string());
                }
                if first_addr.get("country").is_none() {
                    first_addr["country"] = serde_json::Value::String("AU".to_string());
                }
            }
        }
        "Practitioner" => {
            resource["identifier"] = serde_json::json!([
                {
                    "system": "http://ns.electronichealth.net.au/id/hi/hpii/1.0",
                    "type": {
                        "coding": [
                            {
                                "system": "http://terminology.hl7.org/CodeSystem/v2-0203",
                                "code": "NPI"
                            }
                        ]
                    },
                    "extension": [hi_services_status_extension(code_system_codes)],
                    "value": luhn_with_prefix("800361", 16, rng)
                }
            ]);

            let registration_number = format!("MED{}", random_digits(10, rng));
            resource["qualification"] = serde_json::json!([
                {
                    "code": {
                        "text": "General practice"
                    },
                    "identifier": [
                        {
                            "system": "http://hl7.org.au/id/ahpra-registration-number",
                            "type": {
                                "coding": [
                                    {
                                        "system": "http://terminology.hl7.org.au/CodeSystem/v2-0203",
                                        "code": "AHPRA"
                                    }
                                ]
                            },
                            "value": registration_number
                        }
                    ],
                    "issuer": {
                        "reference": "Organization/organization-1"
                    }
                }
            ]);

            resource["extension"] = serde_json::json!([
                {
                    "url": "http://hl7.org/fhir/StructureDefinition/individual-recordedSexOrGender",
                    "extension": [
                        {
                            "url": "value",
                            "valueCodeableConcept": {
                                "coding": [
                                    {
                                        "system": "http://hl7.org/fhir/administrative-gender",
                                        "code": "male",
                                        "display": "Male"
                                    }
                                ]
                            }
                        }
                    ]
                }
            ]);

            practitioner_registration_by_id.insert(id.to_string(), registration_number);
        }
        "HealthcareService" => {
            if resource.get("type").is_none() || !resource["type"].is_array() {
                resource["type"] = serde_json::json!([{}]);
            }
            if let Some(first_type) = resource
                .get_mut("type")
                .and_then(|a| a.as_array_mut())
                .and_then(|a| a.first_mut())
            {
                // Always set a valid SNOMED coding with display — the HCPD profile
                // requires type.coding.display (min = 1).
                first_type["coding"] = serde_json::json!([
                    {
                        "system": "http://snomed.info/sct",
                        "code": "408443003",
                        "display": "General medical practice"
                    }
                ]);
            }

            // Fix suppressedBy extension coding — the HCPD profile requires a code
            // from the responsible-party-type ValueSet, not NullFlavor.
            fix_suppressed_by_coding(resource, value_set_systems, code_system_codes);

            // Fix serviceProvisionCode — the profile-aware generator uses code
            // "unknown" which doesn't exist in the HCPD service-provision CodeSystem.
            // Replace with the first valid code from the CodeSystem.
            fix_service_provision_code(resource, code_system_codes);
        }
        "Location" => {
            resource["type"] = serde_json::json!([
                {
                    "text": "Healthcare service location"
                }
            ]);
        }
        "PractitionerRole" => {
            let practitioner_id = resource
                .get("practitioner")
                .and_then(|p| p.get("reference"))
                .and_then(|r| r.as_str())
                .and_then(extract_reference_id);

            let registration_number = practitioner_id
                .and_then(|pid| practitioner_registration_by_id.get(pid).cloned())
                .unwrap_or_else(|| format!("MED{}", random_digits(10, rng)));

            resource["identifier"] = serde_json::json!([
                {
                    "system": "http://digitalhealth.gov.au/fhir/hcpd/id/hcpd-local-identifier",
                    "type": {
                        "coding": [
                            {
                                "system": "http://terminology.hl7.org/CodeSystem/v2-0203",
                                "code": "XX"
                            }
                        ]
                    },
                    "value": random_digits(12, rng)
                },
                {
                    "system": "http://digitalhealth.gov.au/fhir/hcpd/id/hcpd-source-identifier",
                    "type": {
                        "coding": [
                            {
                                "system": "http://terminology.hl7.org/CodeSystem/v2-0203",
                                "code": "RI"
                            }
                        ]
                    },
                    "value": random_digits(12, rng)
                },
                {
                    "system": "http://hl7.org.au/id/ahpra-registration-number",
                    "type": {
                        "coding": [
                            {
                                "system": "http://terminology.hl7.org.au/CodeSystem/v2-0203",
                                "code": "AHPRA"
                            }
                        ]
                    },
                    "value": registration_number
                }
            ]);

            // Fix suppressedBy extension coding — the HCPD profile requires a code
            // from the responsible-party-type ValueSet, not NullFlavor.
            fix_suppressed_by_coding(resource, value_set_systems, code_system_codes);
        }
        _ => {}
    }
}

fn extract_reference_id(reference: &str) -> Option<&str> {
    reference.split_once('/').map(|(_, id)| id)
}

/// Build the required `hi-services-identifier-status` extension shared by the
/// HI-services identifier profiles (HPII/HPIO/HSPO), which each require a
/// `hi-services-identifier-status` slice with min = 1.
///
/// The status code is resolved from the hi-services-identifier-status
/// CodeSystem via `code_system_codes`, defaulting to "A" (Active).
fn hi_services_status_extension(
    code_system_codes: &HashMap<String, (String, Option<String>)>,
) -> serde_json::Value {
    let system =
        "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/hi-services-identifier-status-cs";
    let (code, display) = code_system_codes
        .get(system)
        .map(|(c, d)| (c.clone(), d.clone().unwrap_or_else(|| c.clone())))
        .unwrap_or_else(|| ("A".to_string(), "Active".to_string()));
    serde_json::json!({
        "url": "http://digitalhealth.gov.au/fhir/hcpd/StructureDefinition/hi-services-identifier-status",
        "valueCoding": {
            "system": system,
            "code": code,
            "display": display
        }
    })
}

/// Fix the `suppressedBy.valueCodeableConcept.coding` in the `suppressed` extension
/// to use a valid code from the responsible-party-type CodeSystem.
///
/// The code is looked up from `value_set_systems` (ValueSet URL → system URL) and
/// `code_system_codes` (system URL → first valid code), falling back to `"UNK"` if
/// neither map contains the relevant entries. This avoids any hardcoded HCPD codes.
fn fix_suppressed_by_coding(
    resource: &mut serde_json::Value,
    value_set_systems: &HashMap<String, String>,
    code_system_codes: &HashMap<String, (String, Option<String>)>,
) {
    // Find the system URL bound to the suppressedBy coding
    // (typically via responsible-party-type ValueSet in HCPD)
    let vs_url = "http://digitalhealth.gov.au/fhir/cc/ValueSet/responsible-party-type";
    let system = value_set_systems
        .get(vs_url)
        .map(|s| s.as_str())
        .unwrap_or("http://digitalhealth.gov.au/fhir/cc/CodeSystem/responsible-party-type");
    let (code, display) = code_system_codes
        .get(system)
        .map(|(c, d)| (c.as_str(), d.as_deref().unwrap_or(c.as_str())))
        .unwrap_or(("UNK", "Unknown"));

    let Some(exts) = resource.get_mut("extension").and_then(|e| e.as_array_mut()) else {
        return;
    };
    for ext in exts {
        let url = ext.get("url").and_then(|u| u.as_str()).unwrap_or("");
        if !url.contains("suppressed") {
            continue;
        }
        let Some(sub_exts) = ext.get_mut("extension").and_then(|e| e.as_array_mut()) else {
            continue;
        };
        for sub_ext in sub_exts.iter_mut() {
            let sub_url = sub_ext.get("url").and_then(|u| u.as_str()).unwrap_or("");
            if sub_url == "suppressedBy" {
                // Only override if the current coding is the generic NullFlavor fallback.
                // When populate_extension_slices has already applied a fixedCoding from the
                // profile (e.g. organisation-initiated for Organization/HealthcareService),
                // leave it intact.
                let already_valid = sub_ext
                    .get("valueCodeableConcept")
                    .and_then(|v| v.get("coding"))
                    .and_then(|c| c.as_array())
                    .and_then(|a| a.first())
                    .and_then(|c| c.get("system"))
                    .and_then(|s| s.as_str())
                    .map(|s| !s.contains("NullFlavor"))
                    .unwrap_or(false);
                if already_valid {
                    continue;
                }
                sub_ext["valueCodeableConcept"] = serde_json::json!({
                    "coding": [{
                        "system": system,
                        "code": code
                    }],
                    "text": display
                });
            }
        }
    }
}

/// Fix `serviceProvisionCode` on a HealthcareService resource.
///
/// The profile-aware generator uses code "unknown" which doesn't exist in the
/// HCPD service-provision CodeSystem. Replace it with the first valid code
/// from the CodeSystem, falling back to a hardcoded valid code if the map
/// doesn't contain the system.
fn fix_service_provision_code(
    resource: &mut serde_json::Value,
    code_system_codes: &HashMap<String, (String, Option<String>)>,
) {
    let system = "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/service-provision-cs";
    let (code, display_str) = code_system_codes
        .get(system)
        .map(|(c, d)| {
            let code: &str = c.as_str();
            let display: &str = d.as_deref().unwrap_or(c.as_str());
            (code.to_string(), display.to_string())
        })
        .unwrap_or(("inperson".to_string(), "In person".to_string()));

    let Some(spc) = resource
        .get_mut("serviceProvisionCode")
        .and_then(|v| v.as_array_mut())
    else {
        return;
    };
    for entry in spc.iter_mut() {
        let Some(codings) = entry.get_mut("coding").and_then(|c| c.as_array_mut()) else {
            continue;
        };
        for coding in codings.iter_mut() {
            let current_code = coding.get("code").and_then(|c| c.as_str());
            if current_code == Some("unknown") || current_code == Some("UNK") {
                coding["code"] = serde_json::Value::String(code.clone());
                coding["display"] = serde_json::Value::String(display_str.clone());
            }
        }
    }
}

fn random_digits(len: usize, rng: &mut impl Rng) -> String {
    let mut out = String::with_capacity(len);
    for i in 0..len {
        let d: u8 = if i == 0 {
            rng.random_range(1..10)
        } else {
            rng.random_range(0..10)
        };
        out.push(char::from(b'0' + d));
    }
    out
}

fn luhn_with_prefix(prefix: &str, total_len: usize, rng: &mut impl Rng) -> String {
    let payload_len = total_len.saturating_sub(1);
    let mut base = prefix.to_string();
    while base.len() < payload_len {
        base.push(char::from(b'0' + rng.random_range(0..10)));
    }
    base.truncate(payload_len);

    let mut sum = 0u32;
    let mut double = true;
    for ch in base.chars().rev() {
        let mut n = ch.to_digit(10).unwrap_or(0);
        if double {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        sum += n;
        double = !double;
    }
    let check = (10 - (sum % 10)) % 10;
    format!("{}{}", base, check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn is_hcpd_ig_detects_digitalhealth_gov_au() {
        let mut urls = HashMap::new();
        urls.insert(
            "Organization".to_string(),
            "http://digitalhealth.gov.au/fhir/hcpd/StructureDefinition/hcpd-Organization"
                .to_string(),
        );
        assert!(is_hcpd_ig(&urls));
    }

    #[test]
    fn is_hcpd_ig_detects_hl7_org_au() {
        let mut urls = HashMap::new();
        urls.insert(
            "Practitioner".to_string(),
            "http://hl7.org.au/fhir/StructureDefinition/au-practitioner".to_string(),
        );
        assert!(is_hcpd_ig(&urls));
    }

    #[test]
    fn is_hcpd_ig_returns_false_for_non_au_igs() {
        let mut urls = HashMap::new();
        urls.insert(
            "Organization".to_string(),
            "http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Organization"
                .to_string(),
        );
        assert!(!is_hcpd_ig(&urls));
    }

    #[test]
    fn is_hcpd_ig_returns_false_for_empty() {
        let urls = HashMap::new();
        assert!(!is_hcpd_ig(&urls));
    }

    #[test]
    fn random_digits_produces_correct_length() {
        let mut rng = rand::rng();
        let s = random_digits(11, &mut rng);
        assert_eq!(s.len(), 11);
        for (i, c) in s.chars().enumerate() {
            assert!(c.is_ascii_digit(), "char at {} should be a digit", i);
        }
        // First digit should not be zero
        assert_ne!(s.chars().next().unwrap(), '0');
    }

    #[test]
    fn random_digits_zero_length() {
        let mut rng = rand::rng();
        let s = random_digits(0, &mut rng);
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn luhn_with_prefix_produces_valid_checksum() {
        let mut rng = rand::rng();
        let s = luhn_with_prefix("800362", 16, &mut rng);
        assert_eq!(s.len(), 16);
        assert!(s.starts_with("800362"));

        // Verify Luhn checksum: start from the rightmost digit (the check digit)
        // and double every second digit to the left. The check digit itself is
        // NOT doubled — it was computed so that the sum of all digits (with
        // doubling applied to every second digit starting from the second-to-last)
        // is divisible by 10.
        let mut sum = 0u32;
        let mut double = false; // rightmost digit (check digit) is not doubled
        for ch in s.chars().rev() {
            let mut n = ch.to_digit(10).unwrap_or(0);
            if double {
                n *= 2;
                if n > 9 {
                    n -= 9;
                }
            }
            sum += n;
            double = !double;
        }
        assert_eq!(sum % 10, 0, "Luhn checksum should be valid");
    }

    #[test]
    fn luhn_with_prefix_exact_prefix_length() {
        let mut rng = rand::rng();
        // total_len = prefix.len() + 1 (check digit only)
        let s = luhn_with_prefix("800362", 7, &mut rng);
        assert_eq!(s.len(), 7);
        assert!(s.starts_with("800362"));
    }

    #[test]
    fn luhn_with_prefix_prefix_longer_than_payload() {
        let mut rng = rand::rng();
        let s = luhn_with_prefix("8003621567890", 10, &mut rng);
        assert_eq!(s.len(), 10);
        // Should be truncated to payload_len
    }

    #[test]
    fn extract_reference_id_works() {
        assert_eq!(extract_reference_id("Organization/org-1"), Some("org-1"));
        assert_eq!(
            extract_reference_id("Practitioner/prac-42"),
            Some("prac-42")
        );
        assert_eq!(extract_reference_id("no-slash"), None);
    }

    // ── apply_hcpd_bulk_fixes tests ──────────────────────────────────────

    #[test]
    fn apply_hcpd_bulk_fixes_organization() {
        let mut resource = json!({"resourceType": "Organization"});
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "Organization",
            "org-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Should have identifier array
        assert!(resource["identifier"].is_array());
        let identifiers = resource["identifier"].as_array().unwrap();
        assert_eq!(identifiers.len(), 2);
        // First identifier should be ABN
        assert_eq!(identifiers[0]["system"], "http://hl7.org.au/id/abn");
        assert_eq!(identifiers[0]["type"]["coding"][0]["code"], "TAX");
        // Second identifier should be HPIO
        assert_eq!(
            identifiers[1]["system"],
            "http://ns.electronichealth.net.au/id/hi/hpio/1.0"
        );
        // Should have address with AU defaults
        assert!(resource["address"].is_array());
        let addr = &resource["address"][0];
        assert_eq!(addr["type"], "physical");
        assert_eq!(addr["city"], "Sydney");
        assert_eq!(addr["country"], "AU");
    }

    #[test]
    fn apply_hcpd_bulk_fixes_organization_preserves_existing_address() {
        let mut resource = json!({
            "resourceType": "Organization",
            "address": [{
                "type": "postal",
                "line": ["PO Box 123"],
                "city": "Melbourne",
                "state": "VIC",
                "postalCode": "3000",
                "country": "AU"
            }]
        });
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "Organization",
            "org-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Existing address fields should be preserved
        let addr = &resource["address"][0];
        assert_eq!(addr["type"], "postal");
        assert_eq!(addr["city"], "Melbourne");
        assert_eq!(addr["state"], "VIC");
    }

    #[test]
    fn apply_hcpd_bulk_fixes_practitioner() {
        let mut resource = json!({"resourceType": "Practitioner"});
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "Practitioner",
            "prac-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Should have identifier with HPI-I
        assert!(resource["identifier"].is_array());
        assert_eq!(
            resource["identifier"][0]["system"],
            "http://ns.electronichealth.net.au/id/hi/hpii/1.0"
        );
        // Should have qualification
        assert!(resource["qualification"].is_array());
        assert_eq!(
            resource["qualification"][0]["code"]["text"],
            "General practice"
        );
        // Should have extension (recordedSexOrGender)
        assert!(resource["extension"].is_array());
        assert_eq!(
            resource["extension"][0]["url"],
            "http://hl7.org/fhir/StructureDefinition/individual-recordedSexOrGender"
        );
        // Registration number should be stored
        assert!(reg.contains_key("prac-1"));
    }

    #[test]
    fn apply_hcpd_bulk_fixes_healthcare_service() {
        let mut resource = json!({"resourceType": "HealthcareService"});
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "HealthcareService",
            "hs-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Should have type with SNOMED coding
        assert!(resource["type"].is_array());
        assert_eq!(
            resource["type"][0]["coding"][0]["system"],
            "http://snomed.info/sct"
        );
        assert_eq!(resource["type"][0]["coding"][0]["code"], "408443003");
    }

    #[test]
    fn apply_hcpd_bulk_fixes_healthcare_service_preserves_existing_type() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "type": [{"text": "Existing type"}]
        });
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "HealthcareService",
            "hs-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Should still have the SNOMED coding added
        assert_eq!(resource["type"][0]["coding"][0]["code"], "408443003");
    }

    #[test]
    fn apply_hcpd_bulk_fixes_location() {
        let mut resource = json!({"resourceType": "Location"});
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "Location",
            "loc-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Should have type with text
        assert!(resource["type"].is_array());
        assert_eq!(resource["type"][0]["text"], "Healthcare service location");
    }

    #[test]
    fn apply_hcpd_bulk_fixes_practitioner_role() {
        let mut resource = json!({
            "resourceType": "PractitionerRole",
            "practitioner": {"reference": "Practitioner/prac-1"}
        });
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        reg.insert("prac-1".to_string(), "MED1234567890".to_string());
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "PractitionerRole",
            "pr-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Should have identifier with AHPRA registration number
        assert!(resource["identifier"].is_array());
        let identifiers = resource["identifier"].as_array().unwrap();
        assert_eq!(identifiers.len(), 3);
        // First identifier should be local identifier
        assert_eq!(
            identifiers[0]["system"],
            "http://digitalhealth.gov.au/fhir/hcpd/id/hcpd-local-identifier"
        );
        // Second identifier should be the source identifier
        assert_eq!(
            identifiers[1]["system"],
            "http://digitalhealth.gov.au/fhir/hcpd/id/hcpd-source-identifier"
        );
        // Third identifier should be AHPRA registration
        assert_eq!(
            identifiers[2]["system"],
            "http://hl7.org.au/id/ahpra-registration-number"
        );
        assert_eq!(identifiers[2]["value"], "MED1234567890");
    }

    #[test]
    fn apply_hcpd_bulk_fixes_practitioner_role_no_practitioner_ref() {
        let mut resource = json!({"resourceType": "PractitionerRole"});
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "PractitionerRole",
            "pr-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Should still have identifiers (with random registration number)
        assert!(resource["identifier"].is_array());
        let identifiers = resource["identifier"].as_array().unwrap();
        assert_eq!(identifiers.len(), 3);
        // Registration number should be random (not from reg map since no practitioner ref)
        assert!(identifiers[2]["value"].as_str().unwrap().starts_with("MED"));
    }

    #[test]
    fn apply_hcpd_bulk_fixes_unknown_type() {
        let mut resource = json!({"resourceType": "Unknown"});
        let mut rng = rand::rng();
        let mut reg = HashMap::new();
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        apply_hcpd_bulk_fixes(
            &mut resource,
            "Unknown",
            "unk-1",
            &mut reg,
            &vs_systems,
            &cs_codes,
            &mut rng,
        );

        // Should not modify anything for unknown type
        assert_eq!(resource.get("identifier"), None);
        assert_eq!(resource.get("type"), None);
    }

    // ── fix_suppressed_by_coding tests ──────────────────────────────────

    #[test]
    fn fix_suppressed_by_coding_no_extension() {
        let mut resource = json!({"resourceType": "HealthcareService"});
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        fix_suppressed_by_coding(&mut resource, &vs_systems, &cs_codes);
        // Should not crash, no extension added
        assert!(resource.get("extension").is_none());
    }

    #[test]
    fn fix_suppressed_by_coding_no_suppressed_extension() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "extension": [{
                "url": "http://example.org/extension/other",
                "extension": [{
                    "url": "suppressedBy",
                    "valueCodeableConcept": {
                        "coding": [{"system": "http://hl7.org/fhir/ValueSet/NullFlavor", "code": "UNK"}]
                    }
                }]
            }]
        });
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        fix_suppressed_by_coding(&mut resource, &vs_systems, &cs_codes);
        // Should not modify non-suppressed extensions
        let coding = &resource["extension"][0]["extension"][0]["valueCodeableConcept"]["coding"][0];
        assert_eq!(coding["code"], "UNK");
    }

    #[test]
    fn fix_suppressed_by_coding_with_suppressed_extension() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "extension": [{
                "url": "http://hl7.org/fhir/StructureDefinition/healthcareservice-suppressed",
                "extension": [{
                    "url": "suppressedBy",
                    "valueCodeableConcept": {
                        "coding": [{"system": "http://hl7.org/fhir/ValueSet/NullFlavor", "code": "UNK"}]
                    }
                }]
            }]
        });
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        fix_suppressed_by_coding(&mut resource, &vs_systems, &cs_codes);
        // Should replace with fallback UNK
        let coding = &resource["extension"][0]["extension"][0]["valueCodeableConcept"]["coding"][0];
        assert_eq!(coding["code"], "UNK");
        assert_eq!(
            coding["system"],
            "http://digitalhealth.gov.au/fhir/cc/CodeSystem/responsible-party-type"
        );
    }

    #[test]
    fn fix_suppressed_by_coding_with_valid_coding_skipped() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "extension": [{
                "url": "http://hl7.org/fhir/StructureDefinition/healthcareservice-suppressed",
                "extension": [{
                    "url": "suppressedBy",
                    "valueCodeableConcept": {
                        "coding": [{"system": "http://example.org/valid", "code": "valid-code"}]
                    }
                }]
            }]
        });
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        fix_suppressed_by_coding(&mut resource, &vs_systems, &cs_codes);
        // Should NOT replace because the coding is already valid (not NullFlavor)
        let coding = &resource["extension"][0]["extension"][0]["valueCodeableConcept"]["coding"][0];
        assert_eq!(coding["code"], "valid-code");
        assert_eq!(coding["system"], "http://example.org/valid");
    }

    #[test]
    fn fix_suppressed_by_coding_with_value_set_system() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "extension": [{
                "url": "http://hl7.org/fhir/StructureDefinition/healthcareservice-suppressed",
                "extension": [{
                    "url": "suppressedBy",
                    "valueCodeableConcept": {
                        "coding": [{"system": "http://hl7.org/fhir/ValueSet/NullFlavor", "code": "UNK"}]
                    }
                }]
            }]
        });
        let mut vs_systems = HashMap::new();
        vs_systems.insert(
            "http://digitalhealth.gov.au/fhir/cc/ValueSet/responsible-party-type".to_string(),
            "http://example.org/cs/responsible-party".to_string(),
        );
        let mut cs_codes = HashMap::new();
        cs_codes.insert(
            "http://example.org/cs/responsible-party".to_string(),
            (
                "org-init".to_string(),
                Some("Organisation initiated".to_string()),
            ),
        );

        fix_suppressed_by_coding(&mut resource, &vs_systems, &cs_codes);
        // Should use the value set system and code
        let coding = &resource["extension"][0]["extension"][0]["valueCodeableConcept"]["coding"][0];
        assert_eq!(coding["system"], "http://example.org/cs/responsible-party");
        assert_eq!(coding["code"], "org-init");
    }

    #[test]
    fn fix_suppressed_by_coding_no_sub_extensions() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "extension": [{
                "url": "http://hl7.org/fhir/StructureDefinition/healthcareservice-suppressed"
            }]
        });
        let vs_systems = HashMap::new();
        let cs_codes = HashMap::new();

        fix_suppressed_by_coding(&mut resource, &vs_systems, &cs_codes);
        // Should not crash — extension has no sub-extensions
        assert!(resource["extension"][0].get("extension").is_none());
    }

    // ── fix_service_provision_code tests ────────────────────────────────

    #[test]
    fn fix_service_provision_code_no_service_provision_code() {
        let mut resource = json!({"resourceType": "HealthcareService"});
        let cs_codes = HashMap::new();

        fix_service_provision_code(&mut resource, &cs_codes);
        // Should not crash
        assert!(resource.get("serviceProvisionCode").is_none());
    }

    #[test]
    fn fix_service_provision_code_with_unknown_code() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "serviceProvisionCode": [{
                "coding": [{"system": "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/service-provision-cs", "code": "unknown"}]
            }]
        });
        let cs_codes = HashMap::new();

        fix_service_provision_code(&mut resource, &cs_codes);
        // Should replace with fallback "inperson"
        assert_eq!(
            resource["serviceProvisionCode"][0]["coding"][0]["code"],
            "inperson"
        );
        assert_eq!(
            resource["serviceProvisionCode"][0]["coding"][0]["display"],
            "In person"
        );
    }

    #[test]
    fn fix_service_provision_code_with_unk_code() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "serviceProvisionCode": [{
                "coding": [{"system": "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/service-provision-cs", "code": "UNK"}]
            }]
        });
        let cs_codes = HashMap::new();

        fix_service_provision_code(&mut resource, &cs_codes);
        // Should replace with fallback "inperson"
        assert_eq!(
            resource["serviceProvisionCode"][0]["coding"][0]["code"],
            "inperson"
        );
    }

    #[test]
    fn fix_service_provision_code_with_valid_code() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "serviceProvisionCode": [{
                "coding": [{"system": "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/service-provision-cs", "code": "inperson"}]
            }]
        });
        let cs_codes = HashMap::new();

        fix_service_provision_code(&mut resource, &cs_codes);
        // Should NOT replace valid code
        assert_eq!(
            resource["serviceProvisionCode"][0]["coding"][0]["code"],
            "inperson"
        );
    }

    #[test]
    fn fix_service_provision_code_with_code_system_codes() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "serviceProvisionCode": [{
                "coding": [{"system": "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/service-provision-cs", "code": "unknown"}]
            }]
        });
        let mut cs_codes = HashMap::new();
        cs_codes.insert(
            "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/service-provision-cs".to_string(),
            ("telehealth".to_string(), Some("Telehealth".to_string())),
        );

        fix_service_provision_code(&mut resource, &cs_codes);
        // Should use the code from the map
        assert_eq!(
            resource["serviceProvisionCode"][0]["coding"][0]["code"],
            "telehealth"
        );
        assert_eq!(
            resource["serviceProvisionCode"][0]["coding"][0]["display"],
            "Telehealth"
        );
    }

    #[test]
    fn fix_service_provision_code_no_coding_array() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "serviceProvisionCode": [{}]
        });
        let cs_codes = HashMap::new();

        fix_service_provision_code(&mut resource, &cs_codes);
        // Should not crash — no coding array
        assert!(resource["serviceProvisionCode"][0].get("coding").is_none());
    }

    #[test]
    fn fix_service_provision_code_multiple_codings() {
        let mut resource = json!({
            "resourceType": "HealthcareService",
            "serviceProvisionCode": [{
                "coding": [
                    {"system": "http://digitalhealth.gov.au/fhir/hcpd/CodeSystem/service-provision-cs", "code": "unknown"},
                    {"system": "http://example.org/other", "code": "valid"}
                ]
            }]
        });
        let cs_codes = HashMap::new();

        fix_service_provision_code(&mut resource, &cs_codes);
        // First coding should be replaced, second should stay
        assert_eq!(
            resource["serviceProvisionCode"][0]["coding"][0]["code"],
            "inperson"
        );
        assert_eq!(
            resource["serviceProvisionCode"][0]["coding"][1]["code"],
            "valid"
        );
    }
}
