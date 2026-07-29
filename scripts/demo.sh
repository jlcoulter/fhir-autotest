#!/bin/bash
# Demo script: generate tests from the HCPD IG package and show the results.
#
# Usage: ./scripts/demo.sh
# Prerequisites: cargo, IG package at package/package.tgz

set -eo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

echo "=== FHIR IG Test Generator Demo ==="
echo ""

echo "📦 Building fhir-autotest..."
cargo build --release 2>&1 | tail -1

echo ""
echo "📋 Generating test plan..."
cargo run --release -- generate --package package/package.tgz --output /tmp/fhir-demo-output 2>&1 | { grep "^Generated" || true; }

echo ""
echo "🔍 Dry-run (all test URLs without executing):"
cargo run --release -- run --package package/package.tgz --config config.example.toml --dry-run 2>&1 | head -30

echo ""
echo "🧪 Integration test (mock FHIR server):"
cargo test --test integration_test run_against_mock_fhir_server -- --nocapture 2>&1 | grep -E "(Total:|test result)" || true

echo ""
echo "✨ To run all 339 tests against a real FHIR server:"
echo "   1. cp config.example.toml config.toml"
echo "   2. Edit config.toml → set server.base_url to your FHIR server"
echo "   3. fhir-autotest run --package package/package.tgz --config config.toml"
echo ""
echo "   Save detailed results to JSON:"
echo "   fhir-autotest run --package package/package.tgz --config config.toml -o results.json"
echo ""
echo "   Preview all test URLs without executing:"
echo "   fhir-autotest run --package package/package.tgz --config config.toml --dry-run"
echo ""
echo "🎉 Demo complete!"