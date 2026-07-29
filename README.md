# fhir-ig-testgen

A Rust CLI tool that parses FHIR R4 Implementation Guide packages (`.tgz`) and automatically generates conformance tests with synthetic test data.

Given an IG package, it:

1. Parses the CapabilityStatement to discover supported resources and interactions
2. Parses StructureDefinitions (profiles) to understand required fields and constraints
3. Generates synthetic FHIR resources that satisfy profile constraints
4. Resolves resource dependencies via topological sort (Patient before Observation, etc.)
5. Creates a test plan with HTTP requests for every supported interaction
6. Runs tests against a FHIR server and validates responses against profiles and assertions

## Installation

```bash
cargo build --release
```

## Usage

Create a `config.toml` (see [`config.toml`](config.toml) for a full example):

```toml
package = "path/to/ig-package.tgz"
output = "./output"
results = "./results.json"

[server]
base_url = "http://localhost:8080/fhir"
```

Then run:

```bash
# Generate test plan and resources
fhir-ig-testgen --generate

# Run tests against a FHIR server
fhir-ig-testgen

# Run tests against a built-in mock server (no real FHIR server needed)
fhir-ig-testgen --mock

# Mock server on a specific port
fhir-ig-testgen --mock --mock-port 8091

# Preview without executing
fhir-ig-testgen --dry-run

# Override specific fields from the CLI
fhir-ig-testgen --config other.toml
fhir-ig-testgen --package path/to/other.tgz
fhir-ig-testgen --output ./other-output
```

CLI flags override config values: `--package`, `--output`, `--results`, `--dry-run`, `--generate`, `--mock`, `--mock-port`.

### Validate a resource against a profile

```bash
fhir-ig-testgen validate --package path/to/ig-package.tgz --resource patient.json
# or with explicit profile URL:
fhir-ig-testgen validate --package path/to/ig-package.tgz --resource patient.json \
  --profile "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient"
```

## Test Coverage

For each resource type declared in a server-mode CapabilityStatement, `fhir-ig-testgen` generates:

### Test Plan (Functional Tests)

These tests exercise the interactions and parameters the CapabilityStatement declares the server supports.

| Test Kind | Description | Example |
|-----------|-------------|---------|
| **CRUD interactions** | One test per supported interaction | `read`, `create`, `update`, `delete`, `search-type` |
| **Single search params** | One test per declared search parameter | `?name=Smith`, `?birthdate=2024-01-01` |
| **Search modifiers** | Type-appropriate modifiers | `:exact`, `:contains` on strings; `:missing` on all; `:not` on tokens |
| **Search prefixes** | All 9 FHIR prefixes on date/number/quantity params | `?birthdate=gt2024-01-01`, `?birthdate=lt2024-01-01` |
| **Near searches** | Coordinate/distance searches for `special`-type params | `?near=-33.86:151.21:10:km` |
| **Combinatorial searches** | All 2-parameter combinations within a resource type | `?name=Smith&birthdate=2024-01-01` |
| **Chained searches** | Reference params chained into target params | `?subject.name=Smith` |
| **`_include` / `_revinclude`** | From the CS's declared `searchInclude` and `searchRevInclude` | `?_include=Patient:organization` |
| **Result parameters** | `_summary`, `_count`, `_sort` on every searchable resource | `?_summary=true`, `?_count=1`, `?_sort=name` |
| **`$operation`** | From both resource-level and system-level `rest.operation` | `$everything`, `$export` |
| **Negative tests** | Read nonexistent ID (404), search with invalid parameter | `/Patient/nonexistent-id-99999` |

### Conformance Tests (Responder Obligations)

These tests verify that the server actually meets the obligations it declares in its CapabilityStatement. They are **IG-agnostic** — driven entirely by whatever CapabilityStatement and StructureDefinitions are in the package, not hardcoded for any specific IG.

| Test Category | What It Checks | Example |
|---------------|----------------|---------|
| **CapabilityStatement validation** | CS has required fields (`status`, server-mode `rest`, resource `type`, search param `name`/`type`) | CS missing `status` → error; resource entry without `type` → error |
| **MustSupport field presence** | Fields marked `mustSupport=true` in declared profiles appear in search responses | `Patient.name` is mustSupport → `GET /Patient?_count=10` must return entries containing `name` |
| **Cardinality enforcement** | `min` and `max` constraints from profile ElementDefinitions are respected | `Patient.name` has min=1 → responses must include `name`; `Patient.birthDate` has max=1 → responses must not have multiple `birthDate` |
| **Undeclared interaction rejection** | Interactions NOT in the CS are rejected (non-2xx expected) | CS declares only `read` and `search-type` for Patient → `POST /Patient` (create) must be rejected |
| **Undeclared search param handling** | Search parameters NOT in the CS are either rejected OR ignored by the server | `GET /Patient?__invalid_conformance_test__=value` may return `4xx/5xx` or `200 Bundle` |

**How profile matching works:**

1. If the CS references a `profile` URL, use that StructureDefinition
2. If the CS references `supportedProfile` URLs, use those
3. Otherwise, fall back to any StructureDefinition whose `base_type` matches the resource type

**Negative conformance tests** use `expected_status: 0` as a sentinel meaning "expect non-2xx." For undeclared search parameters, the harness also accepts `200` with a `Bundle` (server ignored unknown param), which is allowed by FHIR behavior.

### Response Assertions

Every test case carries a `ResponseAssertion` that validates the server response beyond HTTP status codes:

- **Bundle type**: search tests verify `Bundle.type == "searchset"`
- **Entry count**: `_count` tests verify entries ≤ requested count
- **Resource types**: included resources match expected types
- **Field values**: response fields match auto-generated sentinel values
- **Required field presence**: mustSupport fields must exist in responses (regardless of value)
- **Include types**: `_include`/`_revinclude` results contain declared target types
- **Sort order**: `_sort` results are ordered by the specified field
- **Absent fields**: `_summary=true` strips `text` div
- **OperationOutcome**: negative tests verify severity `"error"`

### What A Passing Run Means (And Does Not Mean)

A full pass means the server satisfied the rules in this harness for this dataset and CapabilityStatement.
It does **not** prove complete FHIR conformance in the formal certification sense.

Known limitations of strictness:

- **Undeclared search params are permissive**: `200 + Bundle` can pass (treated as "ignored unknown param").
- **Some conformance checks allow empty search results**: MustSupport/cardinality checks use `min_entries = 0`, so they validate entries when present but do not fail solely for no matches.
- **Include/revinclude checks are type-presence focused**: they verify expected included types appear, not full join provenance for every primary hit.
- **Semantic search checks are existential**: they require at least one matching entry/path, not universal match across every returned entry.

Use this tool as high-signal interoperability testing, and complement it with stricter profile validation and certification-aligned test suites when required.

At runtime, the orchestrator resolves sentinel search values (e.g. `Patient/test-id` → `Patient/actual-id`, `?name=test-value` → `?name=GeneratedFamily`) using field values extracted from generated resources.

## Test Data Generation

The resource generator walks each profile's snapshot elements and:

- **Required fields** (min > 0): generates appropriate values based on the FHIR type
- **Fixed/pattern values**: uses the exact values specified by the profile
- **Reference types**: creates `placeholder:ResourceType` references, resolved to actual IDs at test time
- **Optional fields** (min = 0): omitted to keep resources minimal

Supported FHIR types for generation:

| Type | Generated Value |
|------|-----------------|
| string, code, uri, url, canonical | Sentinel string |
| boolean | `true` |
| integer, positiveInt, unsignedInt | `1` |
| date, dateTime, instant | `2024-01-01` |
| Identifier | `{ "system": "...", "value": "..." }` |
| HumanName | `{ "family": "...", "given": ["..."] }` |
| Address | `{ "line": [...], "city": "...", ... }` |
| CodeableConcept | `{ "coding": [{ "system": "...", "code": "..." }] }` |
| Reference | `{ "reference": "placeholder:ResourceType" }` |

## Dependency Resolution

Resources that reference other resources are created in topological order. For example, an Observation that references a Patient will create the Patient first, then substitute the placeholder reference with the actual ID returned by the server.

Circular dependencies are handled gracefully — resources in a cycle (e.g. Organization ↔ Endpoint) are created in an arbitrary order within the cycle, with one direction using a placeholder reference.

You can override the auto-resolved creation order in `config.toml`.

## Profile Validation

Responses are validated against the IG's StructureDefinitions:

- **resourceType** must match the profile's base type
- **Required elements** (min > 0) must be present
- **Fixed values** must match exactly
- **Pattern values** must match

## Architecture

```
┌──────────────┐     ┌─────────────────┐     ┌──────────────────┐
│  IG Package  │────>│  Parse & Model   │────>│  Generate Tests   │
│  (.tgz)      │     │  (Capability,   │     │  (test plan,      │
│              │     │   Profiles)      │     │   resources)      │
└──────────────┘     └─────────────────┘     └────────┬───────────┘
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
                              ┌───────────────────┐       │
                              │ Response Assertion │───────┤
                              │ (Bundle, fields,  │       │
                              │  includes, sort)  │       │
                              └───────────────────┘       │
                                                         ▼
                                                  ┌──────────────┐
                                                  │  FHIR Server │
                                                  └──────────────┘
```

## Test Results

After every test run, results are written to `{output}/results/`:

```
output/results/
├── summary.json              # Overall totals and per-group breakdowns
├── Patient.json              # Full results for Patient test group
├── Observation.json          # Full results for Observation test group
├── _conformance.json         # Full results for conformance tests
└── ...                       # One file per resource type + conformance
```

**`summary.json`** contains:

| Field | Description |
|-------|-------------|
| `total` | Total test count |
| `passed` | Number of passed tests |
| `failed` | Number of failed tests |
| `groups` | Per-group breakdown (group name, total, passed, failed) |

**Per-group files** contain the full `TestResult` array with request details, response bodies, status codes, and validation errors for every test in that group.

## Configuration

See [`config.toml`](config.toml) for a complete example with comments.

Key settings:

| Setting | Description | Default |
|---------|-------------|---------|
| `package` | Path to the IG package (.tgz) | Required (or `--package` flag) |
| `output` | Directory for generated test plan, resources, and results | `./output` |
| `dry_run` | Print all test URLs without executing | `false` |
| `mock` | Use built-in mock FHIR server | `false` |
| `mock_port` | Port for mock server (0 = random) | `0` |
| `server.base_url` | Public FHIR server URL (for GET/search queries) | Required |
| `server.headers` | HTTP headers for the public server (auth tokens) | None |
| `repository.base_url` | Internal repository URL (for resource upload/delete) | None |
| `repository.username` | Basic auth username for the repository | None |
| `repository.password` | Basic auth password for the repository | None |
| `repository.upload_method` | HTTP method for uploading resources: `PUT` or `POST` | `PUT` |
| `repository.concurrency` | Parallel requests for upload/delete (1 = sequential) | `1` |
| `overrides.capability_statement_file` | Path to responder CapabilityStatement JSON to replace package-selected CS | None |
| `overrides.creation_order` | Manual resource creation order | Auto-resolved |
| `overrides.fixtures_dir` | Directory for fixture JSON files | None |
| `overrides.fixture_map` | Map resource type → fixture filename | None |
| `data_generation.counts` | Bulk data counts per resource type (e.g. Organization = 20000) | None |
| `data_generation.generate_only` | Generate NDJSON files but skip upload/delete | `false` |

### Repository vs Server

When `repository` is configured, resource upload and delete operations go to the
repository endpoint with basic auth, while read/search queries go to the public
`server` endpoint. This matches production setups where the public FHIR API is
read-only and data must be loaded through an internal service.

In development, leave `repository` commented out — all requests go to `server`.

### Upload Method

The `repository.upload_method` setting controls how resources are created:

- **`PUT`** (default): Uses `PUT /{ResourceType}/{id}` with client-assigned IDs
  (FHIR "update as create" pattern). The resource body must include an `id` field.
  This is the most common pattern for bulk data loading.

- **`POST`**: Uses `POST /{ResourceType}` with server-assigned IDs. The `id` field
  is removed from the resource body before sending.

```toml
[repository]
base_url = "http://repo.internal:8080/fhir"
username = "admin"
password = "admin123"
upload_method = "PUT"   # or "POST"
concurrency = 1        # parallel requests for upload/delete
```

### Bulk Data Generation

When `data_generation.counts` is configured, the tool generates realistic FHIR
resources in NDJSON format (one file per resource type under `{output}/data/`),
bulk-uploads them to the repository before tests, and bulk-deletes them afterward.

```toml
[data_generation]
counts.Organization = 20_000
counts.Practitioner = 100_000
counts.PractitionerRole = 300_000
counts.Location = 20_000
counts.HealthcareService = 100_000
```

Key features:

- **Cross-references**: PractitionerRole references Practitioner/Organization/Location/HealthcareService/Endpoint; HealthcareService references Organization/Location/Endpoint; Location references Organization/Endpoint.
- **Realistic data**: Names, addresses, NPIs, specialties, and coordinates are generated using the `fake` crate.
- **Coordinate coverage**: Locations are spread across 20 US cities with lat/lon jitter, enabling `near` search tests.
- **Dependency order**: Resources are created in dependency-safe order (Organization → Practitioner → Endpoint → Location → HealthcareService → PractitionerRole → Provenance) and deleted in reverse.
- **Revinclude seeding**: Provenance targets are seeded to cover `*-1` resources for Organization, Practitioner, Location, HealthcareService, and PractitionerRole, then randomized across remaining IDs.
- **Concurrent uploads**: 20 parallel requests during upload and deletion for throughput.

When `data_generation.counts` is set, the single-resource setup phase is skipped — only bulk data is used.

To generate NDJSON files without uploading (e.g. for manual upload or sending to a separate system):

```toml
[data_generation]
generate_only = true
counts.Organization = 20_000
```

The files are written to `{output}/data/{ResourceType}.ndjson` and left in place. No upload or deletion is performed.

### Mock Server

The built-in mock FHIR server handles CRUD operations and basic search filtering (string contains, token/code match, `_count`). It's useful for development, CI, and quick smoke tests where no real FHIR server is available.

Enable it via config or CLI:

```toml
# In config.toml (top-level, before [server])
mock = true
mock_port = 8091
```

```bash
# Or via CLI flags
fhir-ig-testgen --mock
fhir-ig-testgen --mock --mock-port 8091
```

When `mock` is enabled, the `[server]` and `[repository]` sections are ignored — all requests go to the mock server.

## Real-World Example

Testing against the HCPD (Health Connect Provider Directory) IG:

```bash
# Run against a real FHIR server
fhir-ig-testgen --config config.toml

# Example output:
# === FHIR IG Test Results ===
# Total: 339 | Passed: 287 | Failed: 52
# ---
# [PASS] patient_read (HTTP 200)
# [PASS] patient_search_name (HTTP 200)
# [FAIL] patient_search_birthdate_gt (HTTP 200)
#   - Bundle type mismatch: expected "searchset", got "collection"
# [PASS] patient_negative_read_nonexistent (HTTP 404)
# ...
```

## Mock Server Integration Test

The project includes a full integration test that spins up an in-process mock FHIR server using `axum` and runs the orchestrator end-to-end. This verifies:

- Resource creation (POST)
- Read/search tests (GET)
- Negative tests (404 for nonexistent resources)
- Response assertion validation
- Resource cleanup (DELETE)

```bash
cargo test run_against_mock_fhir_server
```

## License

MIT