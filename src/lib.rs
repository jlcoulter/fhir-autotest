pub mod model;
pub mod parse;
pub mod generate;
pub mod runner;
pub mod config;

// Re-export key types
pub use model::*;
pub use parse::*;
pub use generate::*;
pub use config::models::*;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// Generate a test plan and resources from an IG package.
/// Writes output to the specified directory.
pub fn run_generate(package_path: &str, config_path: Option<&str>, output_dir: &str) -> Result<()> {
    let pkg = parse_package(package_path)?;

    // Load config if provided
    let config = match config_path {
        Some(path) => TestConfig::load(path)?,
        None => TestConfig {
            server: crate::config::models::ServerConfig {
                base_url: "http://localhost:8080/fhir".to_string(),
                headers: HashMap::new(),
            },
            overrides: crate::config::models::OverrideConfig::default(),
        },
    };

    let cs = pkg
        .capability_statements
        .first()
        .context("No CapabilityStatement found in IG package")?;

    // Resolve dependencies
    let auto_deps = extract_dependencies(&pkg.structure_definitions);
    let auto_order = resolve_creation_order(&auto_deps)?;
    let creation_order = merge_creation_order(&auto_order, &config.overrides.creation_order);

    // Generate or load resources
    let fixtures = config.load_fixtures()?;
    let mut resources: HashMap<String, serde_json::Value> = HashMap::new();
    for resource_type in &creation_order {
        if let Some(fixture) = fixtures.get(resource_type) {
            resources.insert(resource_type.clone(), fixture.clone());
        } else {
            let profile = pkg
                .structure_definitions
                .iter()
                .find(|sd| sd.base_type == *resource_type);
            if let Some(profile) = profile {
                let generated = generate_resource(profile)?;
                resources.insert(resource_type.clone(), generated);
            }
        }
    }

    // Generate test plan
    let mut plan = generate_test_plan(
        cs,
        &pkg.structure_definitions,
        &pkg.search_parameters,
        None,
    );
    plan.creation_order = creation_order;

    // Write output
    let output_path = Path::new(output_dir);
    std::fs::create_dir_all(output_path)?;
    std::fs::create_dir_all(output_path.join("resources"))?;

    // Write test plan
    let plan_json = serde_json::to_string_pretty(&plan)?;
    std::fs::write(output_path.join("test_plan.json"), &plan_json)?;

    // Write resources
    for (resource_type, resource) in &resources {
        let resource_json = serde_json::to_string_pretty(resource)?;
        std::fs::write(
            output_path.join("resources").join(format!("{}.json", resource_type.to_lowercase())),
            &resource_json,
        )?;
    }

    tracing::info!(
        "Generated test plan with {} test groups, {} total tests, {} resources",
        plan.test_groups.len(),
        plan.total_tests(),
        resources.len(),
    );
    tracing::info!("Output written to: {}", output_dir);

    println!("Generated test plan: {} test groups, {} total tests", plan.test_groups.len(), plan.total_tests());
    println!("Generated {} resource files", resources.len());
    println!("Output directory: {}", output_dir);

    Ok(())
}

/// Run tests against a FHIR server.
pub async fn run_tests(package_path: &str, config_path: &str) -> Result<()> {
    let config = TestConfig::load(config_path)?;
    let orchestrator = runner::Orchestrator::new(config);
    let report = orchestrator.run(package_path).await?;

    println!("{}", report);

    if report.failed > 0 {
        anyhow::bail!("{} test(s) failed", report.failed);
    }

    Ok(())
}

/// Validate a JSON resource against a profile from the IG package.
pub fn run_validate(package_path: &str, resource_path: &str, profile_url: Option<&str>) -> Result<()> {
    let pkg = parse_package(package_path)?;
    let resource_content = std::fs::read_to_string(resource_path)?;
    let resource: serde_json::Value = serde_json::from_str(&resource_content)?;

    let resource_type = resource
        .get("resourceType")
        .and_then(|v| v.as_str())
        .context("Resource JSON missing 'resourceType' field")?;

    // Find the profile
    let profile = if let Some(url) = profile_url {
        pkg.structure_definitions
            .iter()
            .find(|sd| sd.url == url)
            .with_context(|| format!("Profile '{}' not found in IG package", url))?
    } else {
        // Auto-detect by resource type
        pkg.structure_definitions
            .iter()
            .find(|sd| sd.base_type == resource_type)
            .with_context(|| {
                format!(
                    "No profile found for resource type '{}'. Specify --profile explicitly.",
                    resource_type
                )
            })?
    };

    let errors = runner::validate_against_profile(&resource, profile);

    if errors.is_empty() {
        println!("Validation passed for {} against {}", resource_type, profile.url);
    } else {
        println!("Validation failed for {} against {}:", resource_type, profile.url);
        for err in &errors {
            println!("  - {}", err);
        }
    }

    Ok(())
}