# FHIR IG Test Generator — Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** A Rust CLI tool that parses a FHIR R4 Implementation Guide `.tgz` package, auto-generates synthetic test data conforming to the IG's profiles, resolves resource dependencies, and runs a test suite against a FHIR server that validates response conformance against those profiles.

**Architecture:** CLI binary (`fhir-ig-testgen`) with three phases: (1) parse IG package → extract CapabilityStatement + StructureDefinitions, (2) generate test plan including synthetic resources with auto-resolved dependency ordering + fixture overrides, (3) execute tests against a FHIR server with profile-level validation.

**Tech Stack:** Rust, serde/serde_json for FHIR JSON, reqwest for HTTP, flate2 + tar for `.tgz`, petgraph for dependency resolution, clap for CLI, anyhow for errors, tokio async runtime.

---

## Design Decisions

- **Rust** — strong type safety via serde, matches user's stack
- **FHIR R4 only** — most IGs target R4; R5 support deferred
- **Local `.tgz` package input** — no auto-download from registries
- **Profile-level validation** — verify responses conform to IG StructureDefinitions (not just HTTP status)
- **Auto-resolve dependencies with manual overrides** — topological sort of resource references by default, config file allows fixture injection and ordering overrides

## High-Level Architecture

```
┌──────────────┐     ┌─────────────────┐     ┌──────────────────┐
│  IG Package  │────▶│  Parse & Model  │────▶│  Generate Tests  │
│  (.tgz)      │     │  (Capability,  │     │  (test plan,     │
│              │     │   Profiles)     │     │   resources)     │
└──────────────┘     └─────────────────┘     └────────┬─────────┘
                                                       │
                              ┌─────────────────┐      │
                              │  Config Override │──────┤
                              │  (fixtures,     │      │
                              │   ordering)     │      ▼
                              └─────────────────┘ ┌──────────────┐
                                                  │  Test Runner  │
                                                  │  (execute +   │
                                                  │   validate)   │
                                                  └──────┬───────┘
                                                         │
                                                         ▼
                                                  ┌──────────────┐
                                                  │  FHIR Server │
                                                  └──────────────┘
```

## Key Data Structures

### IgPackage (parsed from .tgz)
- `capability_statements: Vec<CapabilityStatement>`
- `structure_definitions: Vec<StructureDefinition>`
- `search_parameters: Vec<SearchParameter>`
- `operation_definitions: Vec<OperationDefinition>`

### CapabilityStatement (core FHIR resource)
- `rest: Vec<Rest>` — each Rest has resources with interactions, search params, operations
- Each resource: type, supported interactions (read, search-type, create, update, delete, etc.), search params, supported profiles

### StructureDefinition (profile constraints)
- `url: String` — canonical URL
- `type: String` — base resource type (Patient, Observation, etc.)
- `elements: Vec<ElementDefinition>` — differential + snapshot elements
- Each element: path, cardinality (min/max), type constraints, fixed values, pattern values, bindings

### TestPlan (output of generation)
- `test_groups: Vec<TestGroup>` — grouped by resource type
- Each TestGroup: resource_type, profile_url, tests (one per interaction), setup_resources
- `creation_order: Vec<ResourceType>` — topologically sorted
- `fixtures: HashMap<String, serde_json::Value>` — user-provided overrides

### TestCase
- `name: String`
- `interaction: Interaction` (read, search, create, update, delete, etc.)
- `resource_type: String`
- `profile_url: Option<String>`
- `request: HttpRequest` (method, path, headers, body)
- `validation: ValidationSpec` (expected status, profile to validate against, required elements)

---

## Task Breakdown

### Task 1: Initialize Rust project

**Objective:** Create the Cargo project with dependencies and module skeleton.

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`
- Create: `src/error.rs`

**Step 1: Create project**

```bash
cd /home/jc/git/fhir-ig-test-generator
cargo init --name fhir-ig-testgen
```

**Step 2: Set up Cargo.toml with dependencies**

```toml
[package]
name = "fhir-ig-testgen"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["json"] }
tokio = { version = "1", features = ["full"] }
flate2 = "1"
tar = "0.4"
petgraph = "0.7"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4"] }
indexmap = "2"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
```

**Step 3: Create module skeleton**

`src/lib.rs`:
```rust
pub mod model;
pub mod parse;
pub mod generate;
pub mod runner;
pub mod config;
```

`src/error.rs` — empty for now, will add custom error types.

`src/main.rs` — minimal clap CLI that prints version.

**Step 4: Verify build**

Run: `cargo build`
Expected: Compiles successfully.

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: initialize project with dependencies and module skeleton"
```

---

### Task 2: Define core FHIR model types

**Objective:** Create Rust structs for the FHIR R4 resources we need to parse: CapabilityStatement, StructureDefinition, ElementDefinition, SearchParameter, OperationDefinition.

**Files:**
- Create: `src/model/capability.rs`
- Create: `src/model/profile.rs`
- Create: `src/model/search_param.rs`
- Create: `src/model/operation.rs`
- Create: `src/model/mod.rs`
- Modify: `src/lib.rs`

**Step 1: Write failing test for deserialization**

Create `src/model/capability.rs` with CapabilityStatement struct and a test that deserializes a minimal CapabilityStatement JSON.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityStatement {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub url: Option<String>,
    pub name: Option<String>,
    pub rest: Vec<Rest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rest {
    pub mode: String,
    pub resource: Vec<RestResource>,
    pub interaction: Vec<RestInteraction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestResource {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub profile: Option<String>,
    pub supportedProfile: Option<Vec<String>>,
    pub interaction: Vec<RestInteraction>,
    pub searchParam: Option<Vec<RestSearchParam>>,
    pub operation: Option<Vec<RestOperation>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestInteraction {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestSearchParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestOperation {
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_capability_statement() {
        let json = r#"{
            "resourceType": "CapabilityStatement",
            "status": "active",
            "name": "test",
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
        let cs: CapabilityStatement = serde_json::from_str(json).unwrap();
        assert_eq!(cs.rest[0].resource[0].resource_type, "Patient");
        assert_eq!(cs.rest[0].resource[0].interaction.len(), 2);
    }
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test --lib model::capability`
Expected: PASS

**Step 3: Create StructureDefinition model**

Create `src/model/profile.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureDefinition {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub url: String,
    #[serde(rename = "type")]
    pub base_type: String,
    pub name: String,
    pub kind: String,
    pub derivation: Option<String>,
    pub snapshot: Option<Snapshot>,
    pub differential: Option<Differential>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub element: Vec<ElementDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Differential {
    pub element: Vec<ElementDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementDefinition {
    pub id: String,
    pub path: String,
    pub min: Option<u32>,
    pub max: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<Vec<ElementDefinitionType>>,
    pub fixed_string: Option<String>,
    pub fixed_uri: Option<String>,
    pub fixed_code: Option<String>,
    pub pattern_string: Option<String>,
    pub pattern_uri: Option<String>,
    pub pattern_code: Option<String>,
    pub binding: Option<ElementBinding>,
    #[serde(rename = "mustSupport")]
    pub must_support: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementDefinitionType {
    pub code: String,
    #[serde(rename = "targetProfile")]
    pub target_profile: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementBinding {
    pub strength: String,
    pub value_set: Option<String>,
}
```

**Step 4: Create SearchParameter model**

Create `src/model/search_param.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParameter {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub url: String,
    pub name: String,
    pub code: String,
    pub base: Vec<String>,
    #[serde(rename = "type")]
    pub param_type: String,
    pub expression: Option<String>,
}
```

**Step 5: Create OperationDefinition model**

Create `src/model/operation.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationDefinition {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub url: String,
    pub name: String,
    pub code: String,
    pub system: Option<bool>,
    #[serde(rename = "type")]
    pub type_: Option<bool>,
    pub instance: Option<bool>,
    pub parameter: Option<Vec<OperationParameter>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationParameter {
    pub name: String,
    pub use_: Option<String>,
    pub min: Option<u32>,
    pub max: Option<String>,
    #[serde(rename = "type")]
    pub param_type: Option<String>,
}
```

**Step 6: Create mod.rs and update lib.rs**

`src/model/mod.rs`:
```rust
pub mod capability;
pub mod profile;
pub mod search_param;
pub mod operation;

pub use capability::*;
pub use profile::*;
pub use search_param::*;
pub use operation::*;
```

Update `src/lib.rs`:
```rust
pub mod model;
pub mod parse;
pub mod generate;
pub mod runner;
pub mod config;
```

**Step 7: Run all tests**

Run: `cargo test`
Expected: All tests pass.

**Step 8: Commit**

```bash
git add -A && git commit -m "feat: add FHIR R4 model types for CapabilityStatement, StructureDefinition, SearchParameter, OperationDefinition"
```

---

### Task 3: Implement IG package parser (tgz extraction + resource deserialization)

**Objective:** Parse a `.tgz` IG package file, extract all JSON resources, and deserialize them into our model types.

**Files:**
- Create: `src/parse/package.rs`
- Create: `src/parse/mod.rs`
- Modify: `src/lib.rs`

**Step 1: Write failing test**

Create `src/parse/package.rs` with an `IgPackage` struct and a `parse_package` function. Write a test that creates a minimal `.tgz` archive in memory with a CapabilityStatement and a StructureDefinition, then verifies they parse correctly.

```rust
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Read;
use flate2::read::GzDecoder;
use tar::Archive;
use serde_json::Value;
use crate::model::*;

pub struct IgPackage {
    pub capability_statements: Vec<CapabilityStatement>,
    pub structure_definitions: Vec<StructureDefinition>,
    pub search_parameters: Vec<SearchParameter>,
    pub operation_definitions: Vec<OperationDefinition>,
    pub raw_resources: HashMap<String, Value>,
}

pub fn parse_package(path: &str) -> Result<IgPackage> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open IG package: {path}"))?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);

    let mut capability_statements = Vec::new();
    let mut structure_definitions = Vec::new();
    let mut search_parameters = Vec::new();
    let mut operation_definitions = Vec::new();
    let mut raw_resources = HashMap::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let path_str = path.to_string_lossy();

        if !path_str.ends_with(".json") {
            continue;
        }

        let mut content = String::new();
        entry.read_to_string(&mut content)?;
        let json: Value = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON in {}", path_str))?;

        let resource_type = json.get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match resource_type {
            "CapabilityStatement" => {
                let cs: CapabilityStatement = serde_json::from_value(json.clone())?;
                capability_statements.push(cs);
            }
            "StructureDefinition" => {
                let sd: StructureDefinition = serde_json::from_value(json.clone())?;
                structure_definitions.push(sd);
            }
            "SearchParameter" => {
                let sp: SearchParameter = serde_json::from_value(json.clone())?;
                search_parameters.push(sp);
            }
            "OperationDefinition" => {
                let od: OperationDefinition = serde_json::from_value(json.clone())?;
                operation_definitions.push(od);
            }
            _ => {}
        }

        raw_resources.insert(path_str.to_string(), json);
    }

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

            // Add a CapabilityStatement
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
            header.set_path("package/CapabilityStatement-test.json").unwrap();
            header.set_size(cs_json.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, "package/CapabilityStatement-test.json", cs_json.as_bytes()).unwrap();

            // Add a StructureDefinition
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
            header2.set_path("package/StructureDefinition-TestPatient.json").unwrap();
            header2.set_size(sd_json.len() as u64);
            header2.set_cksum();
            tar.append_data(&mut header2, "package/StructureDefinition-TestPatient.json", sd_json.as_bytes()).unwrap();

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
    fn parse_test_package() {
        let tgz_data = create_test_tgz();
        let temp_dir = std::env::temp_dir();
        let tgz_path = temp_dir.join("test_ig_package.tgz");
        std::fs::write(&tgz_path, &tgz_data).unwrap();

        let pkg = parse_package(tgz_path.to_str().unwrap()).unwrap();
        assert_eq!(pkg.capability_statements.len(), 1);
        assert_eq!(pkg.structure_definitions.len(), 1);
        assert_eq!(pkg.capability_statements[0].rest[0].resource[0].resource_type, "Patient");
        assert_eq!(pkg.structure_definitions[0].base_type, "Patient");
        assert!(pkg.raw_resources.len() >= 2);
    }
}
```

**Step 2: Run test to verify**

Run: `cargo test --lib parse::package`
Expected: PASS

**Step 3: Create mod.rs and update lib.rs**

`src/parse/mod.rs`:
```rust
pub mod package;

pub use package::*;
```

**Step 4: Run all tests**

Run: `cargo test`
Expected: All pass.

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: implement IG package parser with tgz extraction and resource deserialization"
```

---

### Task 4: Implement test plan model types

**Objective:** Define the data structures for the generated test plan: TestPlan, TestGroup, TestCase, HttpRequest, ValidationSpec, Interaction enum, and fixture config.

**Files:**
- Create: `src/generate/model.rs`
- Create: `src/generate/mod.rs`

**Step 1: Define test plan types**

Create `src/generate/model.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Interaction {
    Read,
    Vread,
    Update,
    Patch,
    Delete,
    Create,
    SearchType,
    HistoryInstance,
    HistoryType,
    Operation(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSpec {
    pub expected_status: u16,
    pub profile_url: Option<String>,
    pub required_elements: Vec<String>,
    pub forbidden_elements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub name: String,
    pub interaction: Interaction,
    pub resource_type: String,
    pub profile_url: Option<String>,
    pub request: HttpRequest,
    pub validation: ValidationSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestGroup {
    pub resource_type: String,
    pub profile_url: Option<String>,
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPlan {
    pub name: String,
    pub ig_url: Option<String>,
    pub test_groups: Vec<TestGroup>,
    pub creation_order: Vec<String>,
    pub setup_resources: HashMap<String, Vec<serde_json::Value>>,
}

impl TestPlan {
    pub fn total_tests(&self) -> usize {
        self.test_groups.iter().map(|g| g.tests.len()).sum()
    }
}
```

**Step 2: Create mod.rs**

`src/generate/mod.rs`:
```rust
pub mod model;
pub mod planner;
pub mod resource_generator;
pub mod dependency_resolver;

pub use model::*;
pub use planner::*;
pub use resource_generator::*;
pub use dependency_resolver::*;
```

**Step 3: Run tests (compilation check)**

Run: `cargo build`
Expected: Compiles successfully (placeholder modules don't exist yet, we'll add them).

Actually — create placeholder files for `planner.rs`, `resource_generator.rs`, `dependency_resolver.rs` with empty modules first.

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: add test plan model types (TestPlan, TestCase, Interaction, ValidationSpec)"
```

---

### Task 5: Implement test plan generator from CapabilityStatement

**Objective:** Given a parsed IgPackage, generate a TestPlan by examining the CapabilityStatement's rest resources and their supported interactions, then creating TestCases for each.

**Files:**
- Create: `src/generate/planner.rs`

**Step 1: Write failing test**

```rust
use super::*;
use crate::model::*;
use crate::generate::model::*;

fn sample_capability_statement() -> CapabilityStatement {
    CapabilityStatement {
        resource_type: "CapabilityStatement".into(),
        url: Some("http://example.org/CapabilityStatement/test".into()),
        name: Some("TestCS".into()),
        rest: vec![Rest {
            mode: "server".into(),
            resource: vec![
                RestResource {
                    resource_type: "Patient".into(),
                    profile: Some("http://hl7.org/fhir/StructureDefinition/Patient".into()),
                    supported_profile: Some(vec!["http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient".into()]),
                    interaction: vec![
                        RestInteraction { code: "read".into() },
                        RestInteraction { code: "search-type".into() },
                        RestInteraction { code: "create".into() },
                        RestInteraction { code: "update".into() },
                    ],
                    search_param: Some(vec![
                        RestSearchParam { name: "name".into(), param_type: "string".into() },
                        RestSearchParam { name: "birthdate".into(), param_type: "date".into() },
                    ]),
                    operation: None,
                },
            ],
            interaction: vec![],
        }],
    }
}

#[test]
fn generate_test_plan_from_capability_statement() {
    let cs = sample_capability_statement();
    let plan = generate_test_plan(&cs, &[], &[], None);

    assert_eq!(plan.test_groups.len(), 1);
    assert_eq!(plan.test_groups[0].resource_type, "Patient");
    assert!(plan.test_groups[0].tests.iter().any(|t| matches!(t.interaction, Interaction::Read)));
    assert!(plan.test_groups[0].tests.iter().any(|t| matches!(t.interaction, Interaction::SearchType)));
    assert!(plan.test_groups[0].tests.iter().any(|t| matches!(t.interaction, Interaction::Create)));
    assert!(plan.test_groups[0].tests.iter().any(|t| matches!(t.interaction, Interaction::Update)));
}
```

**Step 2: Implement generate_test_plan**

The planner maps each supported interaction code to an Interaction enum, builds HttpRequest templates, and creates ValidationSpec entries. For each RestResource in the CapabilityStatement, it generates one TestGroup.

Key logic:
- Map FHIR interaction codes ("read", "search-type", "create", "update", "delete", "vread", "patch", "history-instance", "history-type") to Interaction enum
- For create: POST to `/{resource_type}` with generated resource body
- For read: GET `/{resource_type}/{id}` (depends on create)
- For search-type: GET `/{resource_type}?{param}={value}` (depends on create)
- For update: PUT `/{resource_type}/{id}` (depends on create)
- For delete: DELETE `/{resource_type}/{id}` (depends on create)
- Validation: check status code, validate against profile if profile is specified

**Step 3: Run tests**

Run: `cargo test --lib generate::planner`
Expected: PASS

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: implement test plan generator from CapabilityStatement"
```

---

### Task 6: Implement resource generator (synthetic FHIR data)

**Objective:** Generate minimal valid FHIR resources that conform to a given StructureDefinition profile, using the snapshot elements to determine required fields, cardinalities, types, and fixed/pattern values.

**Files:**
- Create: `src/generate/resource_generator.rs`

**Step 1: Write failing test**

Test that given a StructureDefinition for a US Core Patient profile, the generator produces a valid Patient JSON with required fields filled.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use serde_json::json;

    fn minimal_patient_profile() -> StructureDefinition {
        StructureDefinition {
            resource_type: "StructureDefinition".into(),
            url: "http://example.org/TestPatient".into(),
            base_type: "Patient".into(),
            name: "TestPatient".into(),
            kind: "resource".into(),
            derivation: Some("constraint".into()),
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Patient".into(),
                        path: "Patient".into(),
                        min: Some(0),
                        max: Some("*".into()),
                        type_: None,
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        binding: None,
                        must_support: None,
                    },
                    ElementDefinition {
                        id: "Patient.identifier".into(),
                        path: "Patient.identifier".into(),
                        min: Some(1),
                        max: Some("*".into()),
                        type_: Some(vec![ElementDefinitionType {
                            code: "Identifier".into(),
                            target_profile: None,
                        }]),
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        binding: None,
                        must_support: Some(true),
                    },
                    ElementDefinition {
                        id: "Patient.name".into(),
                        path: "Patient.name".into(),
                        min: Some(1),
                        max: Some("*".into()),
                        type_: Some(vec![ElementDefinitionType {
                            code: "HumanName".into(),
                            target_profile: None,
                        }]),
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        binding: None,
                        must_support: Some(true),
                    },
                ],
            }),
            differential: None,
        }
    }

    #[test]
    fn generate_patient_from_profile() {
        let profile = minimal_patient_profile();
        let resource = generate_resource(&profile).unwrap();
        assert_eq!(resource["resourceType"], "Patient");
        assert!(resource.get("name").is_some(), "name is required (min=1)");
        assert!(resource.get("identifier").is_some(), "identifier is required (min=1)");
    }
}
```

**Step 2: Implement generate_resource**

The generator logic:
1. Start with `{"resourceType": "<base_type>"}` from the profile
2. Walk snapshot elements (skip the root element, which is the resource itself)
3. For each element with `min >= 1` (required):
   - Parse the path (e.g., "Patient.name" → field "name")
   - If element has a fixed/pattern value, use that
   - If element has a type code, generate a minimal value for that type:
     - Primitive types (string, uri, code, boolean, integer, decimal, date, datetime, instant, time, id, oid, uuid, canonical, url, markdown, base64Binary): generate appropriate sentinel values
     - Complex types (Identifier, HumanName, Address, ContactPoint, CodeableConcept, Quantity, Reference, Period, Attachment, Ratio, Annotation, Timing, Range, Ratio, SampledData): generate minimal required sub-structures
     - Reference types: generate placeholder references like `{"reference": "placeholder"}`
   - If element has a binding with a valueSet, note it for future enhancement (for now, use a sentinel value)
4. For optional elements (min=0), skip them
5. Return the generated serde_json::Value

**Step 3: Run tests**

Run: `cargo test --lib generate::resource_generator`
Expected: PASS

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: implement synthetic FHIR resource generator from StructureDefinition profiles"
```

---

### Task 7: Implement dependency resolver (topological sort)

**Objective:** Analyze resource references across profiles to determine creation order. Build a dependency graph where an edge A→B means "A references B, so B must be created first." Perform topological sort.

**Files:**
- Create: `src/generate/dependency_resolver.rs`

**Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_simple_dependency() {
        // Observation references Patient → Patient must be created first
        let deps = vec![
            ("Observation".into(), vec!["Patient".into(), "Encounter".into()]),
            ("Encounter".into(), vec!["Patient".into()]),
            ("Patient".into(), vec![]),
        ];
        let order = resolve_creation_order(&deps).unwrap();
        let patient_idx = order.iter().position(|r| r == "Patient").unwrap();
        let encounter_idx = order.iter().position(|r| r == "Encounter").unwrap();
        let observation_idx = order.iter().position(|r| r == "Observation").unwrap();
        assert!(patient_idx < encounter_idx);
        assert!(patient_idx < observation_idx);
        assert!(encounter_idx < observation_idx);
    }

    #[test]
    fn detect_circular_dependency() {
        let deps = vec![
            ("A".into(), vec!["B".into()]),
            ("B".into(), vec!["C".into()]),
            ("C".into(), vec!["A".into()]),
        ];
        let result = resolve_creation_order(&deps);
        assert!(result.is_err());
    }
}
```

**Step 2: Implement resolve_creation_order**

Use petgraph::algo::toposort:
1. Build a DiGraph<String, ()> from the dependency map
2. Add nodes for each resource type
3. Add edges: if resource A depends on B, add edge A→B (B must come first)
4. Call toposort to get the creation order
5. If toposort returns an error, report a circular dependency

Also implement `extract_dependencies(profiles: &[StructureDefinition]) -> Vec<(String, Vec<String>)>` which scans each profile's snapshot elements for Reference types and extracts their target_profile URIs, mapping those back to resource types.

**Step 3: Run tests**

Run: `cargo test --lib generate::dependency_resolver`
Expected: PASS

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: implement dependency resolver with topological sort"
```

---

### Task 8: Implement config file for overrides (fixtures + ordering)

**Objective:** Allow users to provide a TOML/YAML config file that overrides auto-generated resources with fixture data, specifies manual creation order, and provides server connection details.

**Files:**
- Create: `src/config/mod.rs`
- Create: `src/config/models.rs`

**Step 1: Define config schema**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct TestConfig {
    pub server: ServerConfig,
    #[serde(default)]
    pub overrides: OverrideConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub base_url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OverrideConfig {
    /// Manual creation order (overrides auto-resolved order)
    #[serde(default)]
    pub creation_order: Vec<String>,
    /// Path to fixture JSON files directory
    #[serde(default)]
    pub fixtures_dir: Option<PathBuf>,
    /// Map of resource type → fixture filename to use instead of generating
    #[serde(default)]
    pub fixture_map: HashMap<String, String>,
}
```

Config file format (TOML):

```toml
[server]
base_url = "http://localhost:8080/fhir"
[server.headers]
Authorization = "Bearer token123"

[overrides]
creation_order = ["Patient", "Encounter", "Observation"]
fixtures_dir = "./fixtures"

[overrides.fixture_map]
Patient = "us-core-patient.json"
Observation = "lab-observation.json"
```

**Step 2: Implement config loading**

```rust
impl TestConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: TestConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_fixtures(&self) -> anyhow::Result<HashMap<String, serde_json::Value>> {
        let mut fixtures = HashMap::new();
        if let Some(dir) = &self.overrides.fixtures_dir {
            for (resource_type, filename) in &self.overrides.fixture_map {
                let path = dir.join(filename);
                let content = std::fs::read_to_string(&path)?;
                let value: serde_json::Value = serde_json::from_str(&content)?;
                fixtures.insert(resource_type.clone(), value);
            }
        }
        Ok(fixtures)
    }
}
```

Add `toml = "0.8"` to Cargo.toml dependencies.

**Step 3: Write test**

Test that a TOML config file parses correctly and that fixtures load from a temp directory.

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: add config file support for server settings, creation order overrides, and fixture mappings"
```

---

### Task 9: Implement test runner (execute requests + profile validation)

**Objective:** Execute the test plan against a FHIR server: create setup resources, run each test case, validate responses against profiles.

**Files:**
- Create: `src/runner/mod.rs`
- Create: `src/runner/executor.rs`
- Create: `src/runner/validator.rs`

**Step 1: Implement HTTP executor**

`src/runner/executor.rs`:

```rust
use anyhow::{Context, Result};
use reqwest::Client;
use crate::generate::model::*;
use crate::config::models::*;
use std::collections::HashMap;

pub struct TestExecutor {
    client: Client,
    base_url: String,
    headers: HashMap<String, String>,
}

#[derive(Debug)]
pub struct TestResult {
    pub test_name: String,
    pub passed: bool,
    pub status_code: u16,
    pub response_body: Option<serde_json::Value>,
    pub validation_errors: Vec<String>,
}

impl TestExecutor {
    pub fn new(config: &ServerConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            headers: config.headers.clone(),
        })
    }

    pub async fn execute_test(&self, test: &TestCase) -> Result<TestResult> {
        let url = format!("{}{}", self.base_url, test.request.url);
        let mut req = match test.request.method.as_str() {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            "PUT" => self.client.put(&url),
            "DELETE" => self.client.delete(&url),
            "PATCH" => self.client.patch(&url),
            method => anyhow::bail!("Unsupported HTTP method: {}", method),
        };

        for (key, value) in &self.headers {
            req = req.header(key, value);
        }
        req = req.header("Content-Type", "application/fhir+json");
        req = req.header("Accept", "application/fhir+json");

        if let Some(body) = &test.request.body {
            req = req.json(body);
        }

        let resp = req.send().await
            .with_context(|| format!("Failed to execute {}", test.name))?;
        let status = resp.status().as_u16();
        let body: Option<serde_json::Value> = resp.json().await.ok();

        Ok(TestResult {
            test_name: test.name.clone(),
            passed: status == test.validation.expected_status,
            status_code: status,
            response_body: body,
            validation_errors: Vec::new(),
        })
    }

    pub async fn create_resource(&self, resource_type: &str, body: &serde_json::Value) -> Result<(String, serde_json::Value)> {
        let url = format!("{}/{}", self.base_url, resource_type);
        let resp = self.client.post(&url)
            .headers(self.headers.iter().map(|(k,v)| (k.clone(), v.clone())).collect())
            .header("Content-Type", "application/fhir+json")
            .header("Accept", "application/fhir+json")
            .json(body)
            .send()
            .await
            .with_context(|| format!("Failed to create {}", resource_type))?;

        let status = resp.status();
        let created: serde_json::Value = resp.json().await
            .context("Failed to parse created resource response")?;

        if status.as_u16() != 201 {
            anyhow::bail!("Expected 201 Created, got {}: {:?}", status.as_u16(), created);
        }

        let id = created.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Created resource missing id"))?
            .to_string();

        Ok((id, created))
    }

    pub async fn delete_resource(&self, resource_type: &str, id: &str) -> Result<()> {
        let url = format!("{}/{}/{}", self.base_url, resource_type, id);
        self.client.delete(&url)
            .headers(self.headers.iter().map(|(k,v)| (k.clone(), v.clone())).collect())
            .send()
            .await?;
        Ok(())
    }
}
```

**Step 2: Implement profile validator**

`src/runner/validator.rs`:

The validator checks a FHIR resource response against a StructureDefinition profile:
- Verify resourceType matches the profile's base type
- For each element in the profile's snapshot with min > 0, verify the field exists in the response
- For each element with fixed/pattern values, verify they match
- For each element with a binding, note if the value is in the bound ValueSet (future: load expansions)

```rust
use crate::model::*;
use crate::generate::model::*;

pub fn validate_against_profile(
    resource: &serde_json::Value,
    profile: &StructureDefinition,
) -> Vec<String> {
    let mut errors = Vec::new();

    // Check resourceType
    if let Some(rt) = resource.get("resourceType").and_then(|v| v.as_str()) {
        if rt != profile.base_type {
            errors.push(format!("resourceType is '{}', expected '{}'", rt, profile.base_type));
        }
    } else {
        errors.push("Missing resourceType".into());
    }

    // Check required elements
    if let Some(snapshot) = &profile.snapshot {
        for element in &snapshot.element {
            if element.min.unwrap_or(0) > 0 {
                // Extract field name from path (e.g., "Patient.name" → "name")
                let field_name = element.path.split('.').last().unwrap_or("");
                if field_name == profile.base_type || field_name.is_empty() {
                    continue; // Skip root element
                }
                if resource.get(field_name).is_none() {
                    errors.push(format!("Missing required element: {} (min={})", element.path, element.min.unwrap_or(0)));
                }
            }
        }
    }

    // Check fixed/pattern values
    if let Some(snapshot) = &profile.snapshot {
        for element in &snapshot.element {
            let field_name = element.path.split('.').last().unwrap_or("");
            if field_name == profile.base_type || field_name.is_empty() {
                continue;
            }
            if let Some(fixed) = &element.fixed_string {
                if let Some(val) = resource.get(field_name).and_then(|v| v.as_str()) {
                    if val != fixed {
                        errors.push(format!("{}: expected '{}', got '{}'", element.path, fixed, val));
                    }
                }
            }
            if let Some(pattern) = &element.pattern_string {
                if let Some(val) = resource.get(field_name).and_then(|v| v.as_str()) {
                    if val != pattern {
                        errors.push(format!("{}: pattern expected '{}', got '{}'", element.path, pattern, val));
                    }
                }
            }
        }
    }

    errors
}
```

**Step 3: Write integration test**

Test that validator catches missing required fields and wrong resourceType. Use mock profile data.

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: implement test executor and profile validator"
```

---

### Task 10: Implement test runner orchestration (tie it all together)

**Objective:** Wire up the full pipeline: parse IG → generate test plan → generate resources → resolve dependencies → execute tests → validate → report results.

**Files:**
- Create: `src/runner/orchestrator.rs`
- Modify: `src/runner/mod.rs`

**Step 1: Implement orchestrator**

```rust
use anyhow::{Context, Result};
use crate::parse::package::*;
use crate::generate::model::*;
use crate::generate::planner::*;
use crate::generate::resource_generator::*;
use crate::generate::dependency_resolver::*;
use crate::config::models::*;
use crate::runner::executor::*;
use crate::runner::validator::*;
use std::collections::HashMap;

pub struct Orchestrator {
    config: TestConfig,
}

impl Orchestrator {
    pub fn new(config: TestConfig) -> Self {
        Self { config }
    }

    pub async fn run(&self, ig_package_path: &str) -> Result<RunReport> {
        // 1. Parse the IG package
        let pkg = parse_package(ig_package_path)?;

        // 2. Extract dependencies and determine creation order
        let auto_deps = extract_dependencies(&pkg.structure_definitions);
        let creation_order = if self.config.overrides.creation_order.is_empty() {
            resolve_creation_order(&auto_deps)?
        } else {
            self.config.overrides.creation_order.clone()
        };

        // 3. Load fixture overrides
        let fixtures = self.config.load_fixtures()?;

        // 4. Generate or load resources for each type
        let mut resources: HashMap<String, serde_json::Value> = HashMap::new();
        for resource_type in &creation_order {
            if let Some(fixture) = fixtures.get(resource_type) {
                resources.insert(resource_type.clone(), fixture.clone());
            } else {
                // Find the profile for this resource type
                let profile = pkg.structure_definitions.iter()
                    .find(|sd| sd.base_type == *resource_type)
                    .cloned()
                    .or_else(|| pkg.structure_definitions.iter().find(|sd| sd.base_type == *resource_type).cloned());
                if let Some(profile) = profile {
                    let generated = generate_resource(&profile)?;
                    resources.insert(resource_type.clone(), generated);
                }
            }
        }

        // 5. Generate test plan
        let cs = pkg.capability_statements.first()
            .context("No CapabilityStatement found in IG package")?;
        let plan = generate_test_plan(cs, &pkg.structure_definitions, &pkg.search_parameters, None);

        // 6. Execute: create setup resources, run tests
        let executor = TestExecutor::new(&self.config.server)?;
        let mut created_ids: HashMap<String, String> = HashMap::new();
        let mut results = Vec::new();

        // Create resources in dependency order
        for resource_type in &creation_order {
            if let Some(body) = resources.get(resource_type) {
                // Replace placeholder references with actual created IDs
                let mut body = body.clone();
                self.resolve_references(&mut body, &created_ids);

                let (id, _) = executor.create_resource(resource_type, &body).await?;
                created_ids.insert(resource_type.clone(), id);
            }
        }

        // Run test cases
        for group in &plan.test_groups {
            for test in &group.tests {
                let mut test = test.clone();
                // Replace {id} placeholders in URLs
                if let Some(id) = created_ids.get(&test.resource_type) {
                    test.request.url = test.request.url.replace("{id}", id);
                }
                let mut result = executor.execute_test(&test).await?;

                // Profile validation
                if let Some(profile_url) = &test.validation.profile_url {
                    if let Some(response_body) = &result.response_body {
                        if let Some(profile) = pkg.structure_definitions.iter().find(|sd| &sd.url == profile_url) {
                            let errors = validate_against_profile(response_body, profile);
                            result.validation_errors.extend(errors);
                        }
                    }
                }

                result.passed = result.passed && result.validation_errors.is_empty();
                results.push(result);
            }
        }

        // Cleanup: delete created resources in reverse order
        for resource_type in creation_order.iter().rev() {
            if let Some(id) = created_ids.get(resource_type) {
                let _ = executor.delete_resource(resource_type, id).await;
            }
        }

        Ok(RunReport {
            total: results.len(),
            passed: results.iter().filter(|r| r.passed).count(),
            failed: results.iter().filter(|r| !r.passed).count(),
            results,
        })
    }

    fn resolve_references(&self, body: &mut serde_json::Value, created_ids: &HashMap<String, String>) {
        // Walk the JSON and replace "reference": "placeholder:ResourceType" with actual IDs
        if let Some(obj) = body.as_object_mut() {
            for (key, value) in obj.iter_mut() {
                if key == "reference" {
                    if let Some(s) = value.as_str() {
                        if let Some(rest) = s.strip_prefix("placeholder:") {
                            if let Some(id) = created_ids.get(rest) {
                                *value = serde_json::Value::String(format!("{}/{}", rest, id));
                            }
                        }
                    }
                } else {
                    self.resolve_references(value, created_ids);
                }
            }
        }
        if let Some(arr) = body.as_array_mut() {
            for item in arr.iter_mut() {
                self.resolve_references(item, created_ids);
            }
        }
    }
}

#[derive(Debug)]
pub struct RunReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<TestResult>,
}

impl std::fmt::Display for RunReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Test Results ===")?;
        writeln!(f, "Total: {} | Passed: {} | Failed: {}", self.total, self.passed, self.failed)?;
        writeln!(f, "---")?;
        for result in &self.results {
            let status = if result.passed { "PASS" } else { "FAIL" };
            writeln!(f, "[{}] {} (HTTP {})", status, result.test_name, result.status_code)?;
            for err in &result.validation_errors {
                writeln!(f, "  ✗ {}", err)?;
            }
        }
        Ok(())
    }
}
```

**Step 2: Write test for the full pipeline**

This is more of an integration test. Create a minimal IG package as a fixture and test the orchestrator with a mock server (or skip the server part and just test the generation/validation).

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: implement orchestrator that ties together parsing, generation, and execution"
```

---

### Task 11: Implement CLI interface

**Objective:** Create a clap-based CLI with subcommands: `generate` (parse + generate test plan + resources), `run` (parse + generate + execute), and `validate` (validate a JSON resource against a profile).

**Files:**
- Modify: `src/main.rs`

**Step 1: Implement CLI with clap derive**

```rust
use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "fhir-ig-testgen")]
#[command(about = "FHIR IG Test Generator — parse IG packages and generate/run conformance tests")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate test plan and synthetic resources from an IG package
    Generate {
        /// Path to the IG package (.tgz)
        #[arg(short, long)]
        package: String,

        /// Path to config file (TOML)
        #[arg(short, long)]
        config: Option<String>,

        /// Output directory for generated test plan and resources
        #[arg(short, long, default_value = "./output")]
        output: String,
    },
    /// Run tests against a FHIR server
    Run {
        /// Path to the IG package (.tgz)
        #[arg(short, long)]
        package: String,

        /// Path to config file (TOML) with server URL
        #[arg(short, long)]
        config: String,
    },
    /// Validate a JSON resource against a profile from the IG package
    Validate {
        /// Path to the IG package (.tgz)
        #[arg(short, long)]
        package: String,

        /// Path to the resource JSON file
        #[arg(short, long)]
        resource: String,

        /// Profile URL to validate against
        #[arg(long)]
        profile: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate { package, config, output } => {
            // Parse IG, generate test plan, write to output dir
            todo!()
        }
        Commands::Run { package, config } => {
            // Parse IG, generate test plan, execute against server
            todo!()
        }
        Commands::Validate { package, resource, profile } => {
            // Parse IG, load resource, validate against profile
            todo!()
        }
    }

    Ok(())
}
```

**Step 2: Wire up generate subcommand**

Implement the `Generate` command:
1. Parse the IG package
2. Load config if provided
3. Extract dependencies and resolve creation order
4. Generate resources (or load fixtures)
5. Generate test plan from CapabilityStatement
6. Write test plan (JSON) and resources (JSON) to output directory

**Step 3: Wire up run subcommand**

Implement the `Run` command:
1. Parse the IG package
2. Load config (required for server URL)
3. Create orchestrator and run
4. Print the RunReport

**Step 4: Wire up validate subcommand**

Implement the `Validate` command:
1. Parse the IG package
2. Load the resource JSON
3. Find the profile (by URL or by resource type)
4. Run validation and print errors

**Step 5: Test CLI**

Run: `cargo run -- --help`
Run: `cargo run -- generate --package test.tgz --output ./output`
Expected: Help text, then actual execution

**Step 6: Commit**

```bash
git add -A && git commit -m "feat: implement CLI with generate, run, and validate subcommands"
```

---

### Task 12: Integration test with real IG package

**Objective:** Write an integration test that uses a real (small) FHIR IG package to verify the full pipeline works end-to-end. Download the US Core R4 package for testing.

**Files:**
- Create: `tests/integration_test.rs`
- Create: `tests/fixtures/` (test data directory)

**Step 1: Download US Core R4 package for testing**

```bash
mkdir -p tests/fixtures
curl -L -o tests/fixtures/hl7.fhir.us.core.tgz \
  "https://packages.fhir.org/hl7.fhir.us.core/6.1.0"
```

If this isn't available, create a minimal test package fixture programmatically.

**Step 2: Write integration test**

```rust
use assert_cmd::Command;

#[test]
fn generate_from_us_core_package() {
    // This test requires the US Core package to be present
    let pkg_path = "tests/fixtures/hl7.fhir.us.core.tgz";
    if !std::path::Path::new(pkg_path).exists() {
        eprintln!("Skipping: US Core package not found at {}", pkg_path);
        return;
    }

    let output_dir = std::env::temp_dir().join("fhir_ig_testgen_output");
    let mut cmd = Command::cargo_bin("fhir-ig-testgen").unwrap();
    cmd.args(["generate", "--package", pkg_path, "--output", output_dir.to_str().unwrap()]);
    cmd.assert().success();

    // Verify output directory contains expected files
    assert!(output_dir.join("test_plan.json").exists());
    assert!(output_dir.join("resources").is_dir());
}
```

**Step 3: Create a minimal test package fixture**

Write a test helper that programmatically creates a minimal `.tgz` package with:
- One CapabilityStatement (Patient, Observation with common interactions)
- Two StructureDefinitions (a Patient profile and an Observation profile)
- One SearchParameter
- Where Observation references Patient (testing dependency resolution)

**Step 4: Test with the minimal fixture**

```rust
#[test]
fn generate_from_minimal_package() {
    let pkg_path = create_minimal_test_package(); // helper that creates .tgz
    let output_dir = std::env::temp_dir().join("fhir_ig_testgen_minimal");
    // ... run generate and verify
}
```

**Step 5: Commit**

```bash
git add -A && git commit -m "test: add integration tests with minimal and US Core IG package fixtures"
```

---

### Task 13: Polish and documentation

**Objective:** Add README, error handling improvements, and final touches.

**Files:**
- Create: `README.md`
- Modify: various files for error handling

**Step 1: Write README.md**

```markdown
# fhir-ig-testgen

A Rust CLI tool that parses FHIR R4 Implementation Guide packages (.tgz),
generates synthetic test data conforming to the IG's profiles, and runs
conformance tests against a FHIR server.

## Features

- Parse FHIR R4 IG packages (.tgz NPM format)
- Auto-generate synthetic resources from StructureDefinition profiles
- Auto-resolve resource creation dependencies (topological sort)
- Override generated resources with fixture files
- Run conformance tests with profile-level validation
- Configurable via TOML config files

## Usage

### Generate test plan and resources

```bash
fhir-ig-testgen generate --package path/to/ig-package.tgz --output ./output
```

### Run tests against a FHIR server

```bash
fhir-ig-testgen run --package path/to/ig-package.tgz --config config.toml
```

### Validate a resource against a profile

```bash
fhir-ig-testgen validate --package path/to/ig-package.tgz --resource patient.json
```

## Configuration

Create a `config.toml`:

```toml
[server]
base_url = "http://localhost:8080/fhir"

[server.headers]
Authorization = "Bearer token123"

[overrides]
creation_order = ["Patient", "Encounter", "Observation"]
fixtures_dir = "./fixtures"

[overrides.fixture_map]
Patient = "us-core-patient.json"
```

## Test Data Generation

The tool analyzes each StructureDefinition's snapshot to determine:
- Required fields (min > 0) and generates appropriate values
- Fixed/pattern values and enforces them
- Reference types and creates placeholder references
- Dependency ordering via topological sort

You can override any auto-generated resource with a fixture file.
```

**Step 2: Add proper error handling with anyhow contexts**

Review all `unwrap()` calls in non-test code and replace with proper `?` and `.context()` chains.

**Step 3: Add `--verbose` flag and logging**

Add `tracing` for structured logging, with a `--verbose` / `-v` flag.

**Step 4: Final build and test**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All pass, no warnings.

**Step 5: Commit**

```bash
git add -A && git commit -m "docs: add README, improve error handling, add tracing"
```

---

## Summary

| Task | Description | Key Files |
|------|-------------|-----------|
| 1 | Initialize Rust project | `Cargo.toml`, `src/main.rs`, `src/lib.rs` |
| 2 | FHIR model types | `src/model/*.rs` |
| 3 | IG package parser | `src/parse/package.rs` |
| 4 | Test plan model types | `src/generate/model.rs` |
| 5 | Test plan generator | `src/generate/planner.rs` |
| 6 | Resource generator | `src/generate/resource_generator.rs` |
| 7 | Dependency resolver | `src/generate/dependency_resolver.rs` |
| 8 | Config file support | `src/config/models.rs` |
| 9 | Test executor + validator | `src/runner/executor.rs`, `src/runner/validator.rs` |
| 10 | Orchestrator | `src/runner/orchestrator.rs` |
| 11 | CLI interface | `src/main.rs` |
| 12 | Integration tests | `tests/integration_test.rs` |
| 13 | Polish & docs | `README.md`, error handling |

**Key risks:**
- FHIR JSON can have inconsistent/optional fields — serde defaults + `Option<T>` handles most cases, but some profiles may use extensions we don't model. Mitigation: keep raw JSON fallback.
- Circular dependencies in profiles (rare but possible) — handled by topological sort error detection.
- Large IG packages (US Core has 100+ profiles) — performance should be fine since we're just parsing JSON, but memory could be a concern for very large packages.
- Profile validation is approximate (no full FHIR path evaluation) — we validate required elements and fixed values, not full FHIRPath invariants.