use super::*;
use std::collections::HashMap;

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
