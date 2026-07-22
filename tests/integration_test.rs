use assert_cmd::Command;
use std::io::Write;

/// Create a minimal but realistic FHIR IG package (.tgz) for testing.
/// Contains:
/// - A CapabilityStatement with Patient and Observation resources
/// - A Patient profile (US Core style)
/// - An Observation profile referencing Patient
/// - A SearchParameter for Patient name
fn create_test_ig_package() -> Vec<u8> {
    let cs_json = r#"{
        "resourceType": "CapabilityStatement",
        "url": "http://example.org/CapabilityStatement/TestIG",
        "name": "TestIG",
        "status": "active",
        "rest": [{
            "mode": "server",
            "resource": [{
                "type": "Patient",
                "profile": "http://hl7.org/fhir/StructureDefinition/Patient",
                "supportedProfile": ["http://example.org/StructureDefinition/TestPatient"],
                "interaction": [
                    {"code": "read"},
                    {"code": "search-type"},
                    {"code": "create"},
                    {"code": "update"},
                    {"code": "delete"}
                ],
                "searchParam": [
                    {"name": "name", "type": "string"},
                    {"name": "birthdate", "type": "date"}
                ]
            }, {
                "type": "Observation",
                "profile": "http://hl7.org/fhir/StructureDefinition/Observation",
                "supportedProfile": ["http://example.org/StructureDefinition/TestObservation"],
                "interaction": [
                    {"code": "read"},
                    {"code": "search-type"},
                    {"code": "create"}
                ],
                "searchParam": [
                    {"name": "category", "type": "token"},
                    {"name": "code", "type": "token"}
                ]
            }],
            "interaction": []
        }]
    }"#;

    let patient_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/TestPatient",
        "name": "TestPatient",
        "type": "Patient",
        "kind": "resource",
        "derivation": "constraint",
        "snapshot": {
            "element": [{
                "id": "Patient",
                "path": "Patient",
                "min": 0,
                "max": "*"
            }, {
                "id": "Patient.identifier",
                "path": "Patient.identifier",
                "min": 1,
                "max": "*",
                "type": [{"code": "Identifier"}],
                "mustSupport": true
            }, {
                "id": "Patient.name",
                "path": "Patient.name",
                "min": 1,
                "max": "*",
                "type": [{"code": "HumanName"}],
                "mustSupport": true
            }, {
                "id": "Patient.gender",
                "path": "Patient.gender",
                "min": 0,
                "max": "1",
                "type": [{"code": "code"}]
            }, {
                "id": "Patient.birthDate",
                "path": "Patient.birthDate",
                "min": 0,
                "max": "1",
                "type": [{"code": "date"}]
            }]
        }
    }"#;

    let observation_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/TestObservation",
        "name": "TestObservation",
        "type": "Observation",
        "kind": "resource",
        "derivation": "constraint",
        "snapshot": {
            "element": [{
                "id": "Observation",
                "path": "Observation",
                "min": 0,
                "max": "*"
            }, {
                "id": "Observation.status",
                "path": "Observation.status",
                "min": 1,
                "max": "1",
                "type": [{"code": "code"}],
                "fixedCode": "final"
            }, {
                "id": "Observation.subject",
                "path": "Observation.subject",
                "min": 1,
                "max": "1",
                "type": [{
                    "code": "Reference",
                    "targetProfile": ["http://hl7.org/fhir/StructureDefinition/Patient"]
                }],
                "mustSupport": true
            }, {
                "id": "Observation.code",
                "path": "Observation.code",
                "min": 1,
                "max": "1",
                "type": [{"code": "CodeableConcept"}]
            }, {
                "id": "Observation.valueString",
                "path": "Observation.valueString",
                "min": 0,
                "max": "1",
                "type": [{"code": "string"}]
            }]
        }
    }"#;

    let sp_json = r#"{
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/patient-name",
        "name": "name",
        "code": "name",
        "base": ["Patient"],
        "type": "string",
        "expression": "Patient.name"
    }"#;

    let mut tar_data = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_data);

        let files = [
            ("package/CapabilityStatement-test.json", cs_json),
            ("package/StructureDefinition-TestPatient.json", patient_sd_json),
            ("package/StructureDefinition-TestObservation.json", observation_sd_json),
            ("package/SearchParameter-patient-name.json", sp_json),
        ];

        for (path, content) in &files {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).unwrap();
            header.set_size(content.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, *path, content.as_bytes()).unwrap();
        }

        tar.finish().unwrap();
    }

    let mut gz_data = Vec::new();
    {
        let mut gz = flate2::write::GzEncoder::new(&mut gz_data, flate2::Compression::default());
        gz.write_all(&tar_data).unwrap();
        gz.finish().unwrap();
    }

    gz_data
}

#[test]
fn generate_from_minimal_package() {
    let tgz_data = create_test_ig_package();
    let temp_dir = tempfile::tempdir().unwrap();
    let tgz_path = temp_dir.path().join("test_ig_package.tgz");
    std::fs::write(&tgz_path, &tgz_data).unwrap();

    let output_dir = temp_dir.path().join("output");

    let mut cmd = Command::cargo_bin("fhir-ig-testgen").unwrap();
    cmd.args([
        "generate",
        "--package",
        tgz_path.to_str().unwrap(),
        "--output",
        output_dir.to_str().unwrap(),
    ])
    .assert()
    .success();

    // Verify output directory contains expected files
    assert!(output_dir.join("test_plan.json").exists(), "test_plan.json should exist");
    assert!(output_dir.join("resources").is_dir(), "resources directory should exist");

    // Verify the test plan is valid JSON
    let plan_json = std::fs::read_to_string(output_dir.join("test_plan.json")).unwrap();
    let plan: serde_json::Value = serde_json::from_str(&plan_json).unwrap();
    assert_eq!(plan["name"], "TestIG");
    assert!(plan["test_groups"].is_array());
    assert!(plan["test_groups"].as_array().unwrap().len() >= 2, "Should have test groups for Patient and Observation");
}

#[test]
fn generated_resources_satisfy_profiles() {
    let tgz_data = create_test_ig_package();
    let temp_dir = tempfile::tempdir().unwrap();
    let tgz_path = temp_dir.path().join("test_ig_package.tgz");
    std::fs::write(&tgz_path, &tgz_data).unwrap();

    let output_dir = temp_dir.path().join("output");

    let mut cmd = Command::cargo_bin("fhir-ig-testgen").unwrap();
    cmd.args([
        "generate",
        "--package",
        tgz_path.to_str().unwrap(),
        "--output",
        output_dir.to_str().unwrap(),
    ])
    .assert()
    .success();

    // Check Patient resource
    let patient_json = std::fs::read_to_string(output_dir.join("resources/patient.json")).unwrap();
    let patient: serde_json::Value = serde_json::from_str(&patient_json).unwrap();
    assert_eq!(patient["resourceType"], "Patient");
    assert!(patient.get("name").is_some(), "Patient should have name (required)");
    assert!(patient.get("identifier").is_some(), "Patient should have identifier (required)");

    // Check Observation resource
    let obs_json = std::fs::read_to_string(output_dir.join("resources/observation.json")).unwrap();
    let obs: serde_json::Value = serde_json::from_str(&obs_json).unwrap();
    assert_eq!(obs["resourceType"], "Observation");
    assert_eq!(obs["status"], "final", "Observation.status should have fixed code 'final'");
    assert!(obs.get("subject").is_some(), "Observation should have subject (required)");
    assert!(obs.get("code").is_some(), "Observation should have code (required)");
}

#[test]
fn dependency_order_is_correct() {
    // Observation depends on Patient, so Patient must come before Observation
    let tgz_data = create_test_ig_package();
    let temp_dir = tempfile::tempdir().unwrap();
    let tgz_path = temp_dir.path().join("test_ig_package.tgz");
    std::fs::write(&tgz_path, &tgz_data).unwrap();

    let output_dir = temp_dir.path().join("output");

    let mut cmd = Command::cargo_bin("fhir-ig-testgen").unwrap();
    cmd.args([
        "generate",
        "--package",
        tgz_path.to_str().unwrap(),
        "--output",
        output_dir.to_str().unwrap(),
    ])
    .assert()
    .success();

    let plan_json = std::fs::read_to_string(output_dir.join("test_plan.json")).unwrap();
    let plan: serde_json::Value = serde_json::from_str(&plan_json).unwrap();

    let creation_order = plan["creation_order"].as_array().unwrap();
    let patient_idx = creation_order.iter().position(|v| v.as_str() == Some("Patient")).unwrap();
    let obs_idx = creation_order.iter().position(|v| v.as_str() == Some("Observation")).unwrap();
    assert!(patient_idx < obs_idx, "Patient should come before Observation in creation order");
}

#[test]
fn validate_command_with_valid_resource() {
    let tgz_data = create_test_ig_package();
    let temp_dir = tempfile::tempdir().unwrap();
    let tgz_path = temp_dir.path().join("test_ig_package.tgz");
    std::fs::write(&tgz_path, &tgz_data).unwrap();

    // Create a valid Patient resource
    let patient = serde_json::json!({
        "resourceType": "Patient",
        "name": [{"family": "TestFamily", "given": ["TestGiven"]}],
        "identifier": [{"system": "http://example.org", "value": "test-123"}]
    });
    let patient_path = temp_dir.path().join("patient.json");
    std::fs::write(&patient_path, serde_json::to_string_pretty(&patient).unwrap()).unwrap();

    let mut cmd = Command::cargo_bin("fhir-ig-testgen").unwrap();
    cmd.args([
        "validate",
        "--package",
        tgz_path.to_str().unwrap(),
        "--resource",
        patient_path.to_str().unwrap(),
    ])
    .assert()
    .success();
}

#[test]
fn validate_command_with_invalid_resource() {
    let tgz_data = create_test_ig_package();
    let temp_dir = tempfile::tempdir().unwrap();
    let tgz_path = temp_dir.path().join("test_ig_package.tgz");
    std::fs::write(&tgz_path, &tgz_data).unwrap();

    // Create an invalid Patient (missing required 'name')
    let patient = serde_json::json!({
        "resourceType": "Patient"
    });
    let patient_path = temp_dir.path().join("bad_patient.json");
    std::fs::write(&patient_path, serde_json::to_string_pretty(&patient).unwrap()).unwrap();

    let mut cmd = Command::cargo_bin("fhir-ig-testgen").unwrap();
    let output = cmd
        .args([
            "validate",
            "--package",
            tgz_path.to_str().unwrap(),
            "--resource",
            patient_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("failed") || stdout.contains("Missing required"), "Should report validation failures");
}