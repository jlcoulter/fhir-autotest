# fhir-autotest

[![CI](https://github.com/jlcoulter/fhir-autotest/actions/workflows/ci.yml/badge.svg)](https://github.com/jlcoulter/fhir-autotest/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.88+-blue.svg)](https://www.rust-lang.org)
[![Edition](https://img.shields.io/badge/edition-2024-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

**Automated FHIR R4 conformance testing from Implementation Guide packages.**

Given a FHIR Implementation Guide `.tgz` package, `fhir-autotest` parses the CapabilityStatement, StructureDefinitions, SearchParameters, and OperationDefinitions to generate and execute a comprehensive test suite — no manual test writing required.

It is designed as an open-source alternative to [Inferno](https://inferno.healthit.gov/) for the subset of conformance testing that can be derived automatically from the IG's own machine-readable artifacts.

---

## Quick Start

```bash
# 1. Build
cargo build --release

# 2. Create a config file pointing at your IG package
cat > config.toml << 'EOF'
package = "./package.tgz"

[server]
base_url = "http://localhost:8080/fhir"
EOF

# 3. Run against the built-in mock server (no real FHIR server needed)
fhir-autotest --mock

# 4. Or run against a real FHIR server
fhir-autotest
```

The tool generates test resources, creates them on the server, runs every test, validates responses, cleans up, and writes detailed results to `output/results/`.

---

## What It Tests

For every resource type declared in a server-mode CapabilityStatement, `fhir-autotest` generates:

### Functional Tests

| Category | What's Tested | Example |
|----------|--------------|---------|
| **CRUD interactions** | Every declared interaction | `GET /Patient/{id}`, `POST /Patient`, `PUT /Patient/{id}`, `DELETE /Patient/{id}` |
| **Search parameters** | One test per declared search param with real values from generated resources | `GET /Patient?name=Smith&_id={id}` |
| **Search modifiers** | Type-appropriate modifiers on every param | `:exact`, `:contains` on strings; `:missing` on all types; `:not`, `:text` on tokens |
| **Search prefixes** | All 9 FHIR prefixes on date/number/quantity params | `?birthdate=gt2024-01-01`, `?value-quantity=le5.0` |
| **Near searches** | Proximity queries for `special`-type params | `GET /Location?near=-33.86%7C151.21%7C10%7Ckm` |
| **Combinatorial search** | All 2-parameter combinations within a resource type | `?name=Smith&birthdate=2024-01-01` |
| **Chained search** | Reference params chained into target resource params | `?subject.name=Smith` |
| **`_include` / `_revinclude`** | From the CS's `searchInclude` and `searchRevInclude` declarations | `?_include=Patient:organization`, `?_revinclude=Location:organization` |
| **Result parameters** | `_summary`, `_count`, `_sort` on every searchable resource | `?_summary=true`, `?_count=1`, `?_sort=_lastUpdated` |
| **`$operations`** | Resource-level and system-level operations from the CS | `POST /Patient/$everything`, `POST /$export` |
| **Negative tests** | Read nonexistent ID, search with invalid parameter name | `GET /Patient/nonexistent-id-99999` → 404 |

### Conformance Tests (Responder Obligations)

These verify the server actually meets the obligations it declares — driven entirely by the IG's own artifacts, not hardcoded for any specific IG.

| Category | What It Checks |
|----------|---------------|
| **CS validation** | CapabilityStatement has required fields (`status`, server-mode `rest`, resource `type`, search param `name`/`type`) |
| **MustSupport presence** | Fields marked `mustSupport=true` in declared profiles appear in search responses |
| **Cardinality enforcement** | `min`/`max` constraints from profile ElementDefinitions are respected |
| **Undeclared interaction rejection** | Interactions NOT in the CS are rejected by the server |
| **Undeclared search param handling** | Unknown search parameters are either rejected or silently ignored (both are valid per FHIR spec) |

### Response Assertions

Every test validates responses beyond HTTP status codes:

- **Bundle structure**: `type` must be `"searchset"` for search responses
- **Entry counts**: `_count` tests verify entries ≤ requested count
- **Resource types**: `_include`/`_revinclude` results contain expected target types
- **Field values**: Response fields match values from generated resources
- **MustSupport presence**: Required fields exist in responses regardless of value
- **Sort order**: `_sort` results are ordered by the specified field and direction
- **Summary mode**: `_summary=true` strips narrative `text` and preserves `id`/`meta`
- **Operation outcomes**: Error responses carry the expected severity

---

## How It Works

```
IG Package (.tgz)
    │
    ▼
┌──────────────────────────────────────────────────┐
│ 1. Parse                                         │
│    Extract CapabilityStatement, StructureDefs,   │
│    SearchParameters, OperationDefinitions,       │
│    ValueSets, CodeSystems                       │
└──────────────────┬───────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│ 2. Resolve                                       │
│    • Download missing parent profiles from       │
│      packages.fhir.org and hl7.org               │
│    • Merge parent snapshots for slice definitions│
│    • Resolve profiled types (e.g. au-hpii)       │
│    • Cache everything to ~/.cache/fhir-autotest│
└──────────────────┬───────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│ 3. Generate Resources                            │
│    • Walk profile snapshots, populate required   │
│      fields (min > 0) with type-appropriate data │
│    • Apply fixed/pattern values from profiles    │
│    • Handle sliced fields (identifier:abn, etc.) │
│    • Handle extension slices with correct URLs   │
│    • Resolve ValueSet bindings to real codes     │
│    • Stamp meta.profile with canonical URL       │
│    • Resolve cross-references between types      │
└──────────────────┬───────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│ 4. Generate Test Plan                            │
│    • Build test cases from CapabilityStatement   │
│    • Embed real field values from generated      │
│      resources directly in test URLs             │
│    • Generate conformance tests from profiles    │
│    • Resolve dependency order (topological sort  │
│      with SCC cycle handling)                    │
└──────────────────┬───────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│ 5. Execute                                       │
│    • Create setup resources on the server        │
│    • Run every test case against the server      │
│    • Validate responses against profiles         │
│    • Evaluate response assertions                │
│    • Clean up (delete created resources)         │
│    • Write per-group results + summary to disk   │
└──────────────────────────────────────────────────┘
```

---

## Installation

### From Source

```bash
git clone https://github.com/jlcoulter/fhir-ig-test-generator.git
cd fhir-ig-test-generator
cargo build --release
```

The binary will be at `target/release/fhir-autotest`.

### Docker

```bash
docker build -t fhir-autotest .
docker run --rm -v $(pwd)/config.toml:/config.toml -v $(pwd)/package.tgz:/package.tgz fhir-autotest
```

### Requirements

- Rust 1.88+ (uses edition 2024)
- No system OpenSSL required — uses `rustls` for TLS

---

## Usage

### Basic Commands

```bash
# Full pipeline: generate resources, run tests, validate, clean up
fhir-autotest

# Generate only (no server needed): produces test plan + resources
fhir-autotest --generate

# Preview all test URLs without executing
fhir-autotest --dry-run

# Run against the built-in mock FHIR server
fhir-autotest --mock

# Mock server on a specific port (useful for debugging with curl)
fhir-autotest --mock --mock-port 8091

# Use a different config file
fhir-autotest --config production.toml

# Override the IG package path
fhir-autotest --package path/to/other-ig.tgz
```

The config file defaults to `config.toml` in the current directory. CLI flags override config values.

### Validate a Resource

```bash
# Auto-detect profile by resource type
fhir-autotest validate --resource patient.json

# Specify an explicit profile URL
fhir-autotest validate --resource patient.json \
  --profile "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient"
```

The `validate` subcommand uses the IG package from your config file to find the profile.

### Test Results

After every run, results are written to `{output}/results/`:

```
output/results/
├── summary.json          # Overall totals and per-group breakdowns
├── failed.json           # All failing tests in one file
├── Patient.json          # Full results for Patient test group
├── Observation.json      # Full results for Observation test group
├── _conformance.json     # Conformance test results
└── ...                   # One file per resource type
```

Each per-group file contains the full `TestResult` array with request method, URL, body, response status, response body, and validation errors.

---

## Configuration

See [`config.toml`](config.toml) for a fully commented template. Key settings:

### Top-Level

| Setting | Description | Default |
|---------|-------------|---------|
| `package` | Path to the IG package (`.tgz`) | Required |
| `output` | Output directory for test plan, resources, and results | `./output` |
| `dry_run` | Print all test URLs without executing | `false` |
| `mock` | Use built-in mock FHIR server | `false` |
| `mock_port` | Port for mock server (`0` = random) | `0` |

### `[server]` — Public FHIR API

| Setting | Description |
|---------|-------------|
| `base_url` | Base URL for read/search requests |
| `headers` | Optional HTTP headers (auth tokens, API keys) |

### `[repository]` — Internal Write Endpoint (optional)

When configured, resource upload/delete goes here instead of the public server. This matches production setups where the public API is read-only.

| Setting | Description | Default |
|---------|-------------|---------|
| `base_url` | Internal repository URL | None |
| `username` | Basic auth username (supports `${ENV_VAR}` syntax) | None |
| `password` | Basic auth password (supports `${ENV_VAR}` syntax) | None |
| `credential_file` | Path to a separate credentials file (TOML with `username`/`password`) | None |
| `upload_method` | `"PUT"` (update-as-create) or `"POST"` (server-assigned ID) | `"PUT"` |
| `concurrency` | Parallel requests for upload/delete | `1` |

> **⚠ Security**: Username and password are stored in plaintext in the config file. To avoid committing credentials to version control:
> - Use `${ENV_VAR}` syntax: `username = "${FHIR_REPO_USER}"` — the tool resolves these from the environment at load time.
> - Use `credential_file` to point to a separate file with restricted permissions (e.g. `chmod 600`). The file is a simple TOML file with optional `username` and `password` fields, and also supports `${ENV_VAR}` syntax.
> - Never commit `config.toml` with real credentials to version control.

### `[overrides]` — Manual Control

| Setting | Description |
|---------|-------------|
| `capability_statement_file` | Path to a CapabilityStatement JSON to use instead of the package's CS |
| `creation_order` | Manual resource creation order (overrides auto-resolved order) |
| `fixtures_dir` | Directory for fixture JSON files |
| `fixture_map` | Map of resource type → fixture filename |

### `[data_generation]` — Bulk Test Data

Generate realistic FHIR resources at scale. Resources are written as NDJSON, uploaded before tests, and deleted afterward.

```toml
[data_generation]
counts.Organization = 20_000
counts.Practitioner = 100_000
counts.PractitionerRole = 300_000
counts.Location = 20_000
counts.HealthcareService = 100_000
```

| Setting | Description | Default |
|---------|-------------|---------|
| `counts.{Type}` | Number of resources to generate per FHIR type | None |
| `generate_only` | Generate NDJSON files but skip upload/delete | `false` |

When `data_generation.counts` is configured, the single-resource setup phase is skipped — only bulk data is used. Cross-references between resources are resolved automatically (e.g., PractitionerRole → Practitioner, HealthcareService → Location).

---

## Resource Generation

The resource generator is **profile-aware** — it walks each StructureDefinition's snapshot elements and produces resources that satisfy the IG's constraints:

- **Required fields** (`min > 0`): populated with type-appropriate values
- **Fixed/pattern values**: applied exactly as specified by the profile
- **Sliced fields**: values match slice discriminator patterns (e.g., `identifier:abn` gets the correct `system` URI)
- **Extension slices**: extensions defined by the profile are included with correct URLs and values
- **ValueSet bindings**: when a field is bound to a ValueSet, the generator resolves the actual code system and picks a valid code
- **BackboneElements**: required sub-fields of complex types (e.g., `Practitioner.qualification.identifier`) are populated
- **`meta.profile`**: stamped with the profile's canonical URL so servers can validate conformance
- **Cross-references**: `Reference` fields point to actual created resources, resolved at runtime

Resources are written as `{output}/resources/{ProfileName}.json` — one file per profile, supporting IGs with multiple profiles for the same base type.

---

## Dependency Resolution

Resources are created in dependency order using topological sort. For example, an Observation referencing a Patient will create the Patient first, then substitute the placeholder reference with the actual server-assigned ID.

Circular dependencies (e.g., Organization ↔ Endpoint) are handled via strongly connected component detection — resources in a cycle are created in arbitrary order, with one direction using a placeholder resolved later.

The auto-resolved order can be overridden via `[overrides].creation_order` in config.

---

## Mock Server

The built-in mock FHIR server supports:

- **CRUD**: `POST`, `GET`, `PUT`, `DELETE` for all resource types with UUID assignment
- **Search**: basic parameter filtering (string contains, token/code matching, name/family/given, identifier)
- **Result params**: `_count`, `_summary`, `_sort`, `_elements`, `_include`, `_revinclude`
- **Operations**: `$everything`, `$export`, etc. return stub `Parameters`
- **Update-as-create**: `PUT` to a nonexistent resource creates it (returns 201)
- **404 handling**: read/delete nonexistent resources returns 404 + OperationOutcome

Use it for development, CI, and smoke tests without a real FHIR server:

```bash
fhir-autotest --mock
```

---

## Real-World Example

Testing against the HCPD (Healthcare Provider Directory) IG with 339 generated tests:

```
── Setup: creating resources ──
  PUT http://localhost:8080/fhir/Organization ... → Organization/abc-123
  PUT http://localhost:8080/fhir/Location ... → Location/def-456

── Running 339 test cases against http://localhost:8080/fhir ──

── Patient ──
  → GET /Patient/abc-123 [200]
  → GET /Patient?name=Smith&_id=abc-123 [200]
  → GET /Patient?name:exact=Smith [200]
  → GET /Patient?birthdate=gt2020-01-01 [200]
  ✗ POST /Patient [400]
  → GET /Patient/nonexistent-id-99999 [404]

── _conformance ──
  → GET /Patient?_id=patient-1&_count=10 [200]
  → GET /Patient?__invalid_conformance_test__=value [200]

=== FHIR IG Test Results ===
Total: 339 | Passed: 287 | Failed: 52
```

---

## Comparison to Inferno

| | Inferno | fhir-autotest |
|---|---|---|
| **Test authoring** | Manual (Ruby DSL) | Automatic from IG package |
| **IG coverage** | One test kit per IG | Any FHIR R4 IG package |
| **Profile awareness** | Manual assertions | Auto-generated from StructureDefinitions |
| **Search param coverage** | Manual per-param tests | Exhaustive: every param × every modifier × every prefix |
| **Conformance tests** | Manual | Auto-generated: MustSupport, cardinality, undeclared interactions |
| **Bulk data** | Manual setup | Auto-generated NDJSON with cross-references |
| **Setup/teardown** | Manual | Automatic resource creation and cleanup |
| **Scope** | Full certification testing | Automatable conformance testing |

`fhir-autotest` does not replace Inferno for certification-grade testing. It automates the subset of conformance testing that can be derived from the IG's machine-readable artifacts — the tests you'd otherwise write by hand for every IG.

---

## Project Structure

```
src/
├── main.rs              # CLI entry point (clap)
├── lib.rs               # Public API: run_generate, run_tests, run_dry_run, run_validate
├── model/               # FHIR R4 data types
│   ├── capability.rs    # CapabilityStatement, Rest, RestResource
│   ├── profile.rs       # StructureDefinition, ElementDefinition, Slicing
│   ├── search_param.rs  # SearchParameter
│   └── operation.rs     # OperationDefinition
├── parse/               # IG package parsing
│   ├── package.rs       # .tgz extraction, resource categorization
│   └── profile_resolver.rs  # Parent profile download, cache, merge
├── generate/            # Test plan and resource generation
│   ├── model.rs         # TestCase, TestPlan, ResponseAssertion, enums
│   ├── planner.rs       # Test builders (CRUD, search, modifiers, operations)
│   ├── resource_generator.rs  # Profile-aware resource generation
│   ├── bulk_data.rs     # Bulk NDJSON generation with cross-references
│   ├── conformance.rs   # Conformance test generation (MustSupport, cardinality)
│   ├── dependency_resolver.rs  # Topological sort with SCC cycle handling
│   └── value_resolver.rs  # Field value extraction and search param resolution
├── runner/              # Test execution
│   ├── orchestrator.rs  # Full pipeline orchestration
│   ├── executor.rs      # HTTP request execution, auth, PUT/POST
│   ├── response_assertions.rs  # Bundle, field, sort, include validation
│   ├── validator.rs     # Profile validation
│   └── bulk_loader.rs   # NDJSON upload, delete, R5 extension profiles
├── config/              # Configuration
│   └── models.rs        # TestConfig, ServerConfig, RepositoryConfig
└── mock_server.rs       # In-process mock FHIR server (axum)
```

---

## Development

### Running Tests

```bash
# All tests (unit + integration)
cargo test --all

# Integration test with visible per-request output
cargo test --test integration_test run_against_mock_fhir_server -- --nocapture

# Specific test module
cargo test generate::planner
```

### Code Quality

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Both must pass clean before committing. CI enforces this on every push and PR.

### CI

GitHub Actions runs `fmt`, `clippy`, `cargo test --all`, and `cargo build --release` on every push to `master` and every pull request.

---

## Limitations

- **FHIR R4 only** — no R5 support
- **Local `.tgz` packages only** — no direct NPM registry or Simplifier.net integration
- **Complex extensions** (nested sub-extensions where `value[x]` is prohibited) are not yet handled — simple extensions with concrete `value[x]` types work correctly
- **Chained search params** are limited to 2-hop chains with string target params
- **Combinatorial search** is limited to 2-parameter combinations (configurable depth planned)
- **Profile validation** checks top-level required fields and fixed values, not nested field constraints or FHIRPath invariants

---

## License

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
