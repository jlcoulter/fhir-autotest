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

### Generate a test plan and resources

```bash
fhir-ig-testgen generate --package path/to/ig-package.tgz --output ./output
```

This creates:

- `output/test_plan.json` — the full test plan with all test cases
- `output/resources/patient.json` — generated Patient resource
- `output/resources/observation.json` — generated Observation resource
- etc.

### Run tests against a FHIR server

Create a `config.toml` (see [config.example.toml](config.example.toml)):

```toml
[server]
base_url = "http://localhost:8080/fhir"

# Optional auth headers:
# [server.headers]
# Authorization = "Bearer your-token"
```

Then run:

```bash
fhir-ig-testgen run --package path/to/ig-package.tgz --config config.toml
```

This generates the test plan, creates setup resources on the server, runs all test cases, validates responses, and cleans up. Each request is printed as it executes:

```
── Setup: creating resources ──
  POST http://localhost:8080/fhir/Patient ... → Patient/abc-123
  POST http://localhost:8080/fhir/Observation ... → Observation/def-456

── Running 339 test cases ──

── Patient ──
  → GET /Patient/abc-123 [200]
  → GET /Patient?name=GeneratedFamily [200]
  ✗ GET /Patient/nonexistent-id-99999 [404]

── Cleanup: deleting resources ──
  DELETE Observation/def-456 ... → deleted
  DELETE Patient/abc-123 ... → deleted
```

#### Save results to JSON

Use `--output` to write detailed results (full URLs, request bodies, response bodies, validation errors) to a file:

```bash
fhir-ig-testgen run --package pkg.tgz --config config.toml -o results.json
```

Each entry in the JSON includes:
- `request_method`, `request_url` — the full HTTP request
- `request_body` — POST/PUT body (if any)
- `status_code` — HTTP response status
- `response_body` — full response JSON
- `passed` — whether the test passed
- `validation_errors` — list of assertion failures

#### Preview without executing

```bash
fhir-ig-testgen run --package pkg.tgz --config config.toml --dry-run
```

Shows all test URLs and setup/cleanup operations without hitting a server.

### Validate a resource against a profile

```bash
fhir-ig-testgen validate --package path/to/ig-package.tgz --resource patient.json
# or with explicit profile URL:
fhir-ig-testgen validate --package path/to/ig-package.tgz --resource patient.json \
  --profile "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient"
```

## Test Coverage

For each resource type declared in a server-mode CapabilityStatement, `fhir-ig-testgen` generates:

| Test Kind | Description | Example |
|-----------|-------------|---------|
| **CRUD interactions** | One test per supported interaction | `read`, `create`, `update`, `delete`, `search-type` |
| **Single search params** | One test per declared search parameter | `?name=Smith`, `?birthdate=2024-01-01` |
| **Search modifiers** | Type-appropriate modifiers | `:exact`, `:contains` on strings; `:missing` on all; `:not` on tokens |
| **Search prefixes** | All 9 FHIR prefixes on date/number/quantity params | `?birthdate=gt2024-01-01`, `?birthdate=lt2024-01-01` |
| **Combinatorial searches** | All 2-parameter combinations within a resource type | `?name=Smith&birthdate=2024-01-01` |
| **`_include` / `_revinclude`** | From the CS's declared `searchInclude` and `searchRevInclude` | `?_include=Patient:organization` |
| **Result parameters** | `_summary`, `_count`, `_sort` on every searchable resource | `?_summary=true`, `?_count=1`, `?_sort=name` |
| **`$operation`** | From both resource-level and system-level `rest.operation` | `$everything`, `$export` |
| **Negative tests** | Read nonexistent ID (404), search with invalid parameter | `/Patient/nonexistent-id-99999` |

### Response Assertions

Every test case carries a `ResponseAssertion` that validates the server response beyond HTTP status codes:

- **Bundle type**: search tests verify `Bundle.type == "searchset"`
- **Entry count**: `_count` tests verify entries ≤ requested count
- **Resource types**: included resources match expected types
- **Field values**: response fields match auto-generated sentinel values
- **Include types**: `_include`/`_revinclude` results contain declared target types
- **Sort order**: `_sort` results are ordered by the specified field
- **Absent fields**: `_summary=true` strips `text` div
- **OperationOutcome**: negative tests verify severity `"error"`

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

## Configuration

See [`config.example.toml`](config.example.toml) for a complete example with comments.

Key settings:

| Setting | Description | Default |
|---------|-------------|---------|
| `server.base_url` | FHIR server base URL | Required |
| `server.headers` | HTTP headers (auth tokens) | None |
| `overrides.creation_order` | Manual resource creation order | Auto-resolved |
| `overrides.fixtures_dir` | Directory for fixture JSON files | None |
| `overrides.fixture_map` | Map resource type → fixture filename | None |

## Real-World Example

Testing against the HCPD (Health Connect Provider Directory) IG:

```bash
# Generate from a real IG package
fhir-ig-testgen generate --package hcpd-package.tgz --output ./hcpd-tests

# Output:
# Generated test plan: 10 test groups, 339 total tests
# Generated 11 resource files

# Run against a FHIR server
fhir-ig-testgen run --package hcpd-package.tgz --config config.toml

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