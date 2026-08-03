use super::*;
use std::collections::HashMap;

// Helper to access update module functions
use super::supplement::normalize_supplement_references;
use super::update::{
    apply_random_updates, discover_mutable_paths, get_at_path, is_reference_field, mutate_bool,
    mutate_number, mutate_string, set_at_path,
};

#[test]
fn generate_creates_ndjson_files() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 10);
    counts.insert("Practitioner".to_string(), 50);
    counts.insert("PractitionerRole".to_string(), 100);
    counts.insert("Location".to_string(), 20);
    counts.insert("HealthcareService".to_string(), 50);

    let profile_urls = HashMap::new();
    let ids = generate_bulk_data(
        &counts,
        &profile_urls,
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // Each type should have the right number of IDs
    assert_eq!(ids.get("Organization").expect("key should exist").len(), 10);
    assert_eq!(ids.get("Practitioner").expect("key should exist").len(), 50);
    assert_eq!(
        ids.get("PractitionerRole").expect("key should exist").len(),
        100
    );
    assert_eq!(ids.get("Location").expect("key should exist").len(), 20);
    assert_eq!(
        ids.get("HealthcareService")
            .expect("key should exist")
            .len(),
        50
    );

    // NDJSON files should exist and have the right line counts
    for (resource_type, count) in &counts {
        let path = dir
            .path()
            .join("data")
            .join(format!("{}.ndjson", resource_type));
        assert!(path.exists(), "{}.ndjson should exist", resource_type);
        let contents = std::fs::read_to_string(&path).expect("should read file");
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
            let parsed: serde_json::Value =
                serde_json::from_str(line).expect("should parse valid JSON");
            assert_eq!(parsed["resourceType"], *resource_type);
            assert!(
                !parsed["id"]
                    .as_str()
                    .expect("should have a string value")
                    .is_empty()
            );
        }
    }
}

#[test]
fn cross_references_are_valid() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 5);
    counts.insert("Practitioner".to_string(), 10);
    counts.insert("PractitionerRole".to_string(), 20);
    counts.insert("Location".to_string(), 5);
    counts.insert("HealthcareService".to_string(), 10);

    let profile_urls = HashMap::new();
    let ids = generate_bulk_data(
        &counts,
        &profile_urls,
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // Check PractitionerRole references
    let pr_path = dir.path().join("data/PractitionerRole.ndjson");
    let pr_contents = std::fs::read_to_string(&pr_path).expect("should read file");
    let org_ids = ids.get("Organization").expect("key should exist");
    let prac_ids = ids.get("Practitioner").expect("key should exist");

    for line in pr_contents.lines().filter(|l| !l.is_empty()) {
        let pr: serde_json::Value = serde_json::from_str(line).expect("should parse valid JSON");
        let prac_ref = pr["practitioner"]["reference"]
            .as_str()
            .expect("should have a string value");
        assert!(prac_ref.starts_with("Practitioner/"));
        let prac_id = prac_ref
            .strip_prefix("Practitioner/")
            .expect("should have expected prefix");
        assert!(
            prac_ids.contains(&prac_id.to_string()),
            "Practitioner reference {} should exist",
            prac_id
        );

        let org_ref = pr["organization"]["reference"]
            .as_str()
            .expect("should have a string value");
        assert!(org_ref.starts_with("Organization/"));
        let org_id = org_ref
            .strip_prefix("Organization/")
            .expect("should have expected prefix");
        assert!(
            org_ids.contains(&org_id.to_string()),
            "Organization reference {} should exist",
            org_id
        );
    }

    // Check HealthcareService references
    let hs_path = dir.path().join("data/HealthcareService.ndjson");
    let hs_contents = std::fs::read_to_string(&hs_path).expect("should read file");
    for line in hs_contents.lines().filter(|l| !l.is_empty()) {
        let hs: serde_json::Value = serde_json::from_str(line).expect("should parse valid JSON");
        let org_ref = hs["providedBy"]["reference"]
            .as_str()
            .expect("should have a string value");
        assert!(org_ref.starts_with("Organization/"));
    }
}

#[test]
fn location_has_coordinates() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Location".to_string(), 100);

    generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    let loc_path = dir.path().join("data/Location.ndjson");
    let contents = std::fs::read_to_string(&loc_path).expect("should read file");
    for line in contents.lines().filter(|l| !l.is_empty()) {
        let loc: serde_json::Value = serde_json::from_str(line).expect("should parse valid JSON");
        let lat = loc["position"]["latitude"]
            .as_f64()
            .expect("should have a float value");
        let lon = loc["position"]["longitude"]
            .as_f64()
            .expect("should have a float value");
        // Generated localities are AU-based.
        assert!(
            (-45.0..=-9.0).contains(&lat),
            "Latitude {} should be in AU range",
            lat
        );
        assert!(
            (110.0..=156.0).contains(&lon),
            "Longitude {} should be in AU range",
            lon
        );
    }
}

#[test]
fn creation_order_respects_dependencies() {
    let mut counts = HashMap::new();
    counts.insert("PractitionerRole".to_string(), 10);
    counts.insert("Organization".to_string(), 5);
    counts.insert("Endpoint".to_string(), 5);
    counts.insert("Location".to_string(), 5);

    let order = bulk_data_creation_order(&counts);

    // Organization, Endpoint, and Location should come before PractitionerRole.
    let org_idx = order
        .iter()
        .position(|t| t == "Organization")
        .expect("type should be in creation order");
    let endpoint_idx = order
        .iter()
        .position(|t| t == "Endpoint")
        .expect("type should be in creation order");
    let loc_idx = order
        .iter()
        .position(|t| t == "Location")
        .expect("type should be in creation order");
    let pr_idx = order
        .iter()
        .position(|t| t == "PractitionerRole")
        .expect("type should be in creation order");
    assert!(
        org_idx < pr_idx,
        "Organization should come before PractitionerRole"
    );
    assert!(
        endpoint_idx < loc_idx,
        "Endpoint should come before Location"
    );
    assert!(
        loc_idx < pr_idx,
        "Location should come before PractitionerRole"
    );
}

#[test]
fn generic_fallback_works() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Patient".to_string(), 5);

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");
    assert_eq!(ids.get("Patient").expect("key should exist").len(), 5);

    let path = dir.path().join("data/Patient.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    let first_line = contents
        .lines()
        .next()
        .expect("should have at least one line");
    let patient: serde_json::Value =
        serde_json::from_str(first_line).expect("should parse valid JSON");
    assert_eq!(patient["resourceType"], "Patient");
    assert_eq!(patient["status"], "active");
}

#[test]
fn profile_urls_override_meta_profile() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 3);

    let mut profile_urls = HashMap::new();
    profile_urls.insert(
        "Organization".to_string(),
        "http://example.org/fhir/StructureDefinition/MyOrg".to_string(),
    );

    let ids = generate_bulk_data(
        &counts,
        &profile_urls,
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");
    assert_eq!(ids.get("Organization").expect("key should exist").len(), 3);

    let path = dir.path().join("data/Organization.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    for line in contents.lines().filter(|l| !l.is_empty()) {
        let org: serde_json::Value = serde_json::from_str(line).expect("should parse valid JSON");
        let profiles = org["meta"]["profile"]
            .as_array()
            .expect("should be an array");
        assert_eq!(
            profiles[0].as_str().expect("should have a string value"),
            "http://example.org/fhir/StructureDefinition/MyOrg",
            "meta.profile should use the IG profile URL, not the hardcoded Plan-Net URL"
        );
    }
}

#[test]
fn profile_urls_fallback_to_base_fhir() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 2);

    // No profile_urls provided — should fall back to base FHIR profile
    let profile_urls = HashMap::new();
    let ids = generate_bulk_data(
        &counts,
        &profile_urls,
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");
    assert_eq!(ids.get("Organization").expect("key should exist").len(), 2);

    let path = dir.path().join("data/Organization.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    for line in contents.lines().filter(|l| !l.is_empty()) {
        let org: serde_json::Value = serde_json::from_str(line).expect("should parse valid JSON");
        let profiles = org["meta"]["profile"]
            .as_array()
            .expect("should be an array");
        assert_eq!(
            profiles[0].as_str().expect("should have a string value"),
            "http://hl7.org/fhir/StructureDefinition/Organization",
            "meta.profile should fall back to base FHIR profile when no IG profile is provided"
        );
    }
}

#[test]
fn profile_aware_generation_uses_structure_definition() {
    let dir = tempfile::tempdir().expect("should create temp dir");
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

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[profile],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");
    assert_eq!(ids.get("Patient").expect("key should exist").len(), 2);

    // When a StructureDefinition is provided, resources should be generated
    // via generate_resource (profile-aware) rather than gen_generic.
    // The profile URL in meta.profile should match the StructureDefinition.
    let path = dir.path().join("data/Patient.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    for line in contents.lines().filter(|l| !l.is_empty()) {
        let patient: serde_json::Value =
            serde_json::from_str(line).expect("should parse valid JSON");
        assert_eq!(patient["resourceType"], "Patient");
        let profiles = patient["meta"]["profile"]
            .as_array()
            .expect("should be an array");
        assert_eq!(
            profiles[0].as_str().expect("should have a string value"),
            "http://example.org/fhir/StructureDefinition/MyPatient",
            "Profile-aware generation should use the StructureDefinition URL"
        );
    }
}

#[test]
fn provenance_overlay_uses_existing_ids() {
    let mut provenance = serde_json::json!({
        "resourceType": "Provenance",
        "id": "provenance-1",
        "target": [{ "reference": "Organization/random-uuid" }],
        "agent": [{ "who": { "reference": "Organization/random-uuid" } }],
        "entity": [{ "role": "source", "what": { "reference": "Resource/random-uuid" } }]
    });

    let org_ids = vec!["organization-1".to_string(), "organization-2".to_string()];
    let prac_ids = vec!["practitioner-1".to_string()];
    let mut rng = rand::rng();

    overlay::overlay_cross_references(
        &mut provenance,
        "Provenance",
        "provenance-1",
        &org_ids,
        &prac_ids,
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );

    let target_ref = provenance["target"][0]["reference"]
        .as_str()
        .expect("should have a string value");
    let agent_ref = provenance["agent"][0]["who"]["reference"]
        .as_str()
        .expect("should have a string value");
    let entity_ref = provenance["entity"][0]["what"]["reference"]
        .as_str()
        .expect("should succeed");

    // target now distributes across all available resource types
    // for _revinclude coverage. With org_ids=[org-1,org-2] and
    // prac_ids=[prac-1], provenance-1 picks Practitioner (idx=1%2=1).
    assert_eq!(
        target_ref, "Practitioner/practitioner-1",
        "target should reference a non-Organization type for _revinclude coverage"
    );
    assert!(
        agent_ref == "Organization/organization-1" || agent_ref == "Organization/organization-2",
        "agent.who should reference an existing Organization ID"
    );
    assert!(
        ["Organization/organization-1", "Organization/organization-2",].contains(&entity_ref),
        "entity.what should reference an Organization ID"
    );
}

#[test]
fn provenance_overlay_populates_target_extension() {
    // Provenance.target.extension is a mustSupport field of type Extension,
    // which the profile-driven generator skips. The overlay must add the
    // standard targetPath extension so the conformance check passes.
    let mut provenance = serde_json::json!({
        "resourceType": "Provenance",
        "id": "provenance-1",
        "target": [{ "reference": "Organization/placeholder" }],
    });

    let org_ids = vec!["organization-1".to_string()];
    let mut rng = rand::rng();

    overlay::overlay_cross_references(
        &mut provenance,
        "Provenance",
        "provenance-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );

    let ext = &provenance["target"][0]["extension"];
    assert!(ext.is_array(), "target.extension should be populated");
    assert_eq!(
        ext[0]["url"].as_str(),
        Some("http://hl7.org/fhir/StructureDefinition/targetPath"),
        "target.extension should be the standard targetPath extension"
    );
    assert!(
        ext[0]["valueString"].is_string(),
        "targetPath extension should carry a valueString"
    );
}

#[test]
fn organization_overlay_anchor_has_partof() {
    // The conformance must_support test queries organization-1, so it must
    // always carry partOf. It points at organization-2, which must remain a
    // root to avoid a reference cycle.
    let org_ids = vec![
        "organization-1".to_string(),
        "organization-2".to_string(),
        "organization-3".to_string(),
    ];
    let mut rng = rand::rng();

    let mut anchor = serde_json::json!({ "resourceType": "Organization", "id": "organization-1" });
    overlay::overlay_cross_references(
        &mut anchor,
        "Organization",
        "organization-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    assert_eq!(
        anchor["partOf"]["reference"].as_str(),
        Some("Organization/organization-2"),
        "organization-1 must reference organization-2 via partOf"
    );

    let mut parent = serde_json::json!({ "resourceType": "Organization", "id": "organization-2", "partOf": { "reference": "Organization/organization-1" } });
    overlay::overlay_cross_references(
        &mut parent,
        "Organization",
        "organization-2",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    assert!(
        parent.get("partOf").is_none(),
        "organization-2 must be a root (no partOf) to prevent a cycle"
    );
}

#[test]
fn healthcareservice_overlay_populates_telecom_contact_purpose() {
    // telecom.extension:contact-purpose is a mustSupport Extension field
    // the profile-driven generator skips. The overlay must add it.
    let mut hs = serde_json::json!({
        "resourceType": "HealthcareService",
        "id": "healthcareservice-1",
        "telecom": [{ "system": "phone", "value": "555-0000" }]
    });

    let org_ids = vec!["organization-1".to_string()];
    let loc_ids = vec!["location-1".to_string()];
    let endpoint_ids = vec!["endpoint-1".to_string()];
    let mut rng = rand::rng();

    overlay::overlay_cross_references(
        &mut hs,
        "HealthcareService",
        "healthcareservice-1",
        &org_ids,
        &[],
        &loc_ids,
        &[],
        &[],
        &endpoint_ids,
        &mut rng,
    );

    let ext = &hs["telecom"][0]["extension"];
    assert!(ext.is_array(), "telecom.extension should be populated");
    assert_eq!(
        ext[0]["url"].as_str(),
        Some("http://hl7.org.au/fhir/StructureDefinition/contact-purpose"),
        "telecom.extension should be the contact-purpose extension"
    );
    assert!(
        ext[0]["valueCodeableConcept"]["coding"][0]["code"].is_string(),
        "contact-purpose should carry a valueCodeableConcept coding"
    );
}

#[test]
fn overlay_adds_endpoint_links_for_include_tests() {
    let mut location = serde_json::json!({ "resourceType": "Location", "id": "location-1" });
    let mut healthcare_service =
        serde_json::json!({ "resourceType": "HealthcareService", "id": "healthcareservice-1" });
    let mut practitioner_role =
        serde_json::json!({ "resourceType": "PractitionerRole", "id": "practitionerrole-1" });

    let org_ids = vec!["organization-1".to_string()];
    let prac_ids = vec!["practitioner-1".to_string()];
    let loc_ids = vec!["location-1".to_string()];
    let hs_ids = vec!["healthcareservice-1".to_string()];
    let endpoint_ids = vec!["endpoint-1".to_string()];
    let mut rng = rand::rng();

    overlay::overlay_cross_references(
        &mut location,
        "Location",
        "location-1",
        &org_ids,
        &prac_ids,
        &loc_ids,
        &hs_ids,
        &[],
        &endpoint_ids,
        &mut rng,
    );
    overlay::overlay_cross_references(
        &mut healthcare_service,
        "HealthcareService",
        "healthcareservice-1",
        &org_ids,
        &prac_ids,
        &loc_ids,
        &hs_ids,
        &[],
        &endpoint_ids,
        &mut rng,
    );
    overlay::overlay_cross_references(
        &mut practitioner_role,
        "PractitionerRole",
        "practitionerrole-1",
        &org_ids,
        &prac_ids,
        &loc_ids,
        &hs_ids,
        &[],
        &endpoint_ids,
        &mut rng,
    );

    assert_eq!(
        location["endpoint"][0]["reference"]
            .as_str()
            .expect("should have a string value"),
        "Endpoint/endpoint-1"
    );
    assert_eq!(
        healthcare_service["endpoint"][0]["reference"]
            .as_str()
            .expect("should succeed"),
        "Endpoint/endpoint-1"
    );
    assert_eq!(
        practitioner_role["endpoint"][0]["reference"]
            .as_str()
            .expect("should succeed"),
        "Endpoint/endpoint-1"
    );
}

#[test]
fn location_one_links_to_organization_one_when_present() {
    let mut location = serde_json::json!({ "resourceType": "Location", "id": "location-1" });
    let org_ids = vec!["organization-1".to_string(), "organization-2".to_string()];
    let mut rng = rand::rng();

    overlay::overlay_cross_references(
        &mut location,
        "Location",
        "location-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );

    assert_eq!(
        location["managingOrganization"]["reference"]
            .as_str()
            .expect("should succeed"),
        "Organization/organization-1"
    );
}

#[test]
fn provenance_overlay_seeds_id_one_targets_for_revinclude_coverage() {
    let org_ids = vec!["organization-1".to_string()];
    let prac_ids = vec!["practitioner-1".to_string()];
    let loc_ids = vec!["location-1".to_string()];
    let hs_ids = vec!["healthcareservice-1".to_string()];
    let practitioner_role_ids = vec!["practitionerrole-1".to_string()];
    let mut rng = rand::rng();

    let mut p1 = serde_json::json!({ "resourceType": "Provenance", "id": "provenance-1" });
    let mut p2 = serde_json::json!({ "resourceType": "Provenance", "id": "provenance-2" });

    for p in [&mut p1, &mut p2] {
        let id = p["id"]
            .as_str()
            .expect("should have a string value")
            .to_string();
        overlay::overlay_cross_references(
            p,
            "Provenance",
            &id,
            &org_ids,
            &prac_ids,
            &loc_ids,
            &hs_ids,
            &practitioner_role_ids,
            &[],
            &mut rng,
        );
    }

    // Provenance targets now distribute across all resource types
    // for _revinclude coverage. With 5 non-empty pools and
    // provenance-1 (idx=1%5=1) → Practitioner, provenance-2 (idx=2%5=2) → Location.
    assert_eq!(
        p1["target"][0]["reference"]
            .as_str()
            .expect("should have a string value"),
        "Practitioner/practitioner-1"
    );
    assert_eq!(
        p2["target"][0]["reference"]
            .as_str()
            .expect("should have a string value"),
        "Location/location-1"
    );
}

// ── Tests for generate_supplement_resource ─────────────────────────────

#[test]
fn supplement_resource_creates_valid_fhir_json() {
    let resource = generate_supplement_resource(
        "Organization",
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("should succeed");

    assert_eq!(resource["resourceType"], "Organization");
    assert_eq!(resource["id"], "organization-1");
    assert!(resource["meta"]["profile"].as_array().is_some());
    assert!(resource["meta"]["lastUpdated"].as_str().is_some());
}

#[test]
fn supplement_resource_uses_profile_url_when_provided() {
    let mut profile_urls = HashMap::new();
    profile_urls.insert(
        "Organization".to_string(),
        "http://example.org/fhir/StructureDefinition/MyOrg".to_string(),
    );

    let resource = generate_supplement_resource(
        "Organization",
        &profile_urls,
        &[],
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("should succeed");

    let profiles = resource["meta"]["profile"]
        .as_array()
        .expect("should be an array");
    assert_eq!(
        profiles[0].as_str().expect("should have a string value"),
        "http://example.org/fhir/StructureDefinition/MyOrg"
    );
}

#[test]
fn supplement_resource_normalizes_references() {
    let resource = generate_supplement_resource(
        "PractitionerRole",
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("should succeed");

    // All references should use the {type}-1 pattern
    let practitioner_ref = resource["practitioner"]["reference"]
        .as_str()
        .expect("should have a string value");
    assert_eq!(practitioner_ref, "Practitioner/practitioner-1");

    let organization_ref = resource["organization"]["reference"]
        .as_str()
        .expect("should have a string value");
    assert_eq!(organization_ref, "Organization/organization-1");
}

#[test]
fn supplement_resource_handles_unknown_type() {
    let resource = generate_supplement_resource(
        "UnknownType",
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("should succeed");

    assert_eq!(resource["resourceType"], "UnknownType");
    assert_eq!(resource["id"], "unknowntype-1");
    assert_eq!(resource["status"], "active");
}

// ── Tests for write_supplement_ndjson ──────────────────────────────────

#[test]
fn write_supplement_creates_files_for_uncovered_types() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    // Create bulk data for Organization only
    let mut bulk_counts = HashMap::new();
    bulk_counts.insert("Organization".to_string(), 5);

    // Creation order includes types not in bulk_counts
    let creation_order = bulk_data_creation_order(&bulk_counts);

    let supplement_ids = write_supplement_ndjson(
        &creation_order,
        &bulk_counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // Supplement IDs should only include types not in bulk_counts
    // (and not in NON_RESOURCE_TYPES)
    assert!(
        !supplement_ids.contains_key("Organization"),
        "Organization has bulk count, should not be in supplement"
    );

    // Each supplement type should have its own NDJSON file
    for (resource_type, ids) in &supplement_ids {
        let path = dir
            .path()
            .join("data")
            .join(format!("{}.ndjson", resource_type));
        assert!(path.exists(), "{}.ndjson should exist", resource_type);
        let contents = std::fs::read_to_string(&path).expect("should read file");
        let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 1, "{} should have 1 line", resource_type);
        assert_eq!(ids.len(), 1, "{} should have 1 ID", resource_type);

        let parsed: serde_json::Value =
            serde_json::from_str(lines[0]).expect("should parse valid JSON");
        assert_eq!(parsed["resourceType"], *resource_type);
        assert_eq!(parsed["id"], format!("{}-1", resource_type.to_lowercase()));
    }
}

#[test]
fn write_supplement_skips_non_resource_types() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    let mut bulk_counts = HashMap::new();
    bulk_counts.insert("Organization".to_string(), 5);

    // Include a non-resource type in the creation order
    let mut creation_order = bulk_data_creation_order(&bulk_counts);
    creation_order.push("Extension".to_string());

    let supplement_ids = write_supplement_ndjson(
        &creation_order,
        &bulk_counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // Extension should be skipped
    assert!(
        !supplement_ids.contains_key("Extension"),
        "Extension is a non-resource type and should be skipped"
    );
}

#[test]
fn write_supplement_appends_to_combined_ndjson() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    // First write bulk data for Organization only
    let mut bulk_counts = HashMap::new();
    bulk_counts.insert("Organization".to_string(), 2);

    // Add a type that has no bulk count so it becomes a supplement
    let mut creation_order = bulk_data_creation_order(&bulk_counts);
    creation_order.push("Patient".to_string());

    generate_bulk_data(
        &bulk_counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // Then write supplements for uncovered types
    write_supplement_ndjson(
        &creation_order,
        &bulk_counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // combined.ndjson should have bulk + supplement resources
    let combined_path = dir.path().join("data/combined.ndjson");
    let contents = std::fs::read_to_string(&combined_path).expect("should read file");
    let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();

    // At least 2 bulk lines + supplement lines
    assert!(
        lines.len() > 2,
        "combined.ndjson should have bulk + supplement resources"
    );
}

// ── Tests for generate_update_ndjson ───────────────────────────────────

#[test]
fn update_ndjson_creates_file_with_same_count() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 5);
    counts.insert("Practitioner".to_string(), 10);

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    generate_update_ndjson(&ids, dir.path()).expect("should generate bulk data");

    let update_path = dir.path().join("data/update.ndjson");
    assert!(update_path.exists(), "update.ndjson should exist");

    let contents = std::fs::read_to_string(&update_path).expect("should read file");
    let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();

    // Should have same number of resources as bulk data
    let total_bulk: usize = counts.values().sum::<u64>() as usize;
    assert_eq!(
        lines.len(),
        total_bulk,
        "update.ndjson should have {} lines",
        total_bulk
    );
}

#[test]
fn update_ndjson_resources_differ_from_originals() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 3);

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // Read original resources
    let orig_path = dir.path().join("data/Organization.ndjson");
    let orig_contents = std::fs::read_to_string(&orig_path).expect("should read file");
    let orig_lines: Vec<&str> = orig_contents.lines().filter(|l| !l.is_empty()).collect();

    generate_update_ndjson(&ids, dir.path()).expect("should generate bulk data");

    // Read updated resources
    let update_path = dir.path().join("data/update.ndjson");
    let update_contents = std::fs::read_to_string(&update_path).expect("should read file");
    let update_lines: Vec<&str> = update_contents.lines().filter(|l| !l.is_empty()).collect();

    assert_eq!(orig_lines.len(), update_lines.len());

    // Each updated resource should have the same id but different content
    for (orig_line, update_line) in orig_lines.iter().zip(update_lines.iter()) {
        let orig: serde_json::Value =
            serde_json::from_str(orig_line).expect("should parse valid JSON");
        let updated: serde_json::Value =
            serde_json::from_str(update_line).expect("should parse valid JSON");

        // Same id
        assert_eq!(orig["id"], updated["id"]);

        // Different content (at least one field should have changed)
        assert_ne!(
            orig, updated,
            "Updated resource should differ from original"
        );
    }
}

#[test]
fn update_ndjson_preserves_resource_type_and_id() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 2);
    counts.insert("Practitioner".to_string(), 2);

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    generate_update_ndjson(&ids, dir.path()).expect("should generate bulk data");

    let update_path = dir.path().join("data/update.ndjson");
    let contents = std::fs::read_to_string(&update_path).expect("should read file");

    for line in contents.lines().filter(|l| !l.is_empty()) {
        let resource: serde_json::Value =
            serde_json::from_str(line).expect("should parse valid JSON");
        let rtype = resource["resourceType"]
            .as_str()
            .expect("should have a string value");
        let id = resource["id"].as_str().expect("should have a string value");

        // resourceType and id should be preserved
        assert!(!rtype.is_empty());
        assert!(!id.is_empty());

        // id should match the pattern {type}-{n}
        assert!(id.starts_with(&rtype.to_lowercase()));
    }
}

#[test]
fn update_ndjson_handles_empty_ids() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let ids = IdStore::new();

    // Create the data directory so the function can write to it
    std::fs::create_dir_all(dir.path().join("data")).expect("should create directory");

    // Should not error when there are no IDs
    generate_update_ndjson(&ids, dir.path()).expect("should generate bulk data");

    let update_path = dir.path().join("data/update.ndjson");
    assert!(update_path.exists(), "update.ndjson should exist");
    let contents = std::fs::read_to_string(&update_path).expect("should read file");
    assert!(contents.trim().is_empty(), "update.ndjson should be empty");
}

// ── Additional generate_bulk_data tests ────────────────────────────────

#[test]
fn bulk_data_handles_empty_counts() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let counts = HashMap::new();

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    assert!(
        ids.is_empty(),
        "No resources should be generated for empty counts"
    );
}

#[test]
fn bulk_data_creates_combined_ndjson() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 3);
    counts.insert("Practitioner".to_string(), 2);

    generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    let combined_path = dir.path().join("data/combined.ndjson");
    assert!(combined_path.exists(), "combined.ndjson should exist");

    let contents = std::fs::read_to_string(&combined_path).expect("should read file");
    let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
    let total: usize = counts.values().sum::<u64>() as usize;
    assert_eq!(
        lines.len(),
        total,
        "combined.ndjson should have all resources"
    );
}

#[test]
fn bulk_data_stamps_created_date() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 5);

    generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    let path = dir.path().join("data/Organization.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");

    for line in contents.lines().filter(|l| !l.is_empty()) {
        let org: serde_json::Value = serde_json::from_str(line).expect("should parse valid JSON");
        let last_updated = org["meta"]["lastUpdated"]
            .as_str()
            .expect("should have a string value");
        assert!(!last_updated.is_empty(), "meta.lastUpdated should be set");
        // Should be an ISO timestamp
        assert!(
            last_updated.contains('T'),
            "meta.lastUpdated should be an ISO timestamp, got: {}",
            last_updated
        );
    }
}

#[test]
fn bulk_data_creation_order_includes_all_types() {
    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 5);
    counts.insert("Practitioner".to_string(), 5);
    counts.insert("Endpoint".to_string(), 5);
    counts.insert("Location".to_string(), 5);
    counts.insert("HealthcareService".to_string(), 5);
    counts.insert("PractitionerRole".to_string(), 5);
    counts.insert("Provenance".to_string(), 5);
    counts.insert("Patient".to_string(), 5);

    let order = bulk_data_creation_order(&counts);

    // All types should be in the order
    for t in counts.keys() {
        assert!(order.contains(t), "{} should be in creation order", t);
    }

    // Order should respect dependencies
    let org_idx = order
        .iter()
        .position(|t| t == "Organization")
        .expect("type should be in creation order");
    let prac_idx = order
        .iter()
        .position(|t| t == "Practitioner")
        .expect("type should be in creation order");
    let endpoint_idx = order
        .iter()
        .position(|t| t == "Endpoint")
        .expect("type should be in creation order");
    let loc_idx = order
        .iter()
        .position(|t| t == "Location")
        .expect("type should be in creation order");
    let hs_idx = order
        .iter()
        .position(|t| t == "HealthcareService")
        .expect("type should be in creation order");
    let pr_idx = order
        .iter()
        .position(|t| t == "PractitionerRole")
        .expect("type should be in creation order");
    let prov_idx = order
        .iter()
        .position(|t| t == "Provenance")
        .expect("type should be in creation order");

    assert!(org_idx < endpoint_idx, "Organization before Endpoint");
    assert!(endpoint_idx < loc_idx, "Endpoint before Location");
    assert!(loc_idx < hs_idx, "Location before HealthcareService");
    assert!(hs_idx < pr_idx, "HealthcareService before PractitionerRole");
    assert!(prac_idx < pr_idx, "Practitioner before PractitionerRole");
    assert!(pr_idx < prov_idx, "PractitionerRole before Provenance");
}

// ── Tests for stamp_created_date ────────────────────────────────────────

#[test]
fn stamp_created_date_sets_last_updated() {
    let mut resource = serde_json::json!({ "resourceType": "Patient", "id": "patient-1" });
    let mut rng = rand::rng();
    stamp_created_date(&mut resource, &mut rng);
    let last_updated = resource["meta"]["lastUpdated"]
        .as_str()
        .expect("should have a string value");
    assert!(!last_updated.is_empty(), "meta.lastUpdated should be set");
    assert!(
        last_updated.contains('T'),
        "meta.lastUpdated should be an ISO timestamp, got: {}",
        last_updated
    );
}

// ── Tests for random_ref ────────────────────────────────────────────────

#[test]
fn random_ref_with_empty_ids_returns_placeholder() {
    let ids: Vec<String> = vec![];
    let mut rng = rand::rng();
    let result = random_ref("Organization", &ids, &mut rng);
    assert_eq!(result, "Organization/placeholder-1");
}

#[test]
fn random_ref_with_ids_returns_valid_reference() {
    let ids = vec!["org-1".to_string(), "org-2".to_string()];
    let mut rng = rand::rng();
    let result = random_ref("Organization", &ids, &mut rng);
    assert!(result.starts_with("Organization/"));
    assert!(
        result == "Organization/org-1" || result == "Organization/org-2",
        "Expected Organization/org-1 or Organization/org-2, got {}",
        result
    );
}

// ── Tests for random_refs (dead_code) ───────────────────────────────────

#[test]
fn random_refs_with_empty_ids_returns_placeholder() {
    let ids: Vec<String> = vec![];
    let mut rng = rand::rng();
    let result = random_refs("Organization", &ids, 3, &mut rng);
    assert_eq!(result, vec!["Organization/placeholder-1".to_string()]);
}

#[test]
fn random_refs_with_ids_returns_correct_count() {
    let ids = vec![
        "org-1".to_string(),
        "org-2".to_string(),
        "org-3".to_string(),
        "org-4".to_string(),
        "org-5".to_string(),
    ];
    let mut rng = rand::rng();
    let result = random_refs("Organization", &ids, 3, &mut rng);
    assert_eq!(result.len(), 3);
    for r in &result {
        assert!(r.starts_with("Organization/"));
    }
}

#[test]
fn random_refs_does_not_exceed_available_ids() {
    let ids = vec!["org-1".to_string(), "org-2".to_string()];
    let mut rng = rand::rng();
    let result = random_refs("Organization", &ids, 10, &mut rng);
    assert_eq!(result.len(), 2);
}

// ── Tests for Endpoint and Provenance in bulk data ──────────────────────

#[test]
fn bulk_data_generates_endpoint() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Endpoint".to_string(), 3);

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    assert_eq!(ids.get("Endpoint").expect("key should exist").len(), 3);

    let path = dir.path().join("data/Endpoint.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 3);

    for line in &lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("should parse valid JSON");
        assert_eq!(parsed["resourceType"], "Endpoint");
        assert!(
            parsed["id"]
                .as_str()
                .expect("should have id")
                .starts_with("endpoint-")
        );
        assert!(parsed["status"].as_str().is_some());
    }
}

#[test]
fn bulk_data_generates_provenance() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Provenance".to_string(), 3);
    counts.insert("Organization".to_string(), 2);

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    assert_eq!(ids.get("Provenance").expect("key should exist").len(), 3);

    let path = dir.path().join("data/Provenance.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 3);

    for line in &lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("should parse valid JSON");
        assert_eq!(parsed["resourceType"], "Provenance");
        assert!(
            parsed["id"]
                .as_str()
                .expect("should have id")
                .starts_with("provenance-")
        );
    }
}

// ── Tests for overlay edge cases ────────────────────────────────────────

#[test]
fn overlay_handles_non_object_resource() {
    let mut resource = serde_json::Value::String("not an object".to_string());
    let mut rng = rand::rng();
    // Should not panic
    overlay::overlay_cross_references(
        &mut resource,
        "PractitionerRole",
        "practitionerrole-1",
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // Resource should be unchanged
    assert_eq!(
        resource,
        serde_json::Value::String("not an object".to_string())
    );
}

#[test]
fn overlay_practitionerrole_with_empty_ids_does_not_add_fields() {
    let mut pr =
        serde_json::json!({ "resourceType": "PractitionerRole", "id": "practitionerrole-1" });
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut pr,
        "PractitionerRole",
        "practitionerrole-1",
        &[], // no org_ids
        &[], // no prac_ids
        &[], // no loc_ids
        &[], // no hs_ids
        &[], // no practitioner_role_ids
        &[], // no endpoint_ids
        &mut rng,
    );
    // Should not add any reference fields when all ID lists are empty
    assert!(pr.get("practitioner").is_none());
    assert!(pr.get("organization").is_none());
    assert!(pr.get("location").is_none());
    assert!(pr.get("healthcareService").is_none());
    assert!(pr.get("endpoint").is_none());
}

#[test]
fn overlay_healthcareservice_without_org_does_not_add_provided_by() {
    let mut hs =
        serde_json::json!({ "resourceType": "HealthcareService", "id": "healthcareservice-1" });
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut hs,
        "HealthcareService",
        "healthcareservice-1",
        &[], // no org_ids
        &[],
        &["location-1".to_string()],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // Should not add providedBy when no org_ids
    assert!(hs.get("providedBy").is_none());
    // Should still add location
    assert!(hs.get("location").is_some());
}

#[test]
fn overlay_healthcareservice_coverage_area_uses_location() {
    let mut hs =
        serde_json::json!({ "resourceType": "HealthcareService", "id": "healthcareservice-1" });
    let org_ids = vec!["organization-1".to_string()];
    let loc_ids = vec!["location-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut hs,
        "HealthcareService",
        "healthcareservice-1",
        &org_ids,
        &[],
        &loc_ids,
        &[],
        &[],
        &[],
        &mut rng,
    );
    // coverageArea should reference Location
    let ca = hs["coverageArea"][0]["reference"]
        .as_str()
        .expect("should have a string value");
    assert!(
        ca.starts_with("Location/"),
        "coverageArea should reference Location, got: {}",
        ca
    );
}

#[test]
fn overlay_healthcareservice_removes_coverage_area_when_no_location() {
    let mut hs = serde_json::json!({
        "resourceType": "HealthcareService",
        "id": "healthcareservice-1",
        "coverageArea": [{ "reference": "Location/some-location" }]
    });
    let org_ids = vec!["organization-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut hs,
        "HealthcareService",
        "healthcareservice-1",
        &org_ids,
        &[],
        &[], // no loc_ids
        &[],
        &[],
        &[],
        &mut rng,
    );
    // coverageArea should be removed when no Location IDs exist
    assert!(hs.get("coverageArea").is_none());
}

#[test]
fn overlay_endpoint_adds_managing_organization() {
    let mut endpoint = serde_json::json!({ "resourceType": "Endpoint", "id": "endpoint-1" });
    let org_ids = vec!["organization-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut endpoint,
        "Endpoint",
        "endpoint-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    let org_ref = endpoint["managingOrganization"]["reference"]
        .as_str()
        .expect("should have a string value");
    assert!(org_ref.starts_with("Organization/"));
}

#[test]
fn overlay_endpoint_without_org_does_not_add_managing_organization() {
    let mut endpoint = serde_json::json!({ "resourceType": "Endpoint", "id": "endpoint-1" });
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut endpoint,
        "Endpoint",
        "endpoint-1",
        &[], // no org_ids
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    assert!(endpoint.get("managingOrganization").is_none());
}

#[test]
fn overlay_location_without_org_does_not_add_managing_organization() {
    let mut location = serde_json::json!({ "resourceType": "Location", "id": "location-1" });
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut location,
        "Location",
        "location-1",
        &[], // no org_ids
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    assert!(location.get("managingOrganization").is_none());
}

#[test]
fn overlay_location_without_endpoint_does_not_add_endpoint() {
    let mut location = serde_json::json!({ "resourceType": "Location", "id": "location-1" });
    let org_ids = vec!["organization-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut location,
        "Location",
        "location-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[], // no endpoint_ids
        &mut rng,
    );
    // Should have managingOrganization but not endpoint
    assert!(location.get("managingOrganization").is_some());
    assert!(location.get("endpoint").is_none());
}

#[test]
fn overlay_organization_anchor_without_parent_removes_partof() {
    // When organization-1 exists but organization-2 does not, partOf should be removed
    let org_ids = vec!["organization-1".to_string()];
    let mut rng = rand::rng();
    let mut anchor = serde_json::json!({ "resourceType": "Organization", "id": "organization-1", "partOf": { "reference": "Organization/organization-2" } });
    overlay::overlay_cross_references(
        &mut anchor,
        "Organization",
        "organization-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // partOf should be removed since organization-2 doesn't exist
    assert!(anchor.get("partOf").is_none());
}

#[test]
fn overlay_organization_non_anchor_removes_partof_when_no_earlier_orgs() {
    // organization-1 is the first org, so it should not get partOf (it's the anchor)
    // organization-2 is the anchor parent, so it should not get partOf
    // Test organization-3 with only org-1 and org-2 in the list
    let org_ids = vec!["organization-1".to_string(), "organization-2".to_string()];
    let mut rng = rand::rng();
    let mut org = serde_json::json!({ "resourceType": "Organization", "id": "organization-3" });
    overlay::overlay_cross_references(
        &mut org,
        "Organization",
        "organization-3",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // organization-3 is not in org_ids, so self_index will be None -> partOf removed
    assert!(org.get("partOf").is_none());
}

#[test]
fn overlay_provenance_without_target_creates_target() {
    let mut provenance = serde_json::json!({
        "resourceType": "Provenance",
        "id": "provenance-1",
    });
    let org_ids = vec!["organization-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut provenance,
        "Provenance",
        "provenance-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // Should create a target with reference and extension
    let target = &provenance["target"];
    assert!(target.is_array(), "target should be an array");
    assert!(
        !target.as_array().unwrap().is_empty(),
        "target should not be empty"
    );
    assert!(target[0]["reference"].as_str().is_some());
    assert!(
        target[0]["extension"].is_array(),
        "target.extension should be populated"
    );
}

#[test]
fn overlay_provenance_without_org_does_not_add_agent_or_entity() {
    let mut provenance = serde_json::json!({
        "resourceType": "Provenance",
        "id": "provenance-1",
    });
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut provenance,
        "Provenance",
        "provenance-1",
        &[], // no org_ids
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // Should not add agent or entity when no org_ids
    assert!(provenance.get("agent").is_none());
    assert!(provenance.get("entity").is_none());
    // Should still have activity
    assert!(provenance.get("activity").is_some());
}

#[test]
fn overlay_provenance_preserves_existing_target_extension() {
    let mut provenance = serde_json::json!({
        "resourceType": "Provenance",
        "id": "provenance-1",
        "target": [{
            "reference": "Organization/placeholder",
            "extension": [{
                "url": "http://example.org/custom-extension",
                "valueString": "custom"
            }]
        }]
    });
    let org_ids = vec!["organization-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut provenance,
        "Provenance",
        "provenance-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // The existing extension should be preserved (not replaced by targetPath)
    let ext = &provenance["target"][0]["extension"];
    assert!(ext.is_array());
    assert_eq!(
        ext[0]["url"].as_str(),
        Some("http://example.org/custom-extension"),
        "Existing extension should be preserved"
    );
}

#[test]
fn overlay_provenance_no_non_empty_pools_returns_none_target() {
    let mut provenance = serde_json::json!({
        "resourceType": "Provenance",
        "id": "provenance-1",
    });
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut provenance,
        "Provenance",
        "provenance-1",
        &[], // all empty
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // Should not have target when all pools are empty
    assert!(provenance.get("target").is_none());
}

// ── Tests for ensure_telecom_contact_purpose edge cases ─────────────────

#[test]
fn overlay_healthcareservice_telecom_empty_array_adds_default() {
    let mut hs = serde_json::json!({
        "resourceType": "HealthcareService",
        "id": "healthcareservice-1",
        "telecom": []
    });
    let org_ids = vec!["organization-1".to_string()];
    let loc_ids = vec!["location-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut hs,
        "HealthcareService",
        "healthcareservice-1",
        &org_ids,
        &[],
        &loc_ids,
        &[],
        &[],
        &[],
        &mut rng,
    );
    // Should add a default telecom entry with contact-purpose extension
    let telecom = hs["telecom"].as_array().expect("should be an array");
    assert!(!telecom.is_empty(), "telecom should not be empty");
    assert!(
        telecom[0]["extension"].is_array(),
        "telecom.extension should be populated"
    );
}

#[test]
fn overlay_healthcareservice_telecom_non_array_unchanged() {
    let mut hs = serde_json::json!({
        "resourceType": "HealthcareService",
        "id": "healthcareservice-1",
        "telecom": "not-an-array"
    });
    let org_ids = vec!["organization-1".to_string()];
    let loc_ids = vec!["location-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut hs,
        "HealthcareService",
        "healthcareservice-1",
        &org_ids,
        &[],
        &loc_ids,
        &[],
        &[],
        &[],
        &mut rng,
    );
    // telecom should remain unchanged (non-array)
    assert_eq!(hs["telecom"], "not-an-array");
}

#[test]
fn overlay_healthcareservice_telecom_with_existing_extension_preserved() {
    let mut hs = serde_json::json!({
        "resourceType": "HealthcareService",
        "id": "healthcareservice-1",
        "telecom": [{
            "system": "phone",
            "value": "555-0000",
            "extension": [{
                "url": "http://example.org/existing",
                "valueString": "existing"
            }]
        }]
    });
    let org_ids = vec!["organization-1".to_string()];
    let loc_ids = vec!["location-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut hs,
        "HealthcareService",
        "healthcareservice-1",
        &org_ids,
        &[],
        &loc_ids,
        &[],
        &[],
        &[],
        &mut rng,
    );
    // Existing extension should be preserved
    let ext = &hs["telecom"][0]["extension"];
    assert_eq!(
        ext[0]["url"].as_str(),
        Some("http://example.org/existing"),
        "Existing extension should be preserved"
    );
}

// ── Tests for supplement edge cases ──────────────────────────────────────

#[test]
fn normalize_supplement_references_maps_resource_to_organization() {
    let mut value = serde_json::json!({
        "reference": "Resource/some-uuid"
    });
    normalize_supplement_references(&mut value);
    assert_eq!(
        value["reference"].as_str(),
        Some("Organization/organization-1"),
        "Resource type should be mapped to Organization"
    );
}

#[test]
fn normalize_supplement_references_handles_nested_objects() {
    let mut value = serde_json::json!({
        "contained": [{
            "reference": "Practitioner/abc-123"
        }],
        "managingOrganization": {
            "reference": "Organization/xyz-789"
        }
    });
    normalize_supplement_references(&mut value);
    assert_eq!(
        value["contained"][0]["reference"].as_str(),
        Some("Practitioner/practitioner-1")
    );
    assert_eq!(
        value["managingOrganization"]["reference"].as_str(),
        Some("Organization/organization-1")
    );
}

#[test]
fn normalize_supplement_references_skips_non_reference_values() {
    let mut value = serde_json::json!({
        "name": "Test",
        "active": true,
        "count": 42
    });
    normalize_supplement_references(&mut value);
    assert_eq!(value["name"], "Test");
    assert_eq!(value["active"], true);
    assert_eq!(value["count"], 42);
}

#[test]
fn normalize_supplement_references_handles_http_urls() {
    let mut value = serde_json::json!({
        "reference": "http://example.org/Patient/some-id"
    });
    normalize_supplement_references(&mut value);
    // HTTP URLs are split on '/', so rtype = "http:" which gets mapped
    // to "http:/http:-1" — this is the current behavior
    let ref_str = value["reference"]
        .as_str()
        .expect("should have a string value");
    assert!(!ref_str.is_empty(), "reference should not be empty");
}

// ── Tests for update edge cases ─────────────────────────────────────────

#[test]
fn update_ndjson_skips_missing_ndjson_files() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut ids = IdStore::new();
    ids.insert(
        "NonExistentType".to_string(),
        vec!["nonexistent-1".to_string()],
    );

    // Create data directory but no NDJSON file
    std::fs::create_dir_all(dir.path().join("data")).expect("should create directory");

    // Should not error — should skip types whose NDJSON files don't exist
    generate_update_ndjson(&ids, dir.path()).expect("should succeed");

    let update_path = dir.path().join("data/update.ndjson");
    assert!(update_path.exists(), "update.ndjson should exist");
    let contents = std::fs::read_to_string(&update_path).expect("should read file");
    assert!(contents.trim().is_empty(), "update.ndjson should be empty");
}

#[test]
fn update_ndjson_with_supplement_ids() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    // Generate bulk data for Organization
    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 2);

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // Add a supplement ID (simulating what write_supplement_ndjson does)
    let mut all_ids = ids;
    all_ids.insert("Patient".to_string(), vec!["patient-1".to_string()]);

    // Write a Patient.ndjson file (simulating supplement output)
    let patient_resource = serde_json::json!({
        "resourceType": "Patient",
        "id": "patient-1",
        "meta": { "profile": ["http://hl7.org/fhir/StructureDefinition/Patient"] },
        "status": "active"
    });
    let patient_path = dir.path().join("data/Patient.ndjson");
    let mut writer = std::io::BufWriter::new(std::fs::File::create(&patient_path).unwrap());
    serde_json::to_writer(&mut writer, &patient_resource).unwrap();
    writeln!(writer).unwrap();
    writer.flush().unwrap();

    generate_update_ndjson(&all_ids, dir.path()).expect("should succeed");

    let update_path = dir.path().join("data/update.ndjson");
    let contents = std::fs::read_to_string(&update_path).expect("should read file");
    let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
    // Should have 3 lines: 2 Organization + 1 Patient
    assert_eq!(lines.len(), 3, "update.ndjson should have 3 lines");
}

// ── Tests for discover_mutable_paths and walk_for_mutables ──────────────

#[test]
fn discover_mutable_paths_finds_string_number_and_bool() {
    let resource = serde_json::json!({
        "resourceType": "Patient",
        "id": "patient-1",
        "name": "John",
        "age": 30,
        "active": true,
        "meta": { "versionId": "1" }
    });
    let candidates = discover_mutable_paths(&resource);
    // Should find: name (string), age (number), active (bool)
    // Should skip: resourceType, id, meta
    assert_eq!(candidates.len(), 3, "Should find 3 mutable paths");
    let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"name"));
    assert!(paths.contains(&"age"));
    assert!(paths.contains(&"active"));
}

#[test]
fn discover_mutable_paths_skips_reference_fields() {
    let resource = serde_json::json!({
        "resourceType": "Patient",
        "id": "patient-1",
        "managingOrganization": {
            "reference": "Organization/org-1"
        },
        "name": "John"
    });
    let candidates = discover_mutable_paths(&resource);
    // Should find: name (string)
    // Should skip: managingOrganization (contains reference), resourceType, id
    let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"name"), "Should find 'name'");
    assert!(
        !paths.contains(&"managingOrganization"),
        "Should skip managingOrganization"
    );
}

#[test]
fn discover_mutable_paths_handles_nested_objects() {
    let resource = serde_json::json!({
        "resourceType": "Patient",
        "id": "patient-1",
        "name": [{
            "family": "Smith",
            "given": ["John"]
        }]
    });
    let candidates = discover_mutable_paths(&resource);
    // Should find: name[0].family (string)
    // Should NOT find: name[0].given (array of primitives, skipped)
    let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
    assert!(
        paths.contains(&"name[0].family"),
        "Should find name[0].family"
    );
}

#[test]
fn discover_mutable_paths_empty_resource() {
    let resource = serde_json::json!({ "resourceType": "Patient", "id": "patient-1" });
    let candidates = discover_mutable_paths(&resource);
    assert!(candidates.is_empty(), "Should find no mutable paths");
}

#[test]
fn is_reference_field_identifies_references() {
    let ref_val = serde_json::Value::String("Organization/org-1".to_string());
    assert!(is_reference_field("reference", &ref_val));
    let non_ref = serde_json::Value::String("John".to_string());
    assert!(!is_reference_field("reference", &non_ref));
    let http_ref = serde_json::Value::String("http://example.org/Patient/1".to_string());
    assert!(!is_reference_field("reference", &http_ref));
    let not_ref_key = serde_json::Value::String("Organization/org-1".to_string());
    assert!(!is_reference_field("name", &not_ref_key));
}

#[test]
fn get_at_path_handles_dotted_and_array_paths() {
    let value = serde_json::json!({
        "name": [{
            "family": "Smith",
            "given": ["John"]
        }],
        "address": {
            "city": "Sydney"
        }
    });
    assert_eq!(
        get_at_path(&value, "name[0].family").and_then(|v| v.as_str()),
        Some("Smith")
    );
    assert_eq!(
        get_at_path(&value, "address.city").and_then(|v| v.as_str()),
        Some("Sydney")
    );
    assert!(get_at_path(&value, "nonexistent").is_none());
    assert!(get_at_path(&value, "name[999]").is_none());
}

#[test]
fn set_at_path_modifies_nested_values() {
    let mut value = serde_json::json!({
        "name": [{
            "family": "Smith"
        }],
        "address": {
            "city": "Sydney"
        }
    });
    set_at_path(&mut value, "name[0].family", serde_json::json!("Jones"));
    assert_eq!(value["name"][0]["family"], "Jones");
    set_at_path(&mut value, "address.city", serde_json::json!("Melbourne"));
    assert_eq!(value["address"]["city"], "Melbourne");
}

#[test]
fn mutate_string_changes_value() {
    let mut value = serde_json::json!({
        "name": "John"
    });
    let mut rng = rand::rng();
    mutate_string(&mut value, "name", &mut rng);
    let new_name = value["name"].as_str().expect("should be a string");
    assert!(
        !new_name.is_empty(),
        "name should not be empty after mutation"
    );
}

#[test]
fn mutate_number_changes_value() {
    let mut value = serde_json::json!({
        "age": 30
    });
    let mut rng = rand::rng();
    mutate_number(&mut value, "age", &mut rng);
    let new_age = value["age"].as_f64().expect("should be a number");
    // Should be within ±10% of 30
    assert!(
        (27.0..=33.0).contains(&new_age),
        "age should be within 10% of 30, got {}",
        new_age
    );
}

#[test]
fn mutate_number_with_zero_default() {
    let mut value = serde_json::json!({
        "count": "not-a-number"
    });
    let mut rng = rand::rng();
    mutate_number(&mut value, "count", &mut rng);
    // Should use 0.0 as default and jitter around it
    let new_count = value["count"].as_f64().expect("should be a number");
    assert!(
        (-0.1..=0.1).contains(&new_count),
        "count should be near 0, got {}",
        new_count
    );
}

#[test]
fn mutate_bool_flips_value() {
    let mut value = serde_json::json!({
        "active": true
    });
    let mut rng = rand::rng();
    mutate_bool(&mut value, "active", &mut rng);
    assert_eq!(value["active"], false, "bool should be flipped to false");
    mutate_bool(&mut value, "active", &mut rng);
    assert_eq!(value["active"], true, "bool should be flipped back to true");
}

#[test]
fn mutate_bool_with_missing_value_defaults_to_true() {
    let mut value = serde_json::json!({
        "active": "not-a-bool"
    });
    let mut rng = rand::rng();
    mutate_bool(&mut value, "active", &mut rng);
    // Should default to true, then flip to false
    assert_eq!(value["active"], false);
}

#[test]
fn apply_random_updates_does_not_change_id_or_resource_type() {
    let mut resource = serde_json::json!({
        "resourceType": "Organization",
        "id": "organization-1",
        "name": "Test Org",
        "active": true
    });
    let mut rng = rand::rng();
    apply_random_updates(&mut resource, "Organization", &mut rng);
    assert_eq!(resource["resourceType"], "Organization");
    assert_eq!(resource["id"], "organization-1");
}

#[test]
fn apply_random_updates_handles_resource_with_no_mutable_fields() {
    let mut resource = serde_json::json!({
        "resourceType": "Patient",
        "id": "patient-1",
        "meta": { "versionId": "1" }
    });
    let mut rng = rand::rng();
    // Should not panic
    apply_random_updates(&mut resource, "Patient", &mut rng);
    assert_eq!(resource["resourceType"], "Patient");
    assert_eq!(resource["id"], "patient-1");
}

// ── Tests for profile-aware generation with profiles ────────────────────

#[test]
fn bulk_data_with_profile_uses_profile_aware_generation() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Patient".to_string(), 2);

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

    let mut profile_urls = HashMap::new();
    profile_urls.insert(
        "Patient".to_string(),
        "http://example.org/fhir/StructureDefinition/MyPatient".to_string(),
    );

    let ids = generate_bulk_data(
        &counts,
        &profile_urls,
        &[profile],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    assert_eq!(ids.get("Patient").expect("key should exist").len(), 2);

    let path = dir.path().join("data/Patient.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    for line in contents.lines().filter(|l| !l.is_empty()) {
        let patient: serde_json::Value =
            serde_json::from_str(line).expect("should parse valid JSON");
        assert_eq!(patient["resourceType"], "Patient");
        // Profile-aware generation should set meta.profile from the StructureDefinition
        let profiles = patient["meta"]["profile"]
            .as_array()
            .expect("should be an array");
        assert_eq!(
            profiles[0].as_str().expect("should have a string value"),
            "http://example.org/fhir/StructureDefinition/MyPatient"
        );
    }
}

#[test]
fn bulk_data_with_profile_url_but_no_profile_falls_back_to_generic() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let mut counts = HashMap::new();
    counts.insert("Patient".to_string(), 2);

    let mut profile_urls = HashMap::new();
    profile_urls.insert(
        "Patient".to_string(),
        "http://example.org/fhir/StructureDefinition/MyPatient".to_string(),
    );

    // No profiles provided — should fall back to generic generation
    // but still stamp the profile URL from profile_urls
    let ids = generate_bulk_data(
        &counts,
        &profile_urls,
        &[], // no profiles
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    assert_eq!(ids.get("Patient").expect("key should exist").len(), 2);

    let path = dir.path().join("data/Patient.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    for line in contents.lines().filter(|l| !l.is_empty()) {
        let patient: serde_json::Value =
            serde_json::from_str(line).expect("should parse valid JSON");
        assert_eq!(patient["resourceType"], "Patient");
        // Should use the profile URL from profile_urls
        let profiles = patient["meta"]["profile"]
            .as_array()
            .expect("should be an array");
        assert_eq!(
            profiles[0].as_str().expect("should have a string value"),
            "http://example.org/fhir/StructureDefinition/MyPatient"
        );
    }
}

// ── Tests for bulk_data_creation_order with empty counts ─────────────────

#[test]
fn bulk_data_creation_order_empty_counts() {
    let counts = HashMap::new();
    let order = bulk_data_creation_order(&counts);
    assert!(order.is_empty(), "Order should be empty for empty counts");
}

#[test]
fn bulk_data_creation_order_with_unknown_types() {
    let mut counts = HashMap::new();
    counts.insert("UnknownType".to_string(), 5);
    let order = bulk_data_creation_order(&counts);
    assert_eq!(order.len(), 1);
    assert_eq!(order[0], "UnknownType");
}

// ── Tests for write_supplement_ndjson with profile-aware generation ─────

#[test]
fn write_supplement_with_profile_uses_profile_aware_generation() {
    let dir = tempfile::tempdir().expect("should create temp dir");

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

    let mut profile_urls = HashMap::new();
    profile_urls.insert(
        "Patient".to_string(),
        "http://example.org/fhir/StructureDefinition/MyPatient".to_string(),
    );

    let creation_order = vec!["Patient".to_string()];
    let bulk_counts = HashMap::new();

    let supplement_ids = write_supplement_ndjson(
        &creation_order,
        &bulk_counts,
        &profile_urls,
        &[profile],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    assert!(supplement_ids.contains_key("Patient"));
    assert_eq!(supplement_ids["Patient"].len(), 1);

    let path = dir.path().join("data/Patient.ndjson");
    let contents = std::fs::read_to_string(&path).expect("should read file");
    let parsed: serde_json::Value =
        serde_json::from_str(contents.trim()).expect("should parse valid JSON");
    assert_eq!(parsed["resourceType"], "Patient");
    assert_eq!(parsed["id"], "patient-1");
}

// ── Tests for generate_supplement_resource with profile ─────────────────

#[test]
fn supplement_resource_with_profile_uses_profile_aware_generation() {
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

    let mut profile_urls = HashMap::new();
    profile_urls.insert(
        "Patient".to_string(),
        "http://example.org/fhir/StructureDefinition/MyPatient".to_string(),
    );

    let resource = generate_supplement_resource(
        "Patient",
        &profile_urls,
        &[profile],
        &HashMap::new(),
        &HashMap::new(),
    )
    .expect("should succeed");

    assert_eq!(resource["resourceType"], "Patient");
    assert_eq!(resource["id"], "patient-1");
    let profiles = resource["meta"]["profile"]
        .as_array()
        .expect("should be an array");
    assert_eq!(
        profiles[0].as_str().expect("should have a string value"),
        "http://example.org/fhir/StructureDefinition/MyPatient"
    );
}

// ── Tests for write_supplement_ndjson error handling ────────────────────

#[test]
fn write_supplement_handles_generation_error_gracefully() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    // Use a type that has no profile and no hardcoded generator
    // The generic generator should handle it
    let creation_order = vec!["Patient".to_string()];
    let bulk_counts = HashMap::new();

    let supplement_ids = write_supplement_ndjson(
        &creation_order,
        &bulk_counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    assert!(supplement_ids.contains_key("Patient"));
}

// ── Tests for write_supplement_ndjson with combined.ndjson ──────────────

#[test]
fn write_supplement_appends_to_existing_combined() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    // Create a combined.ndjson with some content first
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("should create dir");
    let combined_path = data_dir.join("combined.ndjson");
    let existing_resource = serde_json::json!({ "resourceType": "Organization", "id": "org-1" });
    std::fs::write(
        &combined_path,
        serde_json::to_string(&existing_resource).unwrap() + "\n",
    )
    .expect("should write");

    let creation_order = vec!["Patient".to_string()];
    let bulk_counts = HashMap::new();

    write_supplement_ndjson(
        &creation_order,
        &bulk_counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // combined.ndjson should have 2 lines (existing + supplement)
    let contents = std::fs::read_to_string(&combined_path).expect("should read file");
    let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2, "combined.ndjson should have 2 lines");
}

// ── Tests for generate_update_ndjson with various resource shapes ───────

#[test]
fn update_ndjson_with_multiple_types() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    let mut counts = HashMap::new();
    counts.insert("Organization".to_string(), 2);
    counts.insert("Practitioner".to_string(), 2);
    counts.insert("Location".to_string(), 2);

    let ids = generate_bulk_data(
        &counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    generate_update_ndjson(&ids, dir.path()).expect("should succeed");

    let update_path = dir.path().join("data/update.ndjson");
    let contents = std::fs::read_to_string(&update_path).expect("should read file");
    let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
    let total: usize = counts.values().sum::<u64>() as usize;
    assert_eq!(
        lines.len(),
        total,
        "update.ndjson should have {} lines",
        total
    );

    // Verify each line has valid JSON with resourceType and id
    for line in &lines {
        let resource: serde_json::Value =
            serde_json::from_str(line).expect("should parse valid JSON");
        assert!(resource["resourceType"].as_str().is_some());
        assert!(resource["id"].as_str().is_some());
    }
}

// ── Tests for PractitionerRole overlay with all reference types ─────────

#[test]
fn overlay_practitionerrole_with_all_references() {
    let mut pr =
        serde_json::json!({ "resourceType": "PractitionerRole", "id": "practitionerrole-1" });
    let org_ids = vec!["organization-1".to_string()];
    let prac_ids = vec!["practitioner-1".to_string()];
    let loc_ids = vec!["location-1".to_string()];
    let hs_ids = vec!["healthcareservice-1".to_string()];
    let endpoint_ids = vec!["endpoint-1".to_string()];
    let mut rng = rand::rng();

    overlay::overlay_cross_references(
        &mut pr,
        "PractitionerRole",
        "practitionerrole-1",
        &org_ids,
        &prac_ids,
        &loc_ids,
        &hs_ids,
        &[],
        &endpoint_ids,
        &mut rng,
    );

    assert!(pr["practitioner"]["reference"].as_str().is_some());
    assert!(pr["organization"]["reference"].as_str().is_some());
    assert!(pr["location"][0]["reference"].as_str().is_some());
    assert!(pr["healthcareService"][0]["reference"].as_str().is_some());
    assert!(pr["endpoint"][0]["reference"].as_str().is_some());
}

#[test]
fn overlay_practitionerrole_round_robin_healthcareservice() {
    let mut pr1 =
        serde_json::json!({ "resourceType": "PractitionerRole", "id": "practitionerrole-1" });
    let mut pr2 =
        serde_json::json!({ "resourceType": "PractitionerRole", "id": "practitionerrole-2" });
    let org_ids = vec!["organization-1".to_string()];
    let prac_ids = vec!["practitioner-1".to_string()];
    let hs_ids = vec![
        "healthcareservice-1".to_string(),
        "healthcareservice-2".to_string(),
    ];
    let mut rng = rand::rng();

    overlay::overlay_cross_references(
        &mut pr1,
        "PractitionerRole",
        "practitionerrole-1",
        &org_ids,
        &prac_ids,
        &[],
        &hs_ids,
        &[],
        &[],
        &mut rng,
    );
    overlay::overlay_cross_references(
        &mut pr2,
        "PractitionerRole",
        "practitionerrole-2",
        &org_ids,
        &prac_ids,
        &[],
        &hs_ids,
        &[],
        &[],
        &mut rng,
    );

    // Round-robin: practitionerrole-1 → hs_ids[1] (1 % 2 = 1), practitionerrole-2 → hs_ids[0] (2 % 2 = 0)
    assert_eq!(
        pr1["healthcareService"][0]["reference"].as_str(),
        Some("HealthcareService/healthcareservice-2")
    );
    assert_eq!(
        pr2["healthcareService"][0]["reference"].as_str(),
        Some("HealthcareService/healthcareservice-1")
    );
}

#[test]
fn overlay_practitionerrole_deterministic_practitioner_ref() {
    // practitionerrole-1 should always reference practitioner-1 when it exists
    let mut pr =
        serde_json::json!({ "resourceType": "PractitionerRole", "id": "practitionerrole-1" });
    let prac_ids = vec!["practitioner-1".to_string(), "practitioner-2".to_string()];
    let mut rng = rand::rng();

    overlay::overlay_cross_references(
        &mut pr,
        "PractitionerRole",
        "practitionerrole-1",
        &[],
        &prac_ids,
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );

    assert_eq!(
        pr["practitioner"]["reference"].as_str(),
        Some("Practitioner/practitioner-1")
    );
}

// ── Tests for Organization overlay with random partOf ────────────────────

#[test]
fn overlay_organization_non_anchor_with_earlier_orgs_may_get_partof() {
    // Test that organization-3 (which is in org_ids at index 2) can get partOf
    // pointing to an earlier organization
    let org_ids = vec![
        "organization-1".to_string(),
        "organization-2".to_string(),
        "organization-3".to_string(),
    ];
    let mut rng = rand::rng();
    let mut org = serde_json::json!({ "resourceType": "Organization", "id": "organization-3" });
    overlay::overlay_cross_references(
        &mut org,
        "Organization",
        "organization-3",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // organization-3 is at index 2, so it could get partOf (1% chance)
    // or not — either is valid. Just verify it doesn't panic.
    let _ = org;
}

// ── Tests for ensure_target_path_extension ──────────────────────────────

#[test]
fn ensure_target_path_extension_skips_when_extension_exists() {
    let mut target_obj = serde_json::Map::new();
    target_obj.insert(
        "extension".to_string(),
        serde_json::json!([{
            "url": "http://example.org/custom",
            "valueString": "custom"
        }]),
    );
    // This function is private, but we can test it via the Provenance overlay
    // which calls it internally
    let mut provenance = serde_json::json!({
        "resourceType": "Provenance",
        "id": "provenance-1",
        "target": [{
            "reference": "Organization/placeholder",
            "extension": [{
                "url": "http://example.org/custom",
                "valueString": "custom"
            }]
        }]
    });
    let org_ids = vec!["organization-1".to_string()];
    let mut rng = rand::rng();
    overlay::overlay_cross_references(
        &mut provenance,
        "Provenance",
        "provenance-1",
        &org_ids,
        &[],
        &[],
        &[],
        &[],
        &[],
        &mut rng,
    );
    // The existing extension should be preserved
    assert_eq!(
        provenance["target"][0]["extension"][0]["url"].as_str(),
        Some("http://example.org/custom")
    );
}

// ── Tests for write_supplement_ndjson with NON_RESOURCE_TYPES ───────────

#[test]
fn write_supplement_skips_extension_type() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    let creation_order = vec!["Extension".to_string()];
    let bulk_counts = HashMap::new();

    let supplement_ids = write_supplement_ndjson(
        &creation_order,
        &bulk_counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    assert!(
        !supplement_ids.contains_key("Extension"),
        "Extension should be skipped as a non-resource type"
    );
}

// ── Tests for generate_supplement_resource with all resource types ──────

#[test]
fn supplement_resource_generates_all_types() {
    for resource_type in &[
        "Organization",
        "Practitioner",
        "PractitionerRole",
        "Location",
        "HealthcareService",
        "Endpoint",
    ] {
        let resource = generate_supplement_resource(
            resource_type,
            &HashMap::new(),
            &[],
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap_or_else(|_| panic!("should generate {} supplement", resource_type));

        assert_eq!(resource["resourceType"], *resource_type);
        assert_eq!(
            resource["id"],
            format!("{}-1", resource_type.to_lowercase())
        );
    }
}

// ── Tests for write_supplement_ndjson with all types in creation_order ──

#[test]
fn write_supplement_skips_types_with_bulk_count() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    let mut bulk_counts = HashMap::new();
    bulk_counts.insert("Organization".to_string(), 5);
    bulk_counts.insert("Practitioner".to_string(), 3);

    let creation_order = vec![
        "Organization".to_string(),
        "Practitioner".to_string(),
        "Patient".to_string(),
    ];

    let supplement_ids = write_supplement_ndjson(
        &creation_order,
        &bulk_counts,
        &HashMap::new(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        dir.path(),
    )
    .expect("should succeed");

    // Organization and Practitioner have bulk counts, so they should not be in supplement
    assert!(!supplement_ids.contains_key("Organization"));
    assert!(!supplement_ids.contains_key("Practitioner"));
    // Patient has no bulk count, so it should be in supplement
    assert!(supplement_ids.contains_key("Patient"));
}
