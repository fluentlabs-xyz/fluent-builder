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

# Complete workflow: compile -> deploy -> verify
deploy-and-verify contract="power-calculator" network="dev": link
    #!/usr/bin/env bash
    set -euo pipefail
    
    # Compile with archive
    echo "📦 Compiling with source archive..."
    ./fluent-compiler compile examples/{{contract}} --archive
    
    # Deploy
    echo "🚀 Deploying..."
    DEPLOY_OUTPUT=$(gblend deploy \
        --private-key $DEPLOY_PRIVATE_KEY \
        --{{network}} \
        examples/{{contract}}/out/{{contract}}.wasm/lib.wasm \
        --gas-limit 30000000 2>&1)
    
    echo "$DEPLOY_OUTPUT"
    
    # Extract contract address
    CONTRACT_ADDRESS=$(echo "$DEPLOY_OUTPUT" | grep "Contract address:" | sed 's/.*Contract address: //')
    
    # Verify
    echo "🔍 Verifying..."
    ./fluent-compiler verify examples/{{contract}} \
        --address $CONTRACT_ADDRESS \
        --{{network}} \
        --export-abi examples/{{contract}}/verified.abi.json
    
    echo "✅ Done! Contract at $CONTRACT_ADDRESS"

# Quick commands
dev: 
    just deploy-and-verify power-calculator dev

local:
    just deploy-and-verify power-calculator local

# Individual commands
compile contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}}

# Compile with source archive
compile-archive contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}} --archive

# Compile for verification (with archive and specific output)
compile-verify contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}} \
        --archive \
        --output-dir examples/{{contract}}/verification-bundle

deploy contract="power-calculator" network="dev":
    gblend deploy \
        --private-key $DEPLOY_PRIVATE_KEY \
        --{{network}} \
        examples/{{contract}}/out/{{contract}}.wasm/lib.wasm \
        --gas-limit 30000000

verify contract address network="dev": link
    ./fluent-compiler verify examples/{{contract}} \
        --address {{address}} \
        --{{network}} \
        --export-abi examples/{{contract}}/verified.abi.json

# Run tests
test:
    cargo test --all

# Clean
clean:
    cargo clean
    rm -f fluent-compiler
    # Clean all generated artifacts
    find examples -type d -name "out" -exec rm -rf {} + 2>/dev/null || true
    find examples -type d -name "verification-bundle" -exec rm -rf {} + 2>/dev/null || true