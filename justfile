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

# Compile example contract (archive source by default)
compile contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}}

# Compile with Git source (requires clean repository)
compile-git contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}} --git-source

# Compile with explicit archive creation
compile-archive contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}} --archive

# Compile allowing uncommitted changes
compile-dirty contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}} --allow-dirty

# Compile with JSON output
compile-json contract="power-calculator": link
    ./fluent-compiler compile examples/{{contract}} --json

# Show Git status for contract
git-status contract="power-calculator":
    @cd examples/{{contract}} && git status --porcelain || echo "Not a git repository"

# Deploy contract (requires gblend and DEPLOY_PRIVATE_KEY)
deploy contract="power-calculator" rpc="https://rpc.dev.gblend.xyz":
    cd examples/{{contract}} && \
    gblend deploy \
        --private-key $DEPLOY_PRIVATE_KEY \
        --rpc {{rpc}} \
        out/{{contract}}.wasm/lib.wasm \
        --gas-limit 30000000

# Verify contract with archive source
verify contract address chain_id rpc: link
    ./fluent-compiler verify examples/{{contract}} \
        --address {{address}} \
        --chain-id {{chain_id}} \
        --rpc {{rpc}}

# Verify contract with Git source
verify-git contract address chain_id rpc: link
    ./fluent-compiler verify examples/{{contract}} \
        --address {{address}} \
        --chain-id {{chain_id}} \
        --rpc {{rpc}} \
        --git-source

# Complete workflow: compile -> deploy -> verify (archive source)
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
    
    # Compile (archive source by default)
    echo "📦 Compiling with archive source..."
    ./fluent-compiler compile examples/{{contract}}
    
    # Deploy
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

# Complete workflow with Git source
deploy-and-verify-git contract="power-calculator": link
    #!/usr/bin/env bash
    set -euo pipefail
    
    # Default values
    CHAIN_ID="${CHAIN_ID:-20993}"
    RPC_URL="${RPC_URL:-https://rpc.dev.gblend.xyz}"
    
    # Check if DEPLOY_PRIVATE_KEY is set
    if [ -z "${DEPLOY_PRIVATE_KEY:-}" ]; then
        echo "❌ Error: DEPLOY_PRIVATE_KEY environment variable is not set"
        exit 1
    fi
    
    # Check Git status
    echo "🔍 Checking Git status..."
    cd examples/{{contract}}
    if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
        echo "❌ Error: Repository has uncommitted changes"
        echo "Please commit or stash your changes before using Git source"
        exit 1
    fi
    cd ../..
    
    # Compile with Git source
    echo "📦 Compiling with Git source..."
    ./fluent-compiler compile examples/{{contract}} --git-source
    
    # Deploy
    echo "🚀 Deploying to chain $CHAIN_ID via $RPC_URL..."
    
    DEPLOY_OUTPUT=$(gblend deploy \
        --private-key $DEPLOY_PRIVATE_KEY \
        --chain-id $CHAIN_ID \
        --gas-limit 30000000 \
        --rpc $RPC_URL \
       examples/{{contract}}/out/{{contract}}.wasm/lib.wasm 2>&1)
    
    echo "$DEPLOY_OUTPUT"
    
    CONTRACT_ADDRESS=$(echo "$DEPLOY_OUTPUT" | grep "Contract address:" | sed 's/.*Contract address: //')
    
    if [ -z "$CONTRACT_ADDRESS" ]; then
        echo "❌ Failed to extract contract address"
        exit 1
    fi
    
    echo "✅ Contract deployed at: $CONTRACT_ADDRESS"
    sleep 2
    
    # Verify with Git source
    echo "🔍 Verifying contract with Git source..."
    ./fluent-compiler verify examples/{{contract}} \
        --address "$CONTRACT_ADDRESS" \
        --chain-id "$CHAIN_ID" \
        --rpc "$RPC_URL" \
        --git-source
    
    echo "✅ Done! Contract verified with Git source at $CONTRACT_ADDRESS"

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

# Show compilation metadata
show-metadata contract="power-calculator": link
    @echo "📋 Metadata for {{contract}}:"
    @cat examples/{{contract}}/out/{{contract}}.wasm/metadata.json 2>/dev/null | jq '.' || echo "No metadata found. Run 'just compile {{contract}}' first."

# Compare archive vs git compilation
compare-sources contract="power-calculator": link
    #!/usr/bin/env bash
    echo "🔍 Comparing Archive vs Git source compilation..."
    
    # Compile with archive
    echo ""
    echo "📦 Archive source:"
    ./fluent-compiler compile examples/{{contract}} -o out-archive
    ARCHIVE_HASH=$(cat examples/{{contract}}/out-archive/{{contract}}.wasm/metadata.json | jq -r '.bytecode.rwasm.hash' | cut -d: -f2)
    echo "   rWASM hash: $ARCHIVE_HASH"
    
    # Compile with git (if clean)
    echo ""
    echo "📦 Git source:"
    if ./fluent-compiler compile examples/{{contract}} --git-source -o out-git 2>/dev/null; then
        GIT_HASH=$(cat examples/{{contract}}/out-git/{{contract}}.wasm/metadata.json | jq -r '.bytecode.rwasm.hash' | cut -d: -f2)
        echo "   rWASM hash: $GIT_HASH"
        
        if [ "$ARCHIVE_HASH" = "$GIT_HASH" ]; then
            echo ""
            echo "✅ Hashes match! Both sources produce identical bytecode."
        else
            echo ""
            echo "❌ Hashes differ! This should not happen."
        fi
    else
        echo "   ⚠️  Cannot use Git source (repository dirty or not a git repo)"
    fi
    
    # Cleanup
    rm -rf examples/{{contract}}/out-archive examples/{{contract}}/out-git

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
    
    # Check Git status
    echo ""
    echo "📦 Git status:"
    cd examples/{{contract}} 2>/dev/null && git status --short || echo "   Not a git repository"
    cd - > /dev/null
    
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

# Show example commands
examples:
    @echo "📚 Example commands:"
    @echo ""
    @echo "  # Basic compilation (archive source)"
    @echo "  just compile power-calculator"
    @echo ""
    @echo "  # Compile with Git source (requires clean repo)"
    @echo "  just compile-git power-calculator"
    @echo ""
    @echo "  # Deploy and verify"
    @echo "  just deploy-and-verify power-calculator"
    @echo ""
    @echo "  # Deploy and verify with Git source"
    @echo "  just deploy-and-verify-git power-calculator"
    @echo ""
    @echo "  # Show metadata"
    @echo "  just show-metadata power-calculator"
    @echo ""
    @echo "  # Compare compilation methods"
    @echo "  just compare-sources power-calculator"