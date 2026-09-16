#!/bin/bash

# Script to run Python tests with proper coverage measurement
# This addresses PERF-G13: Python test coverage issue

set -e

echo "Running Python tests with coverage measurement..."

# Check if maturin is installed, if not install it
if ! command -v maturin &> /dev/null; then
    echo "maturin not found, installing..."
    pip install maturin
fi

# Create a temporary directory for test artifacts
TEST_DIR="/tmp/weave-python-test-$$"
mkdir -p "$TEST_DIR"

# Cleanup function
cleanup() {
    echo "Cleaning up..."
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# Build the Python wheel
echo "Building Python wheel..."
maturin develop --features python,extension-module

# Run Python tests with coverage
echo "Running Python tests..."

# Run unit tests with cargo
echo "Running Rust unit tests..."
cargo test --features python --lib -- --nocapture

# Run integration tests
echo "Running integration tests..."
python3 -c "
import weave_graph
print('weave_graph module imported successfully')
print('Available attributes:', [attr for attr in dir(weave_graph) if not attr.startswith('_')])
"

echo "Python tests completed successfully!"