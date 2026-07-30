use crate::generate::locality::random_au_locality;
use fake::Fake;
use rand::Rng;
use serde::Serialize;

use super::random_ref;

// ── FHIR Resource Structs ──────────────────────────────────────────────────

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

pub fn gen_organization(id: &str, rng: &mut impl Rng) -> serde_json::Value {
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
    let locality = random_au_locality(rng);

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
            city: locality.city.to_string(),
            state: locality.state.to_string(),
            postal_code: locality.postcode.to_string(),
            country: "AU".to_string(),
        }]),
    };
    serde_json::to_value(org).unwrap()
}

pub fn gen_practitioner(id: &str, rng: &mut impl Rng) -> serde_json::Value {
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

pub fn gen_practitioner_role(
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

pub fn gen_location(id: &str, rng: &mut impl Rng) -> serde_json::Value {
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

    // Spread locations around Australian localities for realistic near searches
    let locality = random_au_locality(rng);
    let lat = locality.lat + (rng.random_range(-50..50) as f64 / 1000.0);
    let lon = locality.lon + (rng.random_range(-50..50) as f64 / 1000.0);

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
            locality.suburb
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
            city: locality.city.to_string(),
            state: locality.state.to_string(),
            postal_code: locality.postcode.to_string(),
            country: "AU".to_string(),
        }),
        managing_organization: None, // filled in bulk loader if org IDs available
    };
    serde_json::to_value(loc).unwrap()
}

pub fn gen_healthcare_service(
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

pub fn gen_endpoint(id: &str, org_ids: &[String], rng: &mut impl Rng) -> serde_json::Value {
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
pub fn gen_generic(resource_type: &str, id: &str, _rng: &mut impl Rng) -> serde_json::Value {
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
