use std::path::Path;
use std::process::Command;

/// Helper: create a minimal IG package (.tgz) using the test helper.
fn create_test_ig_package(path: &Path) {
    let tgz_data = fhir_autotest::test_helpers::create_test_ig_package();
    std::fs::write(path, &tgz_data).unwrap();
}

/// Helper: create a config.toml with flat fuzz fields (no [fuzz] section header).
/// FuzzConfig is deserialized from flat TOML, while TestConfig::load expects [server].
/// Fuzz fields MUST come before [server] so they're at the root level for FuzzConfig.
fn create_flat_config(path: &Path, extra: &str) {
    let content = format!(
        r#"
{extra}

[server]
base_url = "http://localhost:8080/fhir"
"#,
        extra = extra
    );
    std::fs::write(path, content).unwrap();
}

#[test]
fn test_main_dry_run_mock() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    // Create config file with both [server] and flat fuzz fields
    let config_path = temp_path.join("config.toml");
    create_flat_config(
        &config_path,
        r#"
iterations = 2
mutations = "boundary"
seed = 42
concurrency = 1
delay_ms = 0
"#,
    );

    // Create test IG package
    let tgz_path = temp_path.join("test_ig.tgz");
    create_test_ig_package(&tgz_path);

    // Build the binary first
    let status = Command::new("cargo")
        .args(["build", "-p", "fhir-autotest-fuzz"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to build binary");
    assert!(status.success(), "cargo build should succeed");

    // Find the binary path
    let binary_path = if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/fhir-autotest-fuzz")
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/release/fhir-autotest-fuzz")
    };

    // Run with --dry-run --mock
    let output = Command::new(binary_path)
        .args([
            "--package",
            tgz_path.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--dry-run",
            "--mock",
            "--output",
            temp_path.join("fuzz-output").to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run fhir-autotest-fuzz");

    // Check that it ran successfully
    assert!(
        output.status.success(),
        "Binary should exit successfully. stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should have printed mock server info
    assert!(
        stdout.contains("Mock FHIR server running at"),
        "Should print mock server URL. Output: {}",
        stdout
    );

    // Should have parsed the package
    assert!(
        stdout.contains("Loaded"),
        "Should print package loading info. Output: {}",
        stdout
    );

    // Should have started fuzz run
    assert!(
        stdout.contains("Starting fuzz run"),
        "Should print fuzz run start. Output: {}",
        stdout
    );

    // Should have printed results
    assert!(
        stdout.contains("Fuzz Results"),
        "Should print fuzz results. Output: {}",
        stdout
    );

    // Should have 0 anomalies (dry run)
    assert!(
        stdout.contains("Anomalies found: 0"),
        "Should have 0 anomalies in dry run. Output: {}",
        stdout
    );
}

#[test]
fn test_main_dry_run_mock_with_all_mutations() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    // Valid config with all mutations
    let config_path = temp_path.join("config.toml");
    create_flat_config(
        &config_path,
        r#"
iterations = 1
mutations = "all"
seed = 42
concurrency = 1
"#,
    );

    let tgz_path = temp_path.join("test_ig.tgz");
    create_test_ig_package(&tgz_path);

    // Build
    let status = Command::new("cargo")
        .args(["build", "-p", "fhir-autotest-fuzz"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to build binary");
    assert!(status.success());

    let binary_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/fhir-autotest-fuzz");

    let output = Command::new(binary_path)
        .args([
            "--package",
            tgz_path.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--dry-run",
            "--mock",
            "--mock-port",
            "0",
            "--output",
            temp_path.join("fuzz-output").to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run fhir-autotest-fuzz");

    assert!(
        output.status.success(),
        "Binary should exit successfully. stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Fuzz Results"),
        "Should print fuzz results. Output: {}",
        stdout
    );
}

#[test]
fn test_main_without_package_fails() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    // Valid config without package
    let config_path = temp_path.join("config.toml");
    create_flat_config(
        &config_path,
        r#"
iterations = 1
"#,
    );

    // Build
    let status = Command::new("cargo")
        .args(["build", "-p", "fhir-autotest-fuzz"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to build binary");
    assert!(status.success());

    let binary_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/fhir-autotest-fuzz");

    let output = Command::new(binary_path)
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "--dry-run",
            "--mock",
        ])
        .output()
        .expect("Failed to run fhir-autotest-fuzz");

    // Should fail with error about missing package
    assert!(
        !output.status.success(),
        "Binary should fail without package. stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No IG package specified"),
        "Should mention missing package. stderr: {}",
        stderr
    );
}

#[test]
fn test_main_without_target_and_no_mock_fails() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    let tgz_path = temp_path.join("test_ig.tgz");
    create_test_ig_package(&tgz_path);

    // Config without [server] section — TestConfig::load will fail,
    // so config will be None, and the code will bail with "No target server"
    let config_content = r#"
iterations = 1
"#;
    std::fs::write(temp_path.join("config.toml"), config_content).unwrap();

    // Build
    let status = Command::new("cargo")
        .args(["build", "-p", "fhir-autotest-fuzz"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to build binary");
    assert!(status.success());

    let binary_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/fhir-autotest-fuzz");

    let output = Command::new(binary_path)
        .args([
            "--package",
            tgz_path.to_str().unwrap(),
            "--config",
            temp_path.join("config.toml").to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .expect("Failed to run fhir-autotest-fuzz");

    // Should fail with error about missing target
    assert!(
        !output.status.success(),
        "Binary should fail without target. stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No target server"),
        "Should mention missing target. stderr: {}",
        stderr
    );
}

#[test]
fn test_main_with_unknown_mutation_fails() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    let tgz_path = temp_path.join("test_ig.tgz");
    create_test_ig_package(&tgz_path);

    // Valid config with unknown mutation
    let config_path = temp_path.join("config.toml");
    create_flat_config(
        &config_path,
        r#"
iterations = 1
mutations = "unknown_mutator"
"#,
    );

    // Build
    let status = Command::new("cargo")
        .args(["build", "-p", "fhir-autotest-fuzz"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("Failed to build binary");
    assert!(status.success());

    let binary_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/fhir-autotest-fuzz");

    let output = Command::new(binary_path)
        .args([
            "--package",
            tgz_path.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--dry-run",
            "--mock",
        ])
        .output()
        .expect("Failed to run fhir-autotest-fuzz");

    // Should fail with error about unknown mutation
    assert!(
        !output.status.success(),
        "Binary should fail with unknown mutation. stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unknown mutation category"),
        "Should mention unknown mutation. stderr: {}",
        stderr
    );
}
