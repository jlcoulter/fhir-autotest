pub mod config;
pub mod generate;
pub mod mock_server;
pub mod model;
pub mod parse;
pub mod runner;

// Re-export key types
pub use config::models::*;
pub use generate::*;
pub use model::*;
pub use parse::*;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// Select the CapabilityStatement used to generate responder-driven tests.
///
/// If `overrides.capability_statement_file` is configured, that JSON file is
/// loaded and used. Otherwise, the best match from the IG package is selected:
/// server-mode first, then any with resources, then the first entry.
pub(crate) fn select_capability_statement(
    pkg: &IgPackage,
    config: &TestConfig,
) -> Result<CapabilityStatement> {
    if let Some(path) = &config.overrides.capability_statement_file {
        let content = std::fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read CapabilityStatement override: {}",
                path.display()
            )
        })?;
        let json: serde_json::Value = serde_json::from_str(&content).with_context(|| {
            format!(
                "CapabilityStatement override is not valid JSON: {}",
                path.display()
            )
        })?;
        let resource_type = json
            .get("resourceType")
            .and_then(|v| v.as_str())
            .context("CapabilityStatement override JSON is missing 'resourceType'")?;
        if resource_type != "CapabilityStatement" {
            anyhow::bail!(
                "CapabilityStatement override must have resourceType='CapabilityStatement' (found '{}')",
                resource_type
            );
        }
        let cs: CapabilityStatement = serde_json::from_value(json).with_context(|| {
            format!(
                "Failed to deserialize CapabilityStatement override: {}",
                path.display()
            )
        })?;
        tracing::info!("Using CapabilityStatement override from {}", path.display());
        return Ok(cs);
    }

    pkg.capability_statements
        .iter()
        .find(|cs| {
            cs.rest
                .iter()
                .any(|r| r.mode == "server" && !r.resource.is_empty())
        })
        .or_else(|| {
            pkg.capability_statements
                .iter()
                .find(|cs| cs.rest.iter().any(|r| !r.resource.is_empty()))
        })
        .or(pkg.capability_statements.first())
        .cloned()
        .context("No CapabilityStatement found in IG package")
}

/// Generate a test plan and resources from an IG package.
/// Writes output to the config's output directory.
///
/// Generates one resource per profile, named after the profile (e.g.
/// `TestPatient.json`), and includes `meta.profile` with the profile URL.
pub async fn run_generate(package_path: &str, config: &TestConfig) -> Result<()> {
    let pkg = parse_package(package_path)?;
    let value_set_systems = build_value_set_system_map(&pkg.raw_resources);
    let cs = select_capability_statement(&pkg, config)?;

    // Resolve parent profile chains — download missing parent profiles
    // from the FHIR package registry and merge their snapshots so that
    // slice definitions with discriminator patterns are available.
    let mut profiles = pkg.structure_definitions;
    resolve_parent_chain(&mut profiles).await?;

    // Resolve dependencies (by resource type)
    let auto_deps = extract_dependencies(&profiles);
    let auto_order = resolve_creation_order(&auto_deps)?;
    let creation_order = merge_creation_order(&auto_order, &config.overrides.creation_order);

    // Load fixture overrides
    let fixtures = config.load_fixtures()?;

    // Generate or load resources for EACH profile (not just one per type).
    // Each gets a unique filename based on the profile name.
    let mut profile_resources: Vec<(String, String, serde_json::Value)> = Vec::new(); // (profile_name, resource_type, json)

    // Track which resource types we've already generated a resource for,
    // so the orchestrator can pick one per type for the setup phase.
    let mut type_to_profile: HashMap<String, String> = HashMap::new();

    for resource_type in &creation_order {
        // Check fixtures first
        if let Some(fixture) = fixtures.get(resource_type) {
            let profile_name = resource_type.clone();
            profile_resources.push((profile_name.clone(), resource_type.clone(), fixture.clone()));
            if !type_to_profile.contains_key(resource_type) {
                type_to_profile.insert(resource_type.clone(), profile_name);
            }
            continue;
        }

        // Generate one resource per profile for this resource type
        let profiles_for_type: Vec<_> = profiles
            .iter()
            .filter(|sd| sd.base_type == *resource_type)
            .collect();

        if profiles_for_type.is_empty() {
            tracing::warn!(
                "No profile found for {}, skipping resource generation",
                resource_type
            );
            continue;
        }

        for profile in profiles_for_type {
            let generated =
                generate_resource_with_value_sets(profile, &profiles, &value_set_systems)?;
            // Use the profile name as the unique key (e.g. "TestPatient", "HcpdPractitioner")
            let profile_name = profile.name.clone();
            profile_resources.push((profile_name.clone(), resource_type.clone(), generated));
            if !type_to_profile.contains_key(resource_type) {
                type_to_profile.insert(resource_type.clone(), profile_name);
            }
        }
    }

    // Generate test plan
    let mut plan = generate_test_plan(
        &cs,
        &profiles,
        &pkg.search_parameters,
        Some(&pkg.operation_definitions),
        None,
    );
    plan.creation_order = creation_order.clone();

    // Write output
    let output_path = Path::new(&config.output);
    std::fs::create_dir_all(output_path)?;
    std::fs::create_dir_all(output_path.join("resources"))?;

    // Write test plan
    let plan_json = serde_json::to_string_pretty(&plan)?;
    std::fs::write(output_path.join("test_plan.json"), &plan_json)?;

    // Write resources — one file per profile, named after the profile
    for (profile_name, _resource_type, resource) in &profile_resources {
        let resource_json = serde_json::to_string_pretty(resource)?;
        let filename = format!("{}.json", profile_name);
        std::fs::write(
            output_path.join("resources").join(&filename),
            &resource_json,
        )?;
        tracing::info!("Wrote resource file: {}", filename);
    }

    tracing::info!(
        "Generated test plan with {} test groups, {} total tests, {} resources",
        plan.test_groups.len(),
        plan.total_tests(),
        profile_resources.len(),
    );
    tracing::info!("Output written to: {}", config.output);

    println!(
        "Generated test plan: {} test groups, {} total tests",
        plan.test_groups.len(),
        plan.total_tests()
    );
    println!("Generated {} resource files", profile_resources.len());
    println!("Output directory: {}", config.output);

    Ok(())
}

/// Run tests against a FHIR server.
/// Always writes per-group results and a summary into `{output}/results/`.
pub async fn run_tests(package_path: &str, config: &TestConfig) -> Result<()> {
    let orchestrator = runner::Orchestrator::new(config.clone());
    let report = orchestrator.run(package_path).await?;

    println!("{}", report);

    // Write per-group result files and summary into the output directory
    let output_dir = std::path::Path::new(&config.output);
    report.write_results(output_dir)?;
    println!("\nResults written to: {}/results/", config.output);

    if report.failed > 0 {
        anyhow::bail!("{} test(s) failed", report.failed);
    }

    Ok(())
}

/// Dry-run: generate the test plan and print all test URLs without executing them.
pub async fn run_dry_run(package_path: &str, config: &TestConfig) -> Result<()> {
    let pkg = parse_package(package_path)?;
    let value_set_systems = build_value_set_system_map(&pkg.raw_resources);
    let cs = select_capability_statement(&pkg, config)?;

    // Resolve parent profile chains
    let mut profiles = pkg.structure_definitions;
    resolve_parent_chain(&mut profiles).await?;

    let auto_deps = extract_dependencies(&profiles);
    let auto_order = resolve_creation_order(&auto_deps)?;
    let creation_order = merge_creation_order(&auto_order, &config.overrides.creation_order);

    let fixtures = config.load_fixtures()?;

    // Build a type-keyed map for dry-run display
    let mut resources: HashMap<String, serde_json::Value> = HashMap::new();
    for resource_type in &creation_order {
        if let Some(fixture) = fixtures.get(resource_type) {
            resources.insert(resource_type.clone(), fixture.clone());
        } else {
            // Use first profile for each type
            let profile = profiles.iter().find(|sd| sd.base_type == *resource_type);
            if let Some(profile) = profile {
                let generated =
                    generate_resource_with_value_sets(profile, &profiles, &value_set_systems)?;
                resources.insert(resource_type.clone(), generated);
            }
        }
    }

    let mut plan = generate_test_plan(
        &cs,
        &profiles,
        &pkg.search_parameters,
        Some(&pkg.operation_definitions),
        None,
    );
    plan.creation_order = creation_order.clone();

    println!();
    println!(
        "=== Dry Run: {} test groups, {} total tests ===",
        plan.test_groups.len(),
        plan.total_tests()
    );
    println!();
    println!("Read endpoint (GET/search):  {}", config.server.base_url);
    match &config.repository {
        Some(repo) => println!(
            "Write endpoint ({}): {} (user: {})",
            repo.upload_method.to_uppercase(),
            repo.base_url,
            repo.username
        ),
        None => println!(
            "Write endpoint (PUT): {} (same as read)",
            config.server.base_url
        ),
    }
    println!();

    let write_url = match &config.repository {
        Some(repo) => &repo.base_url,
        None => &config.server.base_url,
    };
    let upload_method = config
        .repository
        .as_ref()
        .map(|r| r.upload_method.to_uppercase())
        .unwrap_or_else(|| "PUT".to_string());

    println!("Setup resources (creation order):");
    for rt in &creation_order {
        if resources.contains_key(rt) {
            println!("  {} {}/{}  [will create]", upload_method, write_url, rt);
        } else {
            println!(
                "  {} {}/{}  [no profile — skipped]",
                upload_method, write_url, rt
            );
        }
    }
    println!();

    println!("Test cases:");
    let mut last_group = String::new();
    for group in &plan.test_groups {
        for test in &group.tests {
            if group.resource_type != last_group {
                println!();
                println!("── {} ──", group.resource_type);
                last_group = group.resource_type.clone();
            }
            println!(
                "  {} {}{}",
                test.request.method, config.server.base_url, test.request.url
            );
        }
    }
    println!();
    println!(
        "Cleanup: DELETE {} resources from {}",
        creation_order.len(),
        write_url
    );
    println!();

    Ok(())
}

/// Validate a JSON resource against a profile from the IG package.
pub async fn run_validate(
    package_path: &str,
    resource_path: &str,
    profile_url: Option<&str>,
) -> Result<()> {
    let pkg = parse_package(package_path)?;

    // Resolve parent profile chains
    let mut profiles = pkg.structure_definitions;
    resolve_parent_chain(&mut profiles).await?;

    let resource_content = std::fs::read_to_string(resource_path)?;
    let resource: serde_json::Value = serde_json::from_str(&resource_content)?;

    let resource_type = resource
        .get("resourceType")
        .and_then(|v| v.as_str())
        .context("Resource JSON missing 'resourceType' field")?;

    // Find the profile
    let profile = if let Some(url) = profile_url {
        profiles
            .iter()
            .find(|sd| sd.url == url)
            .with_context(|| format!("Profile '{}' not found in IG package", url))?
    } else {
        // Auto-detect by resource type
        profiles
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
        println!(
            "Validation passed for {} against {}",
            resource_type, profile.url
        );
    } else {
        println!(
            "Validation failed for {} against {}:",
            resource_type, profile.url
        );
        for err in &errors {
            println!("  - {}", err);
        }
    }

    Ok(())
}
