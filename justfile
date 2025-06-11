# Default recipe - show available commands
default:
    @just --list

# Build the CLI
build:
    cargo build --release -p fluent-compiler-cli

# Link CLI for local use
link: build
    @rm -f fluent-compiler
    @ln -s target/release/fluent-compiler fluent-compiler

# Install CLI globally
install:
    cargo install --path crates/cli

# Build all crates
build-all:
    cargo build --all

# Run all tests
test:
    cargo test --all

# Format code
fmt:
    cargo fmt --all

# Run clippy
clippy:
    cargo clippy --all -- -D warnings

# Clean build artifacts
clean:
    cargo clean
    rm -f fluent-compiler
    find examples -type d -name "out" -exec rm -rf {} + 2>/dev/null || true
    find examples -type d -name "target" -exec rm -rf {} + 2>/dev/null || true

# Development workflow - format, build, test
dev: fmt build test

# Compile example contract
compile contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}}

# Compile with source archive
compile-archive contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}} --archive

# Compile with JSON output
compile-json contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}} --json

# Deploy contract (requires gblend and DEPLOY_PRIVATE_KEY)
deploy contract="power-calculator" rpc="https://rpc.dev.gblend.xyz":
    cd examples/{{contract}} && \
    gblend deploy \
        --private-key $DEPLOY_PRIVATE_KEY \
        --rpc {{rpc}} \
        out/{{contract}}.wasm/lib.wasm \
        --gas-limit 30000000

# Verify contract - all parameters required
verify contract address chain_id rpc: link
    ./fluent-compiler verify examples/{{contract}} \
        --address {{address}} \
        --chain-id {{chain_id}} \
        --rpc {{rpc}}

# Complete workflow: compile -> deploy -> verify
deploy-and-verify contract="power-calculator": link
    #!/usr/bin/env bash
    set -euo pipefail
    
    # Default values
    CHAIN_ID="${CHAIN_ID:-20993}"
    RPC_URL="${RPC_URL:-https://rpc.dev.gblend.xyz}"
    
    # Check if DEPLOY_PRIVATE_KEY is set
    if [ -z "${DEPLOY_PRIVATE_KEY:-}" ]; then
        echo "❌ Error: DEPLOY_PRIVATE_KEY environment variable is not set"
        echo "Please export it: export DEPLOY_PRIVATE_KEY=0x..."
        exit 1
    fi
    
    # Compile with archive
    echo "📦 Compiling with source archive..."
    ./fluent-compiler compile examples/{{contract}} --archive
    
    # Deploy - gblend needs to be run from the contract directory
    echo "🚀 Deploying to chain $CHAIN_ID via $RPC_URL..."
    
    DEPLOY_OUTPUT=$(gblend deploy \
        --private-key $DEPLOY_PRIVATE_KEY \
        --chain-id $CHAIN_ID \
        --gas-limit 30000000 \
        --rpc $RPC_URL \
       examples/{{contract}}/out/{{contract}}.wasm/lib.wasm 2>&1)
    
    echo "$DEPLOY_OUTPUT"
    
    # Extract contract address from gblend output
    CONTRACT_ADDRESS=$(echo "$DEPLOY_OUTPUT" | grep "Contract address:" | sed 's/.*Contract address: //')
    
    if [ -z "$CONTRACT_ADDRESS" ]; then
        echo "❌ Failed to extract contract address from deployment output"
        exit 1
    fi
    
    echo "✅ Contract deployed at: $CONTRACT_ADDRESS"
    
    # Small delay to ensure contract is available
    sleep 2
    
    # Verify
    echo "🔍 Verifying contract at $CONTRACT_ADDRESS..."
    ./fluent-compiler verify examples/{{contract}} \
        --address "$CONTRACT_ADDRESS" \
        --chain-id "$CHAIN_ID" \
        --rpc "$RPC_URL"
    
    echo "✅ Done! Contract verified at $CONTRACT_ADDRESS"

# Deploy with gblend's --dev flag
deploy-dev contract="power-calculator": link
    #!/usr/bin/env bash
    set -euo pipefail
    
    if [ -z "${DEPLOY_PRIVATE_KEY:-}" ]; then
        echo "❌ Error: DEPLOY_PRIVATE_KEY environment variable is not set"
        exit 1
    fi
    
    # Compile
    echo "📦 Compiling..."
    ./fluent-compiler compile examples/{{contract}}
    
    # Deploy with --dev flag
    echo "🚀 Deploying with --dev..."
    cd examples/{{contract}}
    
    DEPLOY_OUTPUT=$(gblend deploy \
        --private-key "$DEPLOY_PRIVATE_KEY" \
        --dev \
        out/{{contract}}.wasm/lib.wasm \
        --gas-limit 30000000 2>&1)
    
    echo "$DEPLOY_OUTPUT"
    
    CONTRACT_ADDRESS=$(echo "$DEPLOY_OUTPUT" | grep "📍 Contract address:" | grep -oE "0x[a-fA-F0-9]{40}" | head -1)
    
    if [ -z "$CONTRACT_ADDRESS" ]; then
        echo "❌ Failed to extract contract address"
        exit 1
    fi
    
    # Go back to project root
    cd ../..
    
    # Verify
    echo "🔍 Verifying contract at $CONTRACT_ADDRESS..."
    ./fluent-compiler verify examples/{{contract}} \
        --address "$CONTRACT_ADDRESS" \
        --chain-id 20993 \
        --rpc https://rpc.dev.gblend.xyz
    
    echo "✅ Done!"

# Quick test with local node
test-local: link
    CHAIN_ID=1337 RPC_URL="http://localhost:8545" just deploy-and-verify power-calculator

# Quick test with dev network  
test-dev: link
    just deploy-and-verify power-calculator

# Debug deployment issues
debug-deploy contract="power-calculator": link
    #!/usr/bin/env bash
    echo "🔍 Checking setup..."
    
    # Check DEPLOY_PRIVATE_KEY
    if [ -z "${DEPLOY_PRIVATE_KEY:-}" ]; then
        echo "❌ DEPLOY_PRIVATE_KEY not set"
    else
        echo "✅ DEPLOY_PRIVATE_KEY is set (length: ${#DEPLOY_PRIVATE_KEY})"
    fi
    
    # Check gblend
    if command -v gblend &> /dev/null; then
        echo "✅ gblend found at: $(which gblend)"
        gblend --version || true
    else
        echo "❌ gblend not found"
    fi
    
    # Check compiled files
    echo ""
    echo "📁 Checking compiled files..."
    if [ -d "examples/{{contract}}/out" ]; then
        echo "✅ Output directory exists"
        ls -la examples/{{contract}}/out/{{contract}}.wasm/ 2>/dev/null || echo "❌ No {{contract}}.wasm directory"
    else
        echo "❌ No output directory found"
    fi
    
    # Test compilation
    echo ""
    echo "🔨 Testing compilation..."
    ./fluent-compiler compile examples/{{contract}}
    
    # Show exact path that will be used
    echo ""
    echo "📍 Deployment will use: examples/{{contract}}/out/{{contract}}.wasm/lib.wasm"
    ls -la examples/{{contract}}/out/{{contract}}.wasm/lib.wasm 2>/dev/null || echo "❌ File not found!"