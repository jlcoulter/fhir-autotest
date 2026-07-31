use crate::model::*;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use tar::Archive;

/// Parsed contents of a FHIR IG package (.tgz).
#[derive(Debug)]
pub struct IgPackage {
    pub capability_statements: Vec<CapabilityStatement>,
    pub structure_definitions: Vec<StructureDefinition>,
    pub search_parameters: Vec<SearchParameter>,
    pub operation_definitions: Vec<OperationDefinition>,
    pub raw_resources: HashMap<String, Value>,
}

/// Parse a FHIR IG package (.tgz) file.
/// Extracts all JSON resources and categorizes them by resourceType.
pub fn parse_package(path: &str) -> Result<IgPackage> {
    let file =
        std::fs::File::open(path).with_context(|| format!("Failed to open IG package: {path}"))?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);

    let mut capability_statements = Vec::new();
    let mut structure_definitions = Vec::new();
    let mut search_parameters = Vec::new();
    let mut operation_definitions = Vec::new();
    let mut raw_resources = HashMap::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.to_path_buf();
        let path_str = entry_path.to_string_lossy();

        // Only process JSON files in the package/ directory
        if !path_str.starts_with("package/") || !path_str.ends_with(".json") {
            continue;
        }

        let mut content = String::new();
        entry.read_to_string(&mut content)?;

        let json: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("Skipping non-JSON or invalid file {}: {e}", path_str);
                continue;
            }
        };

        let resource_type = json
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match resource_type {
            "CapabilityStatement" => {
                match serde_json::from_value::<CapabilityStatement>(json.clone()) {
                    Ok(cs) => capability_statements.push(cs),
                    Err(e) => {
                        tracing::warn!("Failed to parse CapabilityStatement in {}: {e}", path_str)
                    }
                }
            }
            "StructureDefinition" => {
                match serde_json::from_value::<StructureDefinition>(json.clone()) {
                    Ok(sd) => structure_definitions.push(sd),
                    Err(e) => {
                        tracing::warn!("Failed to parse StructureDefinition in {}: {e}", path_str)
                    }
                }
            }
            "SearchParameter" => match serde_json::from_value::<SearchParameter>(json.clone()) {
                Ok(sp) => search_parameters.push(sp),
                Err(e) => tracing::warn!("Failed to parse SearchParameter in {}: {e}", path_str),
            },
            "OperationDefinition" => {
                match serde_json::from_value::<OperationDefinition>(json.clone()) {
                    Ok(od) => operation_definitions.push(od),
                    Err(e) => {
                        tracing::warn!("Failed to parse OperationDefinition in {}: {e}", path_str)
                    }
                }
            }
            _ => {}
        }

        raw_resources.insert(path_str.to_string(), json);
    }

    tracing::info!(
        "Parsed IG package: {} CapabilityStatements, {} StructureDefinitions, {} SearchParameters, {} OperationDefinitions",
        capability_statements.len(),
        structure_definitions.len(),
        search_parameters.len(),
        operation_definitions.len(),
    );

    Ok(IgPackage {
        capability_statements,
        structure_definitions,
        search_parameters,
        operation_definitions,
        raw_resources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_test_tgz() -> Vec<u8> {
        let mut tar_data = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_data);

            let cs_json = r#"{
                "resourceType": "CapabilityStatement",
                "url": "http://example.org/CapabilityStatement/test",
                "name": "TestCS",
                "status": "active",
                "rest": [{
                    "mode": "server",
                    "resource": [{
                        "type": "Patient",
                        "interaction": [{"code": "read"}, {"code": "search-type"}],
                        "searchParam": [{"name": "name", "type": "string"}]
                    }],
                    "interaction": []
                }]
            }"#;

            let mut header = tar::Header::new_gnu();
            header
                .set_path("package/CapabilityStatement-test.json")
                .unwrap();
            header.set_size(cs_json.len() as u64);
            header.set_cksum();
            tar.append_data(
                &mut header,
                "package/CapabilityStatement-test.json",
                cs_json.as_bytes(),
            )
            .unwrap();

            let sd_json = r#"{
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
                        "id": "Patient.name",
                        "path": "Patient.name",
                        "min": 1,
                        "max": "*",
                        "type": [{"code": "HumanName"}]
                    }]
                }
            }"#;

            let mut header2 = tar::Header::new_gnu();
            header2
                .set_path("package/StructureDefinition-TestPatient.json")
                .unwrap();
            header2.set_size(sd_json.len() as u64);
            header2.set_cksum();
            tar.append_data(
                &mut header2,
                "package/StructureDefinition-TestPatient.json",
                sd_json.as_bytes(),
            )
            .unwrap();

            tar.finish().unwrap();
        }

        let mut gz_data = Vec::new();
        {
            let mut gz =
                flate2::write::GzEncoder::new(&mut gz_data, flate2::Compression::default());
            gz.write_all(&tar_data).unwrap();
            gz.finish().unwrap();
        }
        gz_data
    }

    /// Helper: write a tgz archive from a list of (path, content) entries.
    fn create_custom_tgz(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut tar_data = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_data);
            for (path, content) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(path).unwrap();
                header.set_size(content.len() as u64);
                header.set_cksum();
                tar.append_data(&mut header, path, content.as_bytes())
                    .unwrap();
            }
            tar.finish().unwrap();
        }
        let mut gz_data = Vec::new();
        {
            let mut gz =
                flate2::write::GzEncoder::new(&mut gz_data, flate2::Compression::default());
            gz.write_all(&tar_data).unwrap();
            gz.finish().unwrap();
        }
        gz_data
    }

    /// Helper: write tgz bytes to a temp file and parse it.
    fn parse_tgz_bytes(tgz_data: &[u8]) -> Result<IgPackage> {
        let temp_dir = std::env::temp_dir();
        let tgz_path = temp_dir.join(format!("fhir_test_{}.tgz", uuid::Uuid::new_v4()));
        std::fs::write(&tgz_path, tgz_data).unwrap();
        let result = parse_package(tgz_path.to_str().unwrap());
        let _ = std::fs::remove_file(&tgz_path);
        result
    }

    #[test]
    fn parse_test_package() {
        let tgz_data = create_test_tgz();
        let temp_dir = std::env::temp_dir();
        let tgz_path = temp_dir.join("fhir_test_ig_package.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = parse_package(tgz_path.to_str().unwrap()).unwrap();
        assert_eq!(pkg.capability_statements.len(), 1);
        assert_eq!(pkg.structure_definitions.len(), 1);
        assert_eq!(
            pkg.capability_statements[0].rest[0].resource[0].resource_type,
            "Patient"
        );
        assert_eq!(pkg.structure_definitions[0].base_type, "Patient");
        assert!(pkg.raw_resources.len() >= 2);
    }

    #[test]
    fn parse_nonexistent_file_returns_error() {
        let result = parse_package("/nonexistent/path.tgz");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_package_returns_empty_ig() {
        let tgz_data = create_custom_tgz(&[]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.capability_statements.len(), 0);
        assert_eq!(pkg.structure_definitions.len(), 0);
        assert_eq!(pkg.search_parameters.len(), 0);
        assert_eq!(pkg.operation_definitions.len(), 0);
        assert!(pkg.raw_resources.is_empty());
    }

    #[test]
    fn parse_skips_non_package_directory_files() {
        let tgz_data = create_custom_tgz(&[(
            "META-INF/manifest.xml",
            r#"<?xml version="1.0"?><manifest/>"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.raw_resources.len(), 0);
    }

    #[test]
    fn parse_skips_non_json_files_in_package() {
        let tgz_data =
            create_custom_tgz(&[("package/README.txt", "This is a text file, not JSON.")]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.raw_resources.len(), 0);
    }

    #[test]
    fn parse_skips_invalid_json_in_package() {
        let tgz_data = create_custom_tgz(&[("package/bad.json", "this is not valid json {{{")]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.raw_resources.len(), 0);
    }

    #[test]
    fn parse_handles_unknown_resource_type() {
        let tgz_data = create_custom_tgz(&[(
            "package/CustomResource.json",
            r#"{
                "resourceType": "CustomResource",
                "id": "custom-1",
                "name": "My Custom Resource"
            }"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        // Unknown resource type should still be in raw_resources
        assert_eq!(pkg.raw_resources.len(), 1);
        // But not parsed into any typed collection
        assert_eq!(pkg.capability_statements.len(), 0);
        assert_eq!(pkg.structure_definitions.len(), 0);
        assert_eq!(pkg.search_parameters.len(), 0);
        assert_eq!(pkg.operation_definitions.len(), 0);
    }

    #[test]
    fn parse_handles_search_parameter() {
        let tgz_data = create_custom_tgz(&[(
            "package/SearchParameter-patient-name.json",
            r#"{
                "resourceType": "SearchParameter",
                "url": "http://hl7.org/fhir/SearchParameter/Patient-name",
                "name": "name",
                "code": "name",
                "base": ["Patient"],
                "type": "string"
            }"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.search_parameters.len(), 1);
        assert_eq!(pkg.search_parameters[0].code, "name");
        assert_eq!(pkg.raw_resources.len(), 1);
    }

    #[test]
    fn parse_handles_operation_definition() {
        let tgz_data = create_custom_tgz(&[(
            "package/OperationDefinition-everything.json",
            r#"{
                "resourceType": "OperationDefinition",
                "url": "http://hl7.org/fhir/OperationDefinition/Patient-everything",
                "name": "everything",
                "code": "everything",
                "system": false,
                "instance": true
            }"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.operation_definitions.len(), 1);
        assert_eq!(pkg.operation_definitions[0].code, "everything");
        assert_eq!(pkg.raw_resources.len(), 1);
    }

    #[test]
    fn parse_handles_failed_capability_statement_deserialization() {
        // CapabilityStatement with invalid data that fails deserialization
        let tgz_data = create_custom_tgz(&[(
            "package/CapabilityStatement-bad.json",
            r#"{
                "resourceType": "CapabilityStatement",
                "rest": [{"mode": 123}]
            }"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        // Should still be in raw_resources even if typed parsing failed
        assert_eq!(pkg.raw_resources.len(), 1);
        assert_eq!(pkg.capability_statements.len(), 0);
    }

    #[test]
    fn parse_handles_failed_structure_definition_deserialization() {
        let tgz_data = create_custom_tgz(&[(
            "package/StructureDefinition-bad.json",
            r#"{
                "resourceType": "StructureDefinition",
                "url": "http://example.org/bad"
            }"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.raw_resources.len(), 1);
        assert_eq!(pkg.structure_definitions.len(), 0);
    }

    #[test]
    fn parse_handles_failed_search_parameter_deserialization() {
        let tgz_data = create_custom_tgz(&[(
            "package/SearchParameter-bad.json",
            r#"{
                "resourceType": "SearchParameter",
                "url": "http://example.org/bad"
            }"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.raw_resources.len(), 1);
        assert_eq!(pkg.search_parameters.len(), 0);
    }

    #[test]
    fn parse_handles_failed_operation_definition_deserialization() {
        let tgz_data = create_custom_tgz(&[(
            "package/OperationDefinition-bad.json",
            r#"{
                "resourceType": "OperationDefinition",
                "url": "http://example.org/bad"
            }"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.raw_resources.len(), 1);
        assert_eq!(pkg.operation_definitions.len(), 0);
    }

    #[test]
    fn parse_handles_missing_resource_type() {
        let tgz_data = create_custom_tgz(&[(
            "package/no-resource-type.json",
            r#"{
                "name": "SomeResource",
                "status": "active"
            }"#,
        )]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        // No resourceType means it goes to the _ => {} branch
        assert_eq!(pkg.raw_resources.len(), 1);
        assert_eq!(pkg.capability_statements.len(), 0);
        assert_eq!(pkg.structure_definitions.len(), 0);
    }

    #[test]
    fn parse_handles_multiple_resource_types() {
        let tgz_data = create_custom_tgz(&[
            (
                "package/CapabilityStatement-server.json",
                r#"{
                    "resourceType": "CapabilityStatement",
                    "status": "active",
                    "rest": [{"mode": "server", "resource": [], "interaction": []}]
                }"#,
            ),
            (
                "package/StructureDefinition-Patient.json",
                r#"{
                    "resourceType": "StructureDefinition",
                    "url": "http://example.org/Patient",
                    "name": "Patient",
                    "type": "Patient",
                    "kind": "resource"
                }"#,
            ),
            (
                "package/SearchParameter-name.json",
                r#"{
                    "resourceType": "SearchParameter",
                    "url": "http://example.org/sp-name",
                    "name": "name",
                    "code": "name",
                    "base": ["Patient"],
                    "type": "string"
                }"#,
            ),
            (
                "package/OperationDefinition-validate.json",
                r#"{
                    "resourceType": "OperationDefinition",
                    "url": "http://example.org/op-validate",
                    "name": "validate",
                    "code": "validate"
                }"#,
            ),
        ]);
        let pkg = parse_tgz_bytes(&tgz_data).unwrap();
        assert_eq!(pkg.capability_statements.len(), 1);
        assert_eq!(pkg.structure_definitions.len(), 1);
        assert_eq!(pkg.search_parameters.len(), 1);
        assert_eq!(pkg.operation_definitions.len(), 1);
        assert_eq!(pkg.raw_resources.len(), 4);
    }
}
