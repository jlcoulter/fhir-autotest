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
                        }
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
    fn extract_reference_id_works() {
        assert_eq!(extract_reference_id("Organization/org-1"), Some("org-1"));
        assert_eq!(
            extract_reference_id("Practitioner/prac-42"),
            Some("prac-42")
        );
        assert_eq!(extract_reference_id("no-slash"), None);
    }
}
