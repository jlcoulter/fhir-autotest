# fhir-ig-testgen

A Rust CLI tool that parses FHIR R4 Implementation Guide packages (`.tgz`) and automatically generates conformance tests with synthetic test data.

Given an IG package, it:
1. Parses the CapabilityStatement to discover supported resources and interactions
2. Parses StructureDefinitions (profiles) to understand required fields and constraints
3. Generates synthetic FHIR resources that satisfy profile constraints
4. Resolves resource dependencies via topological sort (Patient before Observation, etc.)
5. Creates a test plan with HTTP requests for each supported interaction
6. Optionally runs tests against a FHIR server and validates responses against profiles

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

Create a `config.toml`:

```toml
[server]
base_url = "http://localhost:8080/fhir"

[server.headers]
Authorization = "Bearer token123"

[overrides]
creation_order = ["Patient", "Observation"]  # optional manual override
fixtures_dir = "./fixtures"                   # optional fixture directory

[overrides.fixture_map]
Patient = "my-patient.json"  # use fixture instead of generating
```

Then run:

```bash
fhir-ig-testgen run --package path/to/ig-package.tgz --config config.toml
```

### Validate a resource against a profile

```bash
fhir-ig-testgen validate --package path/to/ig-package.tgz --resource patient.json
# or with explicit profile URL:
fhir-ig-testgen validate --package path/to/ig-package.tgz --resource patient.json --profile "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient"
```

## Test Data Generation

The resource generator walks each profile's snapshot elements and:

- **Required fields** (min > 0): generates appropriate values based on the FHIR type
- **Fixed/pattern values**: uses the exact values specified by the profile
- **Reference types**: creates `placeholder:ResourceType` references, resolved to actual IDs at test time
- **Optional fields** (min = 0): omitted to keep resources minimal

Supported FHIR types for generation:

| Type | Generated Value |
|------|----------------|
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

Circular dependencies are detected and reported as errors.

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
│  IG Package  │────>│  Parse & Model   │────>│  Generate Tests  │
│  (.tgz)      │     │  (Capability,   │     │  (test plan,     │
│              │     │   Profiles)      │     │   resources)     │
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

## License

MIT