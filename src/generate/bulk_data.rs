use crate::generate::resource_generator::generate_resource;
use crate::model::profile::StructureDefinition;
use anyhow::Result;
use fake::Fake;
use rand::Rng;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// IDs allocated during bulk generation.
/// Maps resource type → list of generated IDs.
pub type IdStore = HashMap<String, Vec<String>>;

/// Generate bulk FHIR resources as NDJSON files.
///
/// Writes one `.ndjson` file per resource type under `output_dir/data/`.
/// Returns an `IdStore` mapping each resource type to its generated IDs,
/// which is used to resolve cross-references during generation.
pub fn generate_bulk_data(
    counts: &HashMap<String, u64>,
    profile_urls: &HashMap<String, String>,
    profiles: &[StructureDefinition],
    output_dir: &Path,
) -> Result<IdStore> {
    use std::io::BufWriter;

    let data_dir = output_dir.join("data");
    std::fs::create_dir_all(&data_dir)?;
    let mut rng = rand::rng();

    // Determine creation order: dependent types first.
    // Organizations and Locations have no FHIR references, so they go first.
    // Practitioners reference nothing. PractitionerRoles and HealthcareServices
    // reference Practitioners, Organizations, and Locations.
    let order = bulk_data_creation_order(counts);

    // First pass: allocate all IDs so cross-references can be resolved.
    let mut ids: IdStore = HashMap::new();
    for resource_type in &order {
        let count = counts.get(resource_type).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let type_ids: Vec<String> = (0..count)
            .map(|i| format!("{}-{}", resource_type.to_lowercase(), i + 1))
            .collect();
        ids.insert(resource_type.clone(), type_ids);
    }

    // Pre-clone ID vectors for cross-referencing (avoids cloning inside hot loops).
    let org_ids = ids.get("Organization").cloned().unwrap_or_default();
    let prac_ids = ids.get("Practitioner").cloned().unwrap_or_default();
    let loc_ids = ids.get("Location").cloned().unwrap_or_default();

    // Build a lookup: resource_type → StructureDefinition so we can use
    // profile-aware generation when available.
    let profile_map: HashMap<&str, &StructureDefinition> =
        profiles.iter().map(|p| (p.base_type.as_str(), p)).collect();

    // Second pass: generate and write resources with buffered I/O.
    for resource_type in &order {
        let count = counts.get(resource_type).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let type_ids = ids.get(resource_type).cloned().unwrap_or_default();
        let path = data_dir.join(format!("{}.ndjson", resource_type));
        let file = std::fs::File::create(&path)?;
        let mut writer = BufWriter::new(file);
        let mut written = 0u64;

        // Resolve the profile URL for this resource type: prefer the IG's
        // profile, fall back to the base FHIR profile.
        let profile_url = profile_urls.get(resource_type).cloned().unwrap_or_else(|| {
            format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type)
        });

        for id in type_ids.iter() {
            let mut resource = if let Some(profile) = profile_map.get(resource_type.as_str()) {
                // Use profile-aware generation: generates a conformant base from
                // the StructureDefinition, then overlay cross-references.
                let mut r = generate_resource(profile)?;
                r["id"] = serde_json::Value::String(id.clone());
                // Overlay cross-references for types that need them.
                overlay_cross_references(
                    &mut r,
                    resource_type,
                    id,
                    &org_ids,
                    &prac_ids,
                    &loc_ids,
                    &mut rng,
                );
                r
            } else {
                match resource_type.as_str() {
                    "Organization" => gen_organization(id, &mut rng),
                    "Practitioner" => gen_practitioner(id, &mut rng),
                    "PractitionerRole" => {
                        gen_practitioner_role(id, &org_ids, &prac_ids, &loc_ids, &mut rng)
                    }
                    "Location" => gen_location(id, &mut rng),
                    "HealthcareService" => gen_healthcare_service(id, &org_ids, &loc_ids, &mut rng),
                    "Endpoint" => gen_endpoint(id, &org_ids, &mut rng),
                    // Generic fallback for any resource type not explicitly handled
                    _ => gen_generic(resource_type, id, &mut rng),
                }
            };

            // When NOT using profile-aware generation (i.e. falling back to
            // the hardcoded gen_* functions), stamp the profile URL from the
            // IG package to override the hardcoded Plan-Net defaults.
            // When using generate_resource(), the profile URL is already set
            // correctly from the StructureDefinition, so skip the override.
            if !profile_map.contains_key(resource_type.as_str()) {
                resource["meta"]["profile"] = serde_json::json!([profile_url]);
            }

            serde_json::to_writer(&mut writer, &resource)?;
            writeln!(writer)?;
            written += 1;
            if written.is_multiple_of(10_000) {
                // Flush progress to disk so external observers see the file growing.
                writer.flush()?;
                tracing::info!(
                    "Generated {}/{} {} resources",
                    written,
                    count,
                    resource_type
                );
            }
        }
        writer.flush()?;
        tracing::info!(
            "Wrote {} {} resources to {}",
            written,
            resource_type,
            path.display()
        );
    }

    Ok(ids)
}

/// Return a creation order that respects dependencies.
/// Organizations and Locations first, then Practitioners, then
/// PractitionerRoles and HealthcareServices, then everything else.
pub fn bulk_data_creation_order(counts: &HashMap<String, u64>) -> Vec<String> {
    let mut order = Vec::new();

    // Tier 1: no dependencies
    for t in &["Organization", "Location"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }
    // Tier 2: depends on tier 1
    for t in &["Practitioner", "Endpoint"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }
    // Tier 3: depends on tiers 1–2
    for t in &["PractitionerRole", "HealthcareService"] {
        if counts.contains_key(*t) {
            order.push((*t).to_string());
        }
    }
    // Anything else not yet ordered
    for t in counts.keys() {
        if !order.contains(t) {
            order.push(t.clone());
        }
    }
    order
}

/// Pick a random ID from a list, returning a FHIR reference string.
/// Overlay cross-references onto a profile-generated resource.
///
/// When `generate_resource` produces a resource from a StructureDefinition,
/// it creates required fields but cannot know about the IDs of other
/// resources in the bulk data set. This function fills in cross-references
/// (practitioner, organization, location, healthcareService) that the
/// profile may require but which depend on other generated resources.
fn overlay_cross_references(
    resource: &mut serde_json::Value,
    resource_type: &str,
    _id: &str,
    org_ids: &[String],
    prac_ids: &[String],
    loc_ids: &[String],
    rng: &mut impl Rng,
) {
    let obj = match resource.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    match resource_type {
        "PractitionerRole" => {
            // HCPD requires practitioner, organization, and healthcareService references.
            // practitioner and organization are always required; healthcareService
            // references depend on HealthcareService IDs which may not exist yet.
            if !prac_ids.is_empty() {
                let ref_str = random_ref("Practitioner", prac_ids, rng);
                obj.insert(
                    "practitioner".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !org_ids.is_empty() {
                let ref_str = random_ref("Organization", org_ids, rng);
                obj.insert(
                    "organization".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !loc_ids.is_empty() {
                let ref_str = random_ref("Location", loc_ids, rng);
                obj.insert(
                    "location".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
        }
        "HealthcareService" => {
            if !org_ids.is_empty() {
                let ref_str = random_ref("Organization", org_ids, rng);
                obj.insert(
                    "providedBy".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !loc_ids.is_empty() {
                let ref_str = random_ref("Location", loc_ids, rng);
                obj.insert(
                    "location".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
        }
        "Endpoint" if !org_ids.is_empty() => {
            let ref_str = random_ref("Organization", org_ids, rng);
            obj.insert(
                "managingOrganization".to_string(),
                serde_json::json!({ "reference": ref_str }),
            );
        }
        _ => {}
    }
}

fn random_ref(resource_type: &str, ids: &[String], rng: &mut impl Rng) -> String {
    if ids.is_empty() {
        // Fallback if no IDs exist for the referenced type
        format!("{}/placeholder-1", resource_type)
    } else {
        let idx = rng.random_range(0..ids.len());
        format!("{}/{}", resource_type, ids[idx])
    }
}

/// Pick N random IDs from a list (without replacement if possible).
#[allow(dead_code)]
fn random_refs(resource_type: &str, ids: &[String], n: usize, rng: &mut impl Rng) -> Vec<String> {
    if ids.is_empty() {
        vec![format!("{}/placeholder-1", resource_type)]
    } else {
        let n = n.min(ids.len());
        let mut chosen = Vec::with_capacity(n);
        let mut indices: Vec<usize> = (0..ids.len()).collect();
        // Fisher-Yates shuffle to pick n random indices
        for i in (0..indices.len()).rev().take(n) {
            let j = rng.random_range(0..=i);
            indices.swap(i, j);
        }
        for &idx in &indices[..n] {
            chosen.push(format!("{}/{}", resource_type, ids[idx]));
        }
        chosen
    }
}

// ── FHIR Resource Generators ──────────────────────────────────────────────

#[derive(Serialize)]
struct FhirOrganization {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifier: Vec<Identifier>,
    #[serde(rename = "type")]
    org_type: Vec<CodeableConcept>,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    telecom: Option<Vec<ContactPoint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<Vec<Address>>,
}

#[derive(Serialize)]
struct FhirPractitioner {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    name: Vec<HumanName>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifier: Vec<Identifier>,
    active: bool,
    #[serde(rename = "birthDate", skip_serializing_if = "Option::is_none")]
    birth_date: Option<String>,
    #[serde(rename = "gender", skip_serializing_if = "Option::is_none")]
    gender: Option<String>,
}

#[derive(Serialize)]
struct FhirPractitionerRole {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    active: bool,
    practitioner: Reference,
    organization: Reference,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    code: Vec<CodeableConcept>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    specialty: Vec<CodeableConcept>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    location: Vec<Reference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    telecom: Vec<ContactPoint>,
    #[serde(rename = "availableTime", skip_serializing_if = "Vec::is_empty")]
    available_time: Vec<AvailableTime>,
}

#[derive(Serialize)]
struct FhirLocation {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    status: String,
    name: String,
    #[serde(rename = "type", skip_serializing_if = "Vec::is_empty")]
    loc_type: Vec<CodeableConcept>,
    #[serde(rename = "physicalType", skip_serializing_if = "Option::is_none")]
    physical_type: Option<CodeableConcept>,
    position: Position,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<Address>,
    #[serde(
        rename = "managingOrganization",
        skip_serializing_if = "Option::is_none"
    )]
    managing_organization: Option<Reference>,
}

#[derive(Serialize)]
struct FhirHealthcareService {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    active: bool,
    #[serde(rename = "providedBy")]
    provided_by: Reference,
    #[serde(rename = "type", skip_serializing_if = "Vec::is_empty")]
    svc_type: Vec<CodeableConcept>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    specialty: Vec<CodeableConcept>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    location: Vec<Reference>,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
}

#[derive(Serialize)]
struct FhirEndpoint {
    #[serde(rename = "resourceType")]
    resource_type: String,
    id: String,
    #[serde(rename = "meta")]
    meta: Meta,
    status: String,
    #[serde(rename = "connectionType")]
    connection_type: Coding,
    name: String,
    #[serde(rename = "payloadType")]
    payload_type: Vec<CodeableConcept>,
    address: String,
    #[serde(
        rename = "managingOrganization",
        skip_serializing_if = "Option::is_none"
    )]
    managing_organization: Option<Reference>,
}

// ── FHIR Datatype Helpers ─────────────────────────────────────────────────

#[derive(Serialize)]
struct Meta {
    profile: Vec<String>,
}

#[derive(Serialize)]
struct HumanName {
    family: String,
    #[serde(rename = "given")]
    given: Vec<String>,
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    name_use: Option<String>,
}

#[derive(Serialize)]
struct Identifier {
    system: String,
    value: String,
}

#[derive(Serialize)]
struct CodeableConcept {
    coding: Vec<Coding>,
}

#[derive(Serialize)]
struct Coding {
    system: String,
    code: String,
    #[serde(rename = "display", skip_serializing_if = "Option::is_none")]
    display: Option<String>,
}

#[derive(Serialize)]
struct Reference {
    reference: String,
    #[serde(rename = "display", skip_serializing_if = "Option::is_none")]
    display: Option<String>,
}

#[derive(Serialize)]
struct ContactPoint {
    system: String,
    value: String,
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    cp_use: Option<String>,
}

#[derive(Serialize)]
struct Address {
    #[serde(rename = "type")]
    addr_type: String,
    line: Vec<String>,
    city: String,
    state: String,
    #[serde(rename = "postalCode")]
    postal_code: String,
    country: String,
}

#[derive(Serialize)]
struct Position {
    latitude: f64,
    longitude: f64,
}

#[derive(Serialize)]
struct AvailableTime {
    #[serde(rename = "daysOfWeek")]
    days_of_week: Vec<String>,
    #[serde(rename = "availableStartTime")]
    available_start_time: String,
    #[serde(rename = "availableEndTime")]
    available_end_time: String,
}

// ── Generator Functions ────────────────────────────────────────────────────

fn gen_organization(id: &str, rng: &mut impl Rng) -> serde_json::Value {
    let name: String = fake::faker::company::en::CompanyName().fake();
    let npi = format!(
        "{}{}",
        rng.random_range(100..999),
        rng.random_range(1000000..9999999)
    );
    let org_types = [
        "prov", "dept", "team", "govt", "ins", "pay", "edu", "reli", "crs",
    ];
    let org_type = org_types[rng.random_range(0..org_types.len())];
    let city: String = fake::faker::address::en::CityName().fake();
    let state: String = fake::faker::address::en::StateAbbr().fake();

    let org = FhirOrganization {
        resource_type: "Organization".to_string(),
        id: id.to_string(),
        meta: Meta { profile: vec!["http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Organization".to_string()] },
        name,
        identifier: vec![Identifier {
            system: "http://hl7.org/fhir/sid/us-npi".to_string(),
            value: npi,
        }],
        org_type: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/organization-type".to_string(),
                code: org_type.to_string(),
                display: Some(match org_type {
                    "prov" => "Healthcare Provider",
                    "dept" => "Hospital Department",
                    "team" => "Organizational team",
                    "govt" => "Government",
                    "ins" => "Insurance Company",
                    "pay" => "Payer",
                    "edu" => "Educational Institute",
                    _ => "Religious Institution",
                }.to_string()),
            }],
        }],
        active: true,
        telecom: Some(vec![ContactPoint {
            system: "phone".to_string(),
            value: fake::faker::phone_number::en::PhoneNumber().fake(),
            cp_use: Some("work".to_string()),
        }]),
        address: Some(vec![Address {
            addr_type: "physical".to_string(),
            line: vec![fake::faker::address::en::StreetName().fake()],
            city,
            state,
            postal_code: fake::faker::address::en::ZipCode().fake(),
            country: "US".to_string(),
        }]),
    };
    serde_json::to_value(org).unwrap()
}

fn gen_practitioner(id: &str, rng: &mut impl Rng) -> serde_json::Value {
    let family: String = fake::faker::name::en::LastName().fake();
    let given: String = fake::faker::name::en::FirstName().fake();
    let npi = format!(
        "{}{}",
        rng.random_range(100..999),
        rng.random_range(1000000..9999999)
    );
    let year: u32 = rng.random_range(1950..=2000);
    let month: u32 = rng.random_range(1..=12);
    let day: u32 = rng.random_range(1..=28);
    let genders = ["male", "female", "other", "unknown"];
    let gender = genders[rng.random_range(0..genders.len())];

    let prac = FhirPractitioner {
        resource_type: "Practitioner".to_string(),
        id: id.to_string(),
        meta: Meta { profile: vec!["http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Practitioner".to_string()] },
        name: vec![HumanName {
            family,
            given: vec![given],
            name_use: Some("official".to_string()),
        }],
        identifier: vec![Identifier {
            system: "http://hl7.org/fhir/sid/us-npi".to_string(),
            value: npi,
        }],
        active: true,
        birth_date: Some(format!("{:04}-{:02}-{:02}", year, month, day)),
        gender: Some(gender.to_string()),
    };
    serde_json::to_value(prac).unwrap()
}

fn gen_practitioner_role(
    id: &str,
    org_ids: &[String],
    prac_ids: &[String],
    loc_ids: &[String],
    rng: &mut impl Rng,
) -> serde_json::Value {
    let role_codes = [
        ("doctor", "Doctor"),
        ("nurse", "Nurse"),
        ("pharmacist", "Pharmacist"),
        ("physicaltherapist", "Physical Therapist"),
        ("socialworker", "Social Worker"),
        ("psychologist", "Psychologist"),
        ("dietitian", "Dietitian"),
        ("optometrist", "Optometrist"),
    ];
    let specialties = [
        ("394577000", "Anesthesiology"),
        ("394583001", "Dermatology"),
        ("394579002", "Cardiology"),
        ("394582007", "Dermatology"),
        ("408467006", "Emergency medicine"),
        ("394597006", "Oncology"),
        ("394580004", "General practice"),
        ("394609004", "Orthopaedics"),
        ("394610002", "Otolaryngology"),
        ("394612008", "Paediatrics"),
        ("394600006", "Neurology"),
        ("394585009", "Psychiatry"),
        ("394591006", "Ophthalmology"),
        ("394584008", "Respiratory"),
    ];
    let spec = specialties[rng.random_range(0..specialties.len())];
    let role = role_codes[rng.random_range(0..role_codes.len())];

    let n_specialties = rng.random_range(1..3);
    let mut spec_list = vec![CodeableConcept {
        coding: vec![Coding {
            system: "http://snomed.info/sct".to_string(),
            code: spec.0.to_string(),
            display: Some(spec.1.to_string()),
        }],
    }];
    for _ in 1..n_specialties {
        let s = specialties[rng.random_range(0..specialties.len())];
        spec_list.push(CodeableConcept {
            coding: vec![Coding {
                system: "http://snomed.info/sct".to_string(),
                code: s.0.to_string(),
                display: Some(s.1.to_string()),
            }],
        });
    }

    let days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let n_days = rng.random_range(2..6);
    let start_day = rng.random_range(0..days.len() - n_days + 1);
    let work_days: Vec<String> = days[start_day..start_day + n_days]
        .iter()
        .map(|d| d.to_string())
        .collect();

    let mut locations = Vec::new();
    if !loc_ids.is_empty() {
        let loc_ref = random_ref("Location", loc_ids, rng);
        locations.push(Reference {
            reference: loc_ref,
            display: None,
        });
    }

    let role = FhirPractitionerRole {
        resource_type: "PractitionerRole".to_string(),
        id: id.to_string(),
        meta: Meta { profile: vec!["http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-PractitionerRole".to_string()] },
        active: true,
        practitioner: Reference {
            reference: random_ref("Practitioner", prac_ids, rng),
            display: None,
        },
        organization: Reference {
            reference: random_ref("Organization", org_ids, rng),
            display: None,
        },
        code: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/v3-RoleCode".to_string(),
                code: role.0.to_string(),
                display: Some(role.1.to_string()),
            }],
        }],
        specialty: spec_list,
        location: locations,
        telecom: vec![ContactPoint {
            system: "phone".to_string(),
            value: fake::faker::phone_number::en::PhoneNumber().fake(),
            cp_use: Some("work".to_string()),
        }],
        available_time: vec![AvailableTime {
            days_of_week: work_days,
            available_start_time: "08:00:00".to_string(),
            available_end_time: "17:00:00".to_string(),
        }],
    };
    serde_json::to_value(role).unwrap()
}

fn gen_location(id: &str, rng: &mut impl Rng) -> serde_json::Value {
    let loc_types = [
        ("si", "Site"),
        ("bu", "Building"),
        ("wi", "Wing"),
        ("wa", "Ward"),
        ("lvl", "Level"),
        ("co", "Corner"),
    ];
    let phys_types = [
        ("si", "Site"),
        ("bu", "Building"),
        ("wi", "Wing"),
        ("wa", "Ward"),
        ("lvl", "Level"),
        ("co", "Corner"),
        ("ho", "House"),
        ("ca", "Room"),
        ("ve", "Vehicle"),
        ("ho", "House"),
    ];
    let loc_type = loc_types[rng.random_range(0..loc_types.len())];
    let phys_type = phys_types[rng.random_range(0..phys_types.len())];

    // Spread locations around major US cities for realistic near searches
    let city_centers: Vec<(f64, f64, &str)> = vec![
        (40.7128, -74.0060, "New York"),
        (34.0522, -118.2437, "Los Angeles"),
        (41.8781, -87.6298, "Chicago"),
        (29.7604, -95.3698, "Houston"),
        (33.4484, -112.0740, "Phoenix"),
        (39.9526, -75.1652, "Philadelphia"),
        (29.4241, -98.4936, "San Antonio"),
        (32.7157, -117.1611, "San Diego"),
        (32.7767, -96.7970, "Dallas"),
        (37.3382, -121.8863, "San Jose"),
        (47.6062, -122.3321, "Seattle"),
        (42.3601, -71.0589, "Boston"),
        (38.9072, -77.0369, "Washington"),
        (39.7392, -104.9903, "Denver"),
        (25.7617, -80.1918, "Miami"),
        (33.7490, -84.3880, "Atlanta"),
        (35.2271, -80.8431, "Charlotte"),
        (36.1627, -86.7816, "Nashville"),
        (45.5051, -122.6750, "Portland"),
        (35.4676, -97.5164, "Oklahoma City"),
    ];
    let center = &city_centers[rng.random_range(0..city_centers.len())];
    let lat = center.0 + (rng.random_range(-50..50) as f64 / 1000.0);
    let lon = center.1 + (rng.random_range(-50..50) as f64 / 1000.0);
    let city_name = center.2.to_string();

    let statuses = ["active", "suspended", "inactive"];
    let status = statuses[rng.random_range(0..2)]; // weight toward active

    let loc = FhirLocation {
        resource_type: "Location".to_string(),
        id: id.to_string(),
        meta: Meta {
            profile: vec![
                "http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Location"
                    .to_string(),
            ],
        },
        status: status.to_string(),
        name: format!(
            "{} Clinic - {}",
            fake::faker::name::en::LastName().fake::<String>(),
            city_name
        ),
        loc_type: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/v3-RoleCode".to_string(),
                code: loc_type.0.to_string(),
                display: Some(loc_type.1.to_string()),
            }],
        }],
        physical_type: Some(CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/location-physical-type".to_string(),
                code: phys_type.0.to_string(),
                display: Some(phys_type.1.to_string()),
            }],
        }),
        position: Position {
            latitude: lat,
            longitude: lon,
        },
        address: Some(Address {
            addr_type: "physical".to_string(),
            line: vec![format!(
                "{} {} St",
                rng.random_range(100..9999),
                fake::faker::name::en::LastName().fake::<String>()
            )],
            city: city_name,
            state: fake::faker::address::en::StateAbbr().fake(),
            postal_code: fake::faker::address::en::ZipCode().fake(),
            country: "US".to_string(),
        }),
        managing_organization: None, // filled in bulk loader if org IDs available
    };
    serde_json::to_value(loc).unwrap()
}

fn gen_healthcare_service(
    id: &str,
    org_ids: &[String],
    loc_ids: &[String],
    rng: &mut impl Rng,
) -> serde_json::Value {
    let svc_types = [
        ("1", "Emergency department"),
        ("2", "Hospital clinic"),
        ("3", "Hospital service"),
        ("4", "Outpatient clinic"),
        ("5", "Specialist clinic"),
        ("6", "Rehabilitation"),
        ("7", "Pharmacy"),
        ("8", "Laboratory"),
        ("9", "Imaging"),
        ("10", "Mental health"),
        ("11", "Dental"),
        ("12", "Home health"),
        ("13", "Hospice"),
        ("14", "Telehealth"),
        ("15", "Urgent care"),
    ];
    let specialties = [
        ("394577000", "Anesthesiology"),
        ("394583001", "Dermatology"),
        ("394579002", "Cardiology"),
        ("394580004", "General practice"),
        ("394597006", "Oncology"),
        ("394600006", "Neurology"),
        ("394609004", "Orthopaedics"),
        ("394612008", "Paediatrics"),
        ("394584008", "Respiratory"),
    ];

    let svc = svc_types[rng.random_range(0..svc_types.len())];
    let spec = specialties[rng.random_range(0..specialties.len())];

    let mut locations = Vec::new();
    if !loc_ids.is_empty() {
        locations.push(Reference {
            reference: random_ref("Location", loc_ids, rng),
            display: None,
        });
    }

    let svc = FhirHealthcareService {
        resource_type: "HealthcareService".to_string(),
        id: id.to_string(),
        meta: Meta { profile: vec!["http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-HealthcareService".to_string()] },
        active: true,
        provided_by: Reference {
            reference: random_ref("Organization", org_ids, rng),
            display: None,
        },
        svc_type: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/service-type".to_string(),
                code: svc.0.to_string(),
                display: Some(svc.1.to_string()),
            }],
        }],
        specialty: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://snomed.info/sct".to_string(),
                code: spec.0.to_string(),
                display: Some(spec.1.to_string()),
            }],
        }],
        location: locations,
        name: format!("{} Service", svc.1),
        comment: Some(format!("Provides {} services", svc.1.to_lowercase())),
    };
    serde_json::to_value(svc).unwrap()
}

fn gen_endpoint(id: &str, org_ids: &[String], rng: &mut impl Rng) -> serde_json::Value {
    let endpoint = FhirEndpoint {
        resource_type: "Endpoint".to_string(),
        id: id.to_string(),
        meta: Meta {
            profile: vec![
                "http://hl7.org/fhir/us/davinci-pdex-plan-net/StructureDefinition/plannet-Endpoint"
                    .to_string(),
            ],
        },
        status: "active".to_string(),
        connection_type: Coding {
            system: "http://terminology.hl7.org/CodeSystem/endpoint-connection-type".to_string(),
            code: "hl7-fhir-rest".to_string(),
            display: Some("HL7 FHIR REST".to_string()),
        },
        name: format!(
            "{} FHIR Endpoint",
            fake::faker::company::en::CompanyName().fake::<String>()
        ),
        payload_type: vec![CodeableConcept {
            coding: vec![Coding {
                system: "http://terminology.hl7.org/CodeSystem/endpoint-payload-type".to_string(),
                code: "none".to_string(),
                display: Some("None".to_string()),
            }],
        }],
        address: format!(
            "https://{}.example.org/fhir",
            fake::faker::internet::en::DomainSuffix().fake::<String>()
        ),
        managing_organization: if org_ids.is_empty() {
            None
        } else {
            Some(Reference {
                reference: random_ref("Organization", org_ids, rng),
                display: None,
            })
        },
    };
    serde_json::to_value(endpoint).unwrap()
}

/// Generic fallback generator for resource types not explicitly handled.
/// Produces a minimal resource with `resourceType`, `id`, `meta`, and `status`.
fn gen_generic(resource_type: &str, id: &str, _rng: &mut impl Rng) -> serde_json::Value {
    let mut resource = serde_json::json!({
        "resourceType": resource_type,
        "id": id,
        "meta": {
            "profile": [format!("http://hl7.org/fhir/StructureDefinition/{}", resource_type)]
        },
        "status": "active"
    });
    // Add a name field if the resource type typically has one
    if matches!(
        resource_type,
        "Patient" | "Person" | "Group" | "List" | "Library"
    ) {
        resource["name"] = serde_json::json!(fake::faker::name::en::Name().fake::<String>());
    }
    resource
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_creates_ndjson_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 10);
        counts.insert("Practitioner".to_string(), 50);
        counts.insert("PractitionerRole".to_string(), 100);
        counts.insert("Location".to_string(), 20);
        counts.insert("HealthcareService".to_string(), 50);

        let profile_urls = HashMap::new();
        let ids = generate_bulk_data(&counts, &profile_urls, &[], dir.path()).unwrap();

        // Each type should have the right number of IDs
        assert_eq!(ids.get("Organization").unwrap().len(), 10);
        assert_eq!(ids.get("Practitioner").unwrap().len(), 50);
        assert_eq!(ids.get("PractitionerRole").unwrap().len(), 100);
        assert_eq!(ids.get("Location").unwrap().len(), 20);
        assert_eq!(ids.get("HealthcareService").unwrap().len(), 50);

        // NDJSON files should exist and have the right line counts
        for (resource_type, count) in &counts {
            let path = dir
                .path()
                .join("data")
                .join(format!("{}.ndjson", resource_type));
            assert!(path.exists(), "{}.ndjson should exist", resource_type);
            let contents = std::fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
            assert_eq!(
                lines.len(),
                *count as usize,
                "{} should have {} lines",
                resource_type,
                count
            );

            // Each line should be valid JSON
            for line in &lines {
                let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(parsed["resourceType"], *resource_type);
                assert!(!parsed["id"].as_str().unwrap().is_empty());
            }
        }
    }

    #[test]
    fn cross_references_are_valid() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 5);
        counts.insert("Practitioner".to_string(), 10);
        counts.insert("PractitionerRole".to_string(), 20);
        counts.insert("Location".to_string(), 5);
        counts.insert("HealthcareService".to_string(), 10);

        let profile_urls = HashMap::new();
        let ids = generate_bulk_data(&counts, &profile_urls, &[], dir.path()).unwrap();

        // Check PractitionerRole references
        let pr_path = dir.path().join("data/PractitionerRole.ndjson");
        let pr_contents = std::fs::read_to_string(&pr_path).unwrap();
        let org_ids = ids.get("Organization").unwrap();
        let prac_ids = ids.get("Practitioner").unwrap();

        for line in pr_contents.lines().filter(|l| !l.is_empty()) {
            let pr: serde_json::Value = serde_json::from_str(line).unwrap();
            let prac_ref = pr["practitioner"]["reference"].as_str().unwrap();
            assert!(prac_ref.starts_with("Practitioner/"));
            let prac_id = prac_ref.strip_prefix("Practitioner/").unwrap();
            assert!(
                prac_ids.contains(&prac_id.to_string()),
                "Practitioner reference {} should exist",
                prac_id
            );

            let org_ref = pr["organization"]["reference"].as_str().unwrap();
            assert!(org_ref.starts_with("Organization/"));
            let org_id = org_ref.strip_prefix("Organization/").unwrap();
            assert!(
                org_ids.contains(&org_id.to_string()),
                "Organization reference {} should exist",
                org_id
            );
        }

        // Check HealthcareService references
        let hs_path = dir.path().join("data/HealthcareService.ndjson");
        let hs_contents = std::fs::read_to_string(&hs_path).unwrap();
        for line in hs_contents.lines().filter(|l| !l.is_empty()) {
            let hs: serde_json::Value = serde_json::from_str(line).unwrap();
            let org_ref = hs["providedBy"]["reference"].as_str().unwrap();
            assert!(org_ref.starts_with("Organization/"));
        }
    }

    #[test]
    fn location_has_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Location".to_string(), 100);

        generate_bulk_data(&counts, &HashMap::new(), &[], dir.path()).unwrap();

        let loc_path = dir.path().join("data/Location.ndjson");
        let contents = std::fs::read_to_string(&loc_path).unwrap();
        for line in contents.lines().filter(|l| !l.is_empty()) {
            let loc: serde_json::Value = serde_json::from_str(line).unwrap();
            let lat = loc["position"]["latitude"].as_f64().unwrap();
            let lon = loc["position"]["longitude"].as_f64().unwrap();
            // Should be in US range
            assert!(
                (20.0..=55.0).contains(&lat),
                "Latitude {} should be in US range",
                lat
            );
            assert!(
                (-130.0..=-60.0).contains(&lon),
                "Longitude {} should be in US range",
                lon
            );
        }
    }

    #[test]
    fn creation_order_respects_dependencies() {
        let mut counts = HashMap::new();
        counts.insert("PractitionerRole".to_string(), 10);
        counts.insert("Organization".to_string(), 5);
        counts.insert("Location".to_string(), 5);

        let order = bulk_data_creation_order(&counts);

        // Organization and Location should come before PractitionerRole
        let org_idx = order.iter().position(|t| t == "Organization").unwrap();
        let loc_idx = order.iter().position(|t| t == "Location").unwrap();
        let pr_idx = order.iter().position(|t| t == "PractitionerRole").unwrap();
        assert!(
            org_idx < pr_idx,
            "Organization should come before PractitionerRole"
        );
        assert!(
            loc_idx < pr_idx,
            "Location should come before PractitionerRole"
        );
    }

    #[test]
    fn generic_fallback_works() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Patient".to_string(), 5);

        let ids = generate_bulk_data(&counts, &HashMap::new(), &[], dir.path()).unwrap();
        assert_eq!(ids.get("Patient").unwrap().len(), 5);

        let path = dir.path().join("data/Patient.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();
        let first_line = contents.lines().next().unwrap();
        let patient: serde_json::Value = serde_json::from_str(first_line).unwrap();
        assert_eq!(patient["resourceType"], "Patient");
        assert_eq!(patient["status"], "active");
    }

    #[test]
    fn profile_urls_override_meta_profile() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 3);

        let mut profile_urls = HashMap::new();
        profile_urls.insert(
            "Organization".to_string(),
            "http://example.org/fhir/StructureDefinition/MyOrg".to_string(),
        );

        let ids = generate_bulk_data(&counts, &profile_urls, &[], dir.path()).unwrap();
        assert_eq!(ids.get("Organization").unwrap().len(), 3);

        let path = dir.path().join("data/Organization.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();
        for line in contents.lines().filter(|l| !l.is_empty()) {
            let org: serde_json::Value = serde_json::from_str(line).unwrap();
            let profiles = org["meta"]["profile"].as_array().unwrap();
            assert_eq!(
                profiles[0].as_str().unwrap(),
                "http://example.org/fhir/StructureDefinition/MyOrg",
                "meta.profile should use the IG profile URL, not the hardcoded Plan-Net URL"
            );
        }
    }

    #[test]
    fn profile_urls_fallback_to_base_fhir() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Organization".to_string(), 2);

        // No profile_urls provided — should fall back to base FHIR profile
        let profile_urls = HashMap::new();
        let ids = generate_bulk_data(&counts, &profile_urls, &[], dir.path()).unwrap();
        assert_eq!(ids.get("Organization").unwrap().len(), 2);

        let path = dir.path().join("data/Organization.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();
        for line in contents.lines().filter(|l| !l.is_empty()) {
            let org: serde_json::Value = serde_json::from_str(line).unwrap();
            let profiles = org["meta"]["profile"].as_array().unwrap();
            assert_eq!(
                profiles[0].as_str().unwrap(),
                "http://hl7.org/fhir/StructureDefinition/Organization",
                "meta.profile should fall back to base FHIR profile when no IG profile is provided"
            );
        }
    }

    #[test]
    fn profile_aware_generation_uses_structure_definition() {
        let dir = tempfile::tempdir().unwrap();
        let mut counts = HashMap::new();
        counts.insert("Patient".to_string(), 2);

        // Create a minimal StructureDefinition for Patient
        let profile = crate::model::profile::StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/fhir/StructureDefinition/MyPatient".to_string(),
            name: "MyPatient".to_string(),
            base_type: "Patient".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            snapshot: None,
            differential: None,
            base_definition: Some("http://hl7.org/fhir/StructureDefinition/Patient".to_string()),
        };

        let ids = generate_bulk_data(&counts, &HashMap::new(), &[profile], dir.path()).unwrap();
        assert_eq!(ids.get("Patient").unwrap().len(), 2);

        // When a StructureDefinition is provided, resources should be generated
        // via generate_resource (profile-aware) rather than gen_generic.
        // The profile URL in meta.profile should match the StructureDefinition.
        let path = dir.path().join("data/Patient.ndjson");
        let contents = std::fs::read_to_string(&path).unwrap();
        for line in contents.lines().filter(|l| !l.is_empty()) {
            let patient: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(patient["resourceType"], "Patient");
            let profiles = patient["meta"]["profile"].as_array().unwrap();
            assert_eq!(
                profiles[0].as_str().unwrap(),
                "http://example.org/fhir/StructureDefinition/MyPatient",
                "Profile-aware generation should use the StructureDefinition URL"
            );
        }
    }
}
