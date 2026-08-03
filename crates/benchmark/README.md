# fhir-autotest-bench

Performance and load testing for FHIR servers, built on top of [fhir-autotest](https://github.com/jlcoulter/fhir-autotest).

This crate reuses the same IG package parsing, test plan generation, and data setup pipeline as `fhir-autotest`, then runs configurable load tests against a FHIR server and produces detailed latency reports.

## Quick Start

```bash
# Run a quick benchmark against the built-in mock server
cargo run -p fhir-autotest-bench -- --mock

# Run against a real FHIR server (uses config.toml)
cargo run -p fhir-autotest-bench

# One-shot mode: run each test case exactly once
cargo run -p fhir-autotest-bench -- --duration 0
```

## Configuration

Benchmark settings live in the `[bench]` section of the project's `config.toml`. All fields have sensible defaults — only set what you need to override.

### `config.toml` — `[bench]` section

```toml
[bench]
# Number of concurrent virtual users
concurrency = 20

# Duration in seconds (0 = run all tests once)
duration_secs = 60

# Ramp-up time — gradually increase concurrency over this period
ramp_up_secs = 10

# Timeout per individual request
request_timeout_secs = 30

# Output directory for reports
output = "./bench-results"

# Skip data generation/upload (assume data already exists)
# skip_data_ensure = false

# Skip cleanup after benchmark
# skip_cleanup = false

# Filter to specific resource types (empty = all groups)
# filter_groups = ["Patient", "Observation"]

# Warm-up requests before recording
# warmup_requests = 10

# Path to an existing test_plan.json (skip generation)
# test_plan = "./output/test_plan.json"
```

### CLI Overrides

Every config field can be overridden via CLI flags:

```
-c, --concurrency <N>        Number of concurrent virtual users
-d, --duration <SECONDS>     Benchmark duration (0 = one-shot)
    --ramp-up <SECONDS>      Ramp-up time
-o, --output <DIR>           Output directory
    --filter <TYPE>...       Filter test groups (repeatable)
    --skip-data-ensure       Skip data generation/upload
    --skip-cleanup           Skip resource cleanup
    --test-plan <PATH>       Use existing test_plan.json
    --warmup <N>             Warm-up requests
    --mock                   Use mock FHIR server
    --mock-port <PORT>       Mock server port
```

## How It Works

1. **Config loading** — loads `config.toml` (includes `[bench]` section)
2. **Mock server** (optional) — starts an in-process mock FHIR server
3. **Data ensure** — generates bulk test data and uploads it to the server
4. **Test plan** — loads or generates a test plan from the IG package
5. **Warm-up** — sends a configurable number of requests without recording
6. **Benchmark** — runs workers that continuously send random test requests
7. **Report** — writes summary, full results, text, and HTML reports
8. **Cleanup** — deletes uploaded resources from the server

## Report Output

Reports are written to the output directory (`./bench-results` by default):

| File | Format | Contents |
|------|--------|----------|
| `summary.json` | JSON | Overall stats + per-group breakdowns (no raw samples) |
| `full_results.json` | JSON | All raw samples with per-request latency |
| `report.txt` | Text | Human-readable summary |
| `report.html` | HTML | Styled report with tables |

### Latency Percentiles

Latency is measured per-request in microseconds and reported using HDR histograms:

- **p50** — median latency
- **p90** — 90th percentile
- **p95** — 95th percentile
- **p99** — 99th percentile
- **Min / Max / Mean** — basic statistics
- **Throughput** — requests per second

## Modes

### Duration Mode (default)

Workers continuously send random test requests for a fixed duration. Use for sustained load testing and throughput measurement.

```bash
cargo run -p fhir-autotest-bench -- --concurrency 50 --duration 120 --ramp-up 15
```

### One-Shot Mode (`--duration 0`)

Each test case in the plan is executed exactly once, distributed evenly across workers. Use for functional validation before running longer benchmarks.

```bash
cargo run -p fhir-autotest-bench -- --duration 0
```

### Mock Server Mode

Uses the built-in mock FHIR server — no real server needed. Useful for CI, development, and smoke tests.

```bash
cargo run -p fhir-autotest-bench -- --mock
```

## Graceful Shutdown

Press `Ctrl+C` during a benchmark to stop gracefully. All collected samples are preserved and written to the report. Cleanup still runs after shutdown.

## Development

```bash
# Run tests
cargo test -p fhir-autotest-bench

# Run with verbose logging
RUST_LOG=debug cargo run -p fhir-autotest-bench -- --mock
```
