# fhir-autotest-fuzz

**Context-aware FHIR REST API fuzzer** — mutation testing for FHIR servers.

Part of the [fhir-autotest](https://github.com/jlcoulter/fhir-autotest) project. Given a FHIR Implementation Guide `.tgz` package, this tool generates valid resources from the IG's own StructureDefinitions, then applies type-aware mutations to probe the server for vulnerabilities, misconfigurations, and information leaks.

## Quick Start

```bash
# Fuzz against the built-in mock server (no real FHIR server needed)
cargo run -p fhir-autotest-fuzz -- --package ./package.tgz --mock

# Fuzz against a real FHIR server
cargo run -p fhir-autotest-fuzz -- --package ./package.tgz --target http://localhost:8080/fhir

# Use existing config.toml (reads package, server.base_url, mock, output, dry_run)
cargo run -p fhir-autotest-fuzz

# Polite fuzzing with delay and fewer iterations
cargo run -p fhir-autotest-fuzz -- --mock --iterations 50 --delay-ms 200
```

## What It Tests

The fuzzer runs in three phases, each applying multiple mutation strategies:

### Phase 1: POST (Create)
Sends mutated resource bodies to `POST /{ResourceType}`.

| Mutator | What it does |
|---------|-------------|
| **boundary** | Empty strings, very long strings, infinity/NaN, negative zero, max int, null bytes, emoji overflow |
| **type_mismatch** | String where number expected, array where object expected, null for required fields, nested objects where primitives expected |
| **cardinality** | Removes required (min>0) fields, duplicates max=1 fields, adds unexpected fields, strips all optional fields |
| **encoding** | JSON injection (`"; {} //`), deeply nested objects (stack overflow), duplicate unicode-normalized keys, very long field names |

### Phase 2: PUT (Update)
Same mutations as Phase 1, but sent as `PUT /{ResourceType}/{id}` with a synthetic ID.

### Phase 3: GET (Search Parameter Fuzzing)
Sends fuzzed query parameter values to `GET /{ResourceType}?{param}={value}`.

Driven by the CapabilityStatement's declared search parameters. Each param type gets type-appropriate fuzz values:

| Param Type | Fuzz Examples |
|-----------|--------------|
| **string** | Empty, 10K chars, null bytes, SQL injection, XSS, path traversal, unicode |
| **token** | `\|`, `system\|code`, null bytes, `OR 1=1\|`, XSS |
| **date** | `0000-00-00`, `9999-12-31`, invalid month/day, `gt`/`le` prefixes |
| **number** | `0`, `-0`, `NaN`, `Infinity`, `1e9999`, `gt100` |
| **quantity** | `0`, `1000\|...\|mg`, `\|\|`, `gt100\|...\|mg` |
| **uri** | `../../etc/passwd`, `javascript:alert(1)`, `file:///etc/passwd` |
| **reference** | `Patient/`, `../../etc/passwd`, `Patient/OR 1=1--` |
| **composite** | `$`, `value1$value2`, `$$`, 10K-char values |
| **special** | `near`, `-33.86\|151.21\|10\|km`, `near\|NaN\|NaN\|NaN\|km` |

## Anomaly Detection

Every response is classified for anomalies:

| Signal | What it means |
|--------|--------------|
| **HTTP 5xx** | Server crashed or threw an unhandled exception |
| **HTTP 200/201** | Server accepted clearly invalid data (weak validation) |
| **Connection failure** | Server crashed or became unresponsive |
| **Information leak** | Response body contains stack traces, SQL errors, path disclosures, or HTML error pages |

### Leak Detection Patterns (40+ signatures)

- **Stack traces**: Java, .NET, Python, Go, Rust
- **SQL errors**: MySQL, PostgreSQL, SQLite, generic
- **Path disclosure**: `/etc/`, `/var/www/`, `C:\Users\`, `C:\inetpub\`
- **HTML error pages**: 500/502/503/504, Internal Server Error, Runtime Error
- **Framework leaks**: Hibernate, Spring, PHP errors, Node.js errors, Ruby errors
- **Parsing errors**: XML, JSON, null pointer exceptions

## CLI Reference

```
Usage: fhir-autotest-fuzz [OPTIONS]

Options:
  -p, --package <PACKAGE>      IG package (.tgz). Overrides config's `package`
  -t, --target <TARGET>        FHIR server URL. Overrides config's `server.base_url`
  -c, --config <CONFIG>        Config file path [default: ./config.toml]
  -o, --output <OUTPUT>        Output directory [default: ./fuzz-output]
  -i, --iterations <ITERATIONS> Iterations per resource/mutator [default: 100]
      --mutations <MUTATIONS>   Comma-separated: boundary,type_mismatch,cardinality,encoding,search_param [default: all]
      --seed <SEED>            Deterministic seed (0 = random) [default: 0]
      --concurrency <N>        Parallel requests [default: 4]
      --delay-ms <MS>          Delay between requests (polite mode) [default: 0]
      --dry-run                Print what would be sent without executing
      --mock                   Use built-in mock FHIR server
      --mock-port <PORT>       Mock server port [default: 0 = random]
  -h, --help                   Print help
  -V, --version                Print version
```

## Config File

The fuzzer reads the same `config.toml` format as `fhir-autotest`. CLI flags override config values:

```toml
package = "./package.tgz"
output = "./fuzz-output"

[server]
base_url = "http://localhost:8080/fhir"

# mock = true
# mock_port = 0
# dry_run = true
```

## Output

Results are written to `{output}/fuzz_report.json`:

```json
{
  "total": 12000,
  "anomalies": 3,
  "categories_used": ["boundary", "cardinality", "encoding", "search_param", "type_mismatch"],
  "anomaly_details": [
    {
      "mutator": "search_param",
      "resource_type": "Patient",
      "method": "GET",
      "url": "http://localhost:8080/fhir/Patient?name=%00%00%00%00",
      "status_code": 500,
      "reason": "Server error: HTTP 500",
      "response_snippet": "<html><head><title>500 Internal Server Error</title></head>..."
    }
  ]
}
```

## Extending with Custom Mutators

Implement the `Mutator` trait:

```rust
use fhir_autotest_fuzz::mutators::Mutator;
use fhir_autotest::model::profile::StructureDefinition;

pub struct MyCustomMutator;

impl Mutator for MyCustomMutator {
    fn name(&self) -> &'static str { "my_custom" }

    fn mutate(
        &self,
        base_resource: &serde_json::Value,
        profile: &StructureDefinition,
        seed: u64,
    ) -> serde_json::Value {
        // Your mutation logic here
        base_resource.clone()
    }
}
```

Then register it in `main.rs`:

```rust
fuzzer.register_mutator(Box::new(MyCustomMutator));
```

## Development

```bash
# Run all tests
cargo test --workspace

# Run just the fuzzer tests
cargo test -p fhir-autotest-fuzz

# Lint
cargo clippy -p fhir-autotest-fuzz --all-targets -- -D warnings
```
