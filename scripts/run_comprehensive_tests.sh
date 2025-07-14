#!/bin/bash

# Comprehensive test runner for BlockDB
# Runs all test suites and generates coverage reports

set -e

echo "🧪 BlockDB Comprehensive Test Suite"
echo "===================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${2}${1}${NC}"
}

print_status "Starting comprehensive test execution..." $BLUE

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    print_status "❌ Cargo not found. Please install Rust and Cargo." $RED
    exit 1
fi

# Set test environment variables
export RUST_LOG=info
export RUST_BACKTRACE=1

# Create test results directory
mkdir -p test_results
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
RESULTS_DIR="test_results/run_$TIMESTAMP"
mkdir -p "$RESULTS_DIR"

print_status "📁 Test results will be saved to: $RESULTS_DIR" $BLUE

# Function to run tests and capture results
run_test_suite() {
    local test_name=$1
    local test_command=$2
    local description=$3
    
    print_status "🔄 Running $description..." $YELLOW
    
    if eval $test_command > "$RESULTS_DIR/${test_name}.log" 2>&1; then
        print_status "✅ $description - PASSED" $GREEN
        echo "PASSED" > "$RESULTS_DIR/${test_name}.status"
    else
        print_status "❌ $description - FAILED" $RED
        echo "FAILED" > "$RESULTS_DIR/${test_name}.status"
        echo "Error details:"
        tail -20 "$RESULTS_DIR/${test_name}.log"
        return 1
    fi
}

# Test suite execution
FAILED_TESTS=0

# 1. Unit tests
print_status "\n📋 Running Unit Tests" $BLUE
if ! run_test_suite "unit_tests" "cargo test --lib" "Unit Tests"; then
    ((FAILED_TESTS++))
fi

# 2. Integration tests
print_status "\n📋 Running Integration Tests" $BLUE
if ! run_test_suite "integration_tests" "cargo test --test '*'" "Integration Tests"; then
    ((FAILED_TESTS++))
fi

# 3. Authentication integration tests
print_status "\n📋 Running Authentication Integration Tests" $BLUE
if ! run_test_suite "auth_integration" "cargo test --test auth_integration_test" "Authentication Integration Tests"; then
    ((FAILED_TESTS++))
fi

# 4. API authentication tests
print_status "\n📋 Running API Authentication Tests" $BLUE
if ! run_test_suite "api_auth_tests" "cargo test --test api_auth_test" "API Authentication Tests"; then
    ((FAILED_TESTS++))
fi

# 5. Storage comprehensive tests
print_status "\n📋 Running Storage Comprehensive Tests" $BLUE
if ! run_test_suite "storage_tests" "cargo test --test storage_comprehensive_test" "Storage Comprehensive Tests"; then
    ((FAILED_TESTS++))
fi

# 6. Consensus tests
print_status "\n📋 Running Consensus Tests" $BLUE
if ! run_test_suite "consensus_tests" "cargo test --test consensus_test" "Consensus Tests"; then
    ((FAILED_TESTS++))
fi

# 7. Property-based tests
print_status "\n📋 Running Property-Based Tests" $BLUE
if ! run_test_suite "property_tests" "cargo test --test property_based_test" "Property-Based Tests"; then
    ((FAILED_TESTS++))
fi

# 8. CLI tests
print_status "\n📋 Running CLI Tests" $BLUE
if ! run_test_suite "cli_tests" "cargo test --test cli_test" "CLI Tests"; then
    ((FAILED_TESTS++))
fi

# 9. Configuration and error tests
print_status "\n📋 Running Configuration and Error Tests" $BLUE
if ! run_test_suite "config_error_tests" "cargo test --test config_and_error_test" "Configuration and Error Tests"; then
    ((FAILED_TESTS++))
fi

# 10. Benchmarks (optional)
print_status "\n📋 Running Performance Benchmarks" $BLUE
if command -v cargo-criterion &> /dev/null || cargo install --list | grep -q criterion; then
    if ! run_test_suite "benchmarks" "cargo bench" "Performance Benchmarks"; then
        print_status "⚠️  Benchmarks failed but continuing..." $YELLOW
    fi
else
    print_status "⚠️  Criterion not installed, skipping benchmarks" $YELLOW
    echo "To install: cargo install cargo-criterion"
fi

# 11. Documentation tests
print_status "\n📋 Running Documentation Tests" $BLUE
if ! run_test_suite "doc_tests" "cargo test --doc" "Documentation Tests"; then
    ((FAILED_TESTS++))
fi

# Generate test summary
print_status "\n📊 Generating Test Summary" $BLUE

echo "# BlockDB Test Summary - $TIMESTAMP" > "$RESULTS_DIR/summary.md"
echo "" >> "$RESULTS_DIR/summary.md"
echo "## Test Results" >> "$RESULTS_DIR/summary.md"
echo "" >> "$RESULTS_DIR/summary.md"

TOTAL_TESTS=0
PASSED_TESTS=0

for status_file in "$RESULTS_DIR"/*.status; do
    if [ -f "$status_file" ]; then
        test_name=$(basename "$status_file" .status)
        status=$(cat "$status_file")
        ((TOTAL_TESTS++))
        
        if [ "$status" = "PASSED" ]; then
            echo "- ✅ $test_name: **PASSED**" >> "$RESULTS_DIR/summary.md"
            ((PASSED_TESTS++))
        else
            echo "- ❌ $test_name: **FAILED**" >> "$RESULTS_DIR/summary.md"
        fi
    fi
done

echo "" >> "$RESULTS_DIR/summary.md"
echo "## Summary Statistics" >> "$RESULTS_DIR/summary.md"
echo "" >> "$RESULTS_DIR/summary.md"
echo "- **Total Test Suites:** $TOTAL_TESTS" >> "$RESULTS_DIR/summary.md"
echo "- **Passed:** $PASSED_TESTS" >> "$RESULTS_DIR/summary.md"
echo "- **Failed:** $FAILED_TESTS" >> "$RESULTS_DIR/summary.md"

if [ $TOTAL_TESTS -gt 0 ]; then
    SUCCESS_RATE=$((PASSED_TESTS * 100 / TOTAL_TESTS))
    echo "- **Success Rate:** $SUCCESS_RATE%" >> "$RESULTS_DIR/summary.md"
fi

echo "" >> "$RESULTS_DIR/summary.md"
echo "## Test Execution Details" >> "$RESULTS_DIR/summary.md"
echo "" >> "$RESULTS_DIR/summary.md"
echo "- **Timestamp:** $TIMESTAMP" >> "$RESULTS_DIR/summary.md"
echo "- **Platform:** $(uname -s) $(uname -m)" >> "$RESULTS_DIR/summary.md"
echo "- **Rust Version:** $(rustc --version)" >> "$RESULTS_DIR/summary.md"
echo "- **Cargo Version:** $(cargo --version)" >> "$RESULTS_DIR/summary.md"

# Print final summary
print_status "\n📊 Test Execution Summary" $BLUE
print_status "=========================" $BLUE
print_status "Total Test Suites: $TOTAL_TESTS" $BLUE
print_status "Passed: $PASSED_TESTS" $GREEN
print_status "Failed: $FAILED_TESTS" $RED

if [ $TOTAL_TESTS -gt 0 ]; then
    SUCCESS_RATE=$((PASSED_TESTS * 100 / TOTAL_TESTS))
    print_status "Success Rate: $SUCCESS_RATE%" $BLUE
fi

print_status "\n📄 Detailed results saved to: $RESULTS_DIR" $BLUE

# Generate coverage report if llvm-cov is available
if command -v cargo-llvm-cov &> /dev/null; then
    print_status "\n📈 Generating Code Coverage Report" $BLUE
    cargo llvm-cov --html --output-dir "$RESULTS_DIR/coverage" > "$RESULTS_DIR/coverage.log" 2>&1 || {
        print_status "⚠️  Coverage report generation failed" $YELLOW
    }
else
    print_status "\n⚠️  cargo-llvm-cov not installed, skipping coverage report" $YELLOW
    echo "To install: cargo install cargo-llvm-cov"
fi

# Exit with error if any tests failed
if [ $FAILED_TESTS -gt 0 ]; then
    print_status "\n❌ Some tests failed. Check the logs for details." $RED
    exit 1
else
    print_status "\n🎉 All tests passed successfully!" $GREEN
    exit 0
fi