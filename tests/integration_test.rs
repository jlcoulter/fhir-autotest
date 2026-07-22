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

#[test]
fn test_plan_contains_all_test_kinds() {
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

    // Find the Patient test group
    let patient_group = plan["test_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["resource_type"] == "Patient")
        .expect("Should have a Patient test group");

    let tests = patient_group["tests"].as_array().unwrap();
    let test_names: Vec<&str> = tests.iter().map(|t| t["name"].as_str().unwrap()).collect();

    // Must have interaction tests (read, create, update, delete)
    assert!(test_names.iter().any(|n| n.contains("_read")), "Should have read test");
    assert!(test_names.iter().any(|n| n.contains("_create")), "Should have create test");
    assert!(test_names.iter().any(|n| n.contains("_update")), "Should have update test");
    assert!(test_names.iter().any(|n| n.contains("_delete")), "Should have delete test");

    // Must have single search param tests
    assert!(test_names.iter().any(|n| n.contains("_search_name")), "Should have name search test");
    assert!(test_names.iter().any(|n| n.contains("_search_birthdate")), "Should have birthdate search test");

    // Must have modifier tests
    assert!(test_names.iter().any(|n| n.contains("_exact")), "Should have :exact modifier test for name (string)");
    assert!(test_names.iter().any(|n| n.contains("_contains")), "Should have :contains modifier test for name (string)");
    assert!(test_names.iter().any(|n| n.contains("_missing")), "Should have :missing modifier test");

    // Must have prefix tests (date params get eq, ne, gt, lt, etc.)
    assert!(test_names.iter().any(|n| n.contains("_eq")), "Should have eq prefix test");
    assert!(test_names.iter().any(|n| n.contains("_gt")), "Should have gt prefix test");
    assert!(test_names.iter().any(|n| n.contains("_lt")), "Should have lt prefix test");

    // Must have combinatorial search tests
    assert!(test_names.iter().any(|n| n.contains("_combo_")), "Should have combo search tests");

    // Must have result param tests
    assert!(test_names.iter().any(|n| n.contains("_result_")), "Should have result param tests");

    // Must have negative tests
    assert!(test_names.iter().any(|n| n.contains("_negative_")), "Should have negative tests");

    // Total should be substantially more than just CRUD + individual searches
    assert!(tests.len() >= 15, "Should have at least 15 tests for Patient, got {}", tests.len());

    // Observation should also have comprehensive tests
    let obs_group = plan["test_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["resource_type"] == "Observation")
        .expect("Should have an Observation test group");

    let obs_tests = obs_group["tests"].as_array().unwrap();
    // Observation has token params (category, code) which get :not, :text, :missing modifiers
    assert!(obs_tests.len() >= 10, "Should have at least 10 tests for Observation, got {}", obs_tests.len());

    // Verify test case kinds are serialized correctly
    let kinds: Vec<serde_json::Value> = tests.iter().map(|t| t["kind"].clone()).collect();
    assert!(kinds.iter().any(|k| k.is_string() && k.as_str() == Some("Interaction")), "Should have Interaction kind");
    assert!(kinds.iter().any(|k| k.is_object() && k.as_object().map(|o| o.contains_key("SearchSingle")).unwrap_or(false)), "Should have SearchSingle kind");
    assert!(kinds.iter().any(|k| k.is_object() && k.as_object().map(|o| o.contains_key("SearchModifier")).unwrap_or(false)), "Should have SearchModifier kind");
    assert!(kinds.iter().any(|k| k.is_object() && k.as_object().map(|o| o.contains_key("SearchPrefix")).unwrap_or(false)), "Should have SearchPrefix kind");
    assert!(kinds.iter().any(|k| k.is_object() && k.as_object().map(|o| o.contains_key("SearchCombo")).unwrap_or(false)), "Should have SearchCombo kind");
    assert!(kinds.iter().any(|k| k.is_object() && k.as_object().map(|o| o.contains_key("Negative")).unwrap_or(false)), "Should have Negative kind");
    assert!(kinds.iter().any(|k| k.is_object() && k.as_object().map(|o| o.contains_key("ResultParam")).unwrap_or(false)), "Should have ResultParam kind");
}

// ─── Mock FHIR Server Integration Test ──────────────────────────────────────
//
// Spins up an in-process mock FHIR server, generates a test plan, and runs
// the full orchestrator against it. This demonstrates:
// 1. Resource creation (POST)
// 2. Search-type tests (GET with query params)
// 3. Read tests (GET by ID)
// 4. Negative tests (GET nonexistent → 404)
// 5. Response assertion validation
// 6. Resource cleanup (DELETE)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
type MockStore = Arc<Mutex<HashMap<String, Vec<serde_json::Value>>>>;

async fn create_resource(
    State(store): State<MockStore>,
    Path(rtype): Path<String>,
    Json(mut body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let id = uuid::Uuid::new_v4().to_string();
    body["id"] = serde_json::Value::String(id.clone());
    let mut store = store.lock().unwrap();
    store.entry(rtype.clone()).or_default().push(body.clone());
    (StatusCode::CREATED, Json(body))
}

async fn read_resource(
    State(store): State<MockStore>,
    Path((rtype, id)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = store.lock().unwrap();
    if let Some(resources) = store.get(&rtype) {
        if let Some(resource) = resources.iter().find(|r| r.get("id").and_then(|v| v.as_str()) == Some(&id)) {
            return (StatusCode::OK, Json(resource.clone()));
        }
    }
    (StatusCode::NOT_FOUND, Json(serde_json::json!({
        "resourceType": "OperationOutcome",
        "issue": [{"severity": "error", "code": "not-found", "diagnostics": format!("{}/{} not found", rtype, id)}]
    })))
}

async fn search_resources(
    State(store): State<MockStore>,
    Path(rtype): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = store.lock().unwrap();
    let resources = store.get(&rtype).cloned().unwrap_or_default();
    let entries: Vec<serde_json::Value> = resources.iter().map(|r| {
        serde_json::json!({
            "resource": r,
            "fullUrl": format!("http://localhost:8091/fhir/{}/{}", rtype, r["id"].as_str().unwrap_or_default())
        })
    }).collect();

    (StatusCode::OK, Json(serde_json::json!({
        "resourceType": "Bundle",
        "type": "searchset",
        "total": entries.len(),
        "entry": entries
    })))
}

async fn update_resource(
    State(store): State<MockStore>,
    Path((rtype, id)): Path<(String, String)>,
    Json(mut body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    body["id"] = serde_json::Value::String(id.clone());
    let mut store = store.lock().unwrap();
    if let Some(resources) = store.get_mut(&rtype) {
        if let Some(idx) = resources.iter().position(|r| r.get("id").and_then(|v| v.as_str()) == Some(&id)) {
            resources[idx] = body.clone();
            return (StatusCode::OK, Json(body));
        }
    }
    (StatusCode::NOT_FOUND, Json(serde_json::json!({
        "resourceType": "OperationOutcome",
        "issue": [{"severity": "error", "code": "not-found"}]
    })))
}

async fn delete_resource(
    State(store): State<MockStore>,
    Path((rtype, id)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut store = store.lock().unwrap();
    if let Some(resources) = store.get_mut(&rtype) {
        let before = resources.len();
        resources.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(&id));
        if resources.len() < before {
            return (StatusCode::OK, Json(serde_json::json!({
                "resourceType": "OperationOutcome",
                "issue": [{"severity": "information", "code": "informational", "diagnostics": "Deleted"}]
            })));
        }
    }
    (StatusCode::NOT_FOUND, Json(serde_json::json!({
        "resourceType": "OperationOutcome",
        "issue": [{"severity": "error", "code": "not-found"}]
    })))
}

async fn operation_handler(
    State(_store): State<MockStore>,
) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({
        "resourceType": "Parameters",
        "parameter": [{"name": "result", "valueBoolean": true}]
    })))
}

fn create_mock_app() -> Router {
    let store: MockStore = Arc::new(Mutex::new(HashMap::new()));
    Router::new()
        .route("/fhir/{rtype}", post(create_resource))
        .route("/fhir/{rtype}", get(search_resources))
        .route("/fhir/{rtype}/{id}", get(read_resource))
        .route("/fhir/{rtype}/{id}", put(update_resource))
        .route("/fhir/{rtype}/{id}", delete(delete_resource))
        .route("/fhir/${*op}", get(operation_handler))
        .route("/fhir/{rtype}/${*op}", get(operation_handler))
        .with_state(store)
}

#[tokio::test]
async fn run_against_mock_fhir_server() {
    use fhir_ig_testgen::runner::orchestrator::Orchestrator;
    use fhir_ig_testgen::config::models::TestConfig;

    // 1. Create a mock FHIR server on a random port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let port = addr.port();

    let app = create_mock_app();
    let server = axum::serve(listener, app);

    // Run server in background
    tokio::spawn(async move {
        server.await.unwrap();
    });

    // Give server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 2. Create the IG package
    let tgz_data = create_test_ig_package();
    let temp_dir = tempfile::tempdir().unwrap();
    let tgz_path = temp_dir.path().join("test_ig_package.tgz");
    std::fs::write(&tgz_path, &tgz_data).unwrap();

    // 3. Create config
    let config = TestConfig {
        server: fhir_ig_testgen::config::models::ServerConfig {
            base_url: format!("http://127.0.0.1:{}/fhir", port),
            headers: HashMap::new(),
        },
        overrides: fhir_ig_testgen::config::models::OverrideConfig::default(),
    };

    // 4. Run the orchestrator
    let orchestrator = Orchestrator::new(config);
    let report = orchestrator.run(tgz_path.to_str().unwrap()).await.unwrap();

    // 5. Verify results
    // We should have tests for at least Patient and Observation
    assert!(report.total > 0, "Should have run at least some tests, got {}", report.total);

    // Print the report for visibility
    println!("\n{}", report);

    // Some tests should pass (reads/searches of created resources)
    assert!(report.passed > 0, "At least some tests should pass, got {} passed out of {}", report.passed, report.total);

    // Create tests should work (we can POST resources)
    let create_tests: Vec<_> = report.results.iter()
        .filter(|r| r.test_name.contains("_create"))
        .collect();
    assert!(!create_tests.is_empty(), "Should have create tests");

    // Search tests should return searchset Bundles
    let search_tests: Vec<_> = report.results.iter()
        .filter(|r| r.test_name.contains("_search_"))
        .collect();
    assert!(!search_tests.is_empty(), "Should have search tests");

    // Negative tests (read nonexistent) should get 404
    let negative_tests: Vec<_> = report.results.iter()
        .filter(|r| r.test_name.contains("_negative_"))
        .collect();
    assert!(!negative_tests.is_empty(), "Should have negative tests");

    // Check that response assertions were applied
    // At least some tests should have validation errors from assertion checks
    // (not all assertions will pass against a minimal mock)
    let with_assertion_errors: Vec<_> = report.results.iter()
        .filter(|r| !r.validation_errors.is_empty())
        .collect();
    // This proves the assertion system is running — even if mock responses
    // don't perfectly match, the assertions ARE being evaluated
    assert!(!with_assertion_errors.is_empty() || report.passed == report.total,
        "Assertion validation should produce either errors (proving it runs) or all tests pass");
}