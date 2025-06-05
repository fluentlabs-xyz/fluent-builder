# Default recipe - show available commands
default:
    @just --list

# Build the CLI in release mode
build-cli:
    cargo build --release -p fluent-compiler-cli

# Install CLI to cargo bin directory (makes it available system-wide)
install-cli: build-cli
    cargo install --path crates/cli

# Build and link CLI locally (creates symlink in current directory)
link-cli: build-cli
    #!/usr/bin/env bash
    set -euo pipefail
    
    # Remove old symlink if exists
    rm -f fluent-compiler
    
    # Create symlink to the built binary
    ln -s target/release/fluent-compiler fluent-compiler
    
    echo "✅ CLI linked as ./fluent-compiler"
    echo "Run with: ./fluent-compiler --help"

# Quick rebuild and link for development
dev: link-cli

# Build all crates
build-all:
    cargo build --release --all

# Run tests
test:
    cargo test --all

# Clean build artifacts
clean:
    cargo clean
    rm -f fluent-compiler

# Format code
fmt:
    cargo fmt --all

# Run clippy
clippy:
    cargo clippy --all --all-features -- -D warnings

# Compile example with default settings (saves all artifacts)
compile-example name: link-cli
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "📦 Compiling example: {{name}}"
    
    ./fluent-compiler examples/{{name}} -v
    
    echo ""
    echo "📁 Artifacts location: examples/{{name}}/out/{{name}}.wasm/"
    ls -la examples/{{name}}/out/{{name}}.wasm/

# Compile example with archive
compile-example-archive name: link-cli
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "📦 Compiling example with archive: {{name}}"
    
    ./fluent-compiler examples/{{name}} --archive -v
    
    echo ""
    echo "📁 Artifacts with archive:"
    ls -la examples/{{name}}/out/{{name}}.wasm/

# Test JSON output mode
test-json-output name="power-calculator": link-cli
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "🧪 Testing JSON output for: {{name}}"
    echo ""
    
    ./fluent-compiler examples/{{name}} --json-only | jq '.'

# Test compact JSON output
test-compact-json name="power-calculator": link-cli
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "🧪 Testing compact JSON output for: {{name}}"
    echo ""
    
    ./fluent-compiler examples/{{name}} --json-only --compact

# Extract just the rWASM bytecode
extract-rwasm name="power-calculator": link-cli
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "🔍 Extracting rWASM bytecode for: {{name}}"
    echo ""
    
    ./fluent-compiler examples/{{name}} --json-only -q | jq -r '.rwasm_bytecode_hex'

# Check if all dependencies are installed
check-deps:
    #!/usr/bin/env bash
    echo "Checking dependencies..."
    
    if ! command -v rustc &> /dev/null; then
        echo "❌ Rust not installed"
        exit 1
    else
        echo "✅ Rust: $(rustc --version)"
    fi
    
    if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
        echo "❌ wasm32-unknown-unknown target not installed"
        echo "   Run: rustup target add wasm32-unknown-unknown"
        exit 1
    else
        echo "✅ WASM target installed"
    fi
    
    if ! command -v jq &> /dev/null; then
        echo "⚠️  jq not installed (optional, for JSON processing)"
        echo "   Install with: brew install jq (macOS) or apt-get install jq (Linux)"
    else
        echo "✅ jq installed"
    fi

# Development workflow - rebuild and test
dev-test: link-cli
    @echo "🔨 Testing default mode..."
    @just compile-example power-calculator
    @echo ""
    @echo "🔨 Testing JSON output..."
    @just test-json-output power-calculator

# Show CLI help
help: link-cli
    @./fluent-compiler --help

# Run all tests
test-all: test dev-test
    @echo "✅ All tests passed!"

# CI simulation - how it would be used in CI/CD
ci-test name="power-calculator": link-cli
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "🤖 CI Pipeline simulation for: {{name}}"
    echo ""
    
    # Compile and validate
    if ./fluent-compiler examples/{{name}} --json-only -q > /tmp/output.json; then
        echo "✅ Compilation successful"
        
        # Extract values
        CONTRACT_NAME=$(jq -r '.contract_name' /tmp/output.json)
        RWASM_SIZE=$(jq -r '.rwasm_bytecode_hex' /tmp/output.json | wc -c)
        
        echo "📊 Contract: $CONTRACT_NAME"
        echo "📊 rWASM size: $((RWASM_SIZE / 2)) bytes"
    else
        echo "❌ Compilation failed"
        exit 1
    fi

# Publish crates to crates.io (dry run)
publish-dry:
    cargo publish -p fluent-compiler --dry-run
    cargo publish -p fluent-compiler-cli --dry-run

# Publish crates to crates.io (actual)
publish:
    cargo publish -p fluent-compiler
    @echo "Waiting for fluent-compiler to be available on crates.io..."
    @sleep 30
    cargo publish -p fluent-compiler-cli