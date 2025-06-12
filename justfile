# --- Variables ---
default_contract := "power-calculator"

# --- Core Development Tasks ---

# Default recipe - show available commands
default:
    @just --list

# Build the CLI in release mode
build:
    cargo build --release -p fluent-builder-cli

# Link the release binary to the project root for easy local use
link: build
    @rm -f fluent-builder
    @ln -s target/release/fluent-builder fluent-builder

# Install the CLI globally
install:
    cargo install --path crates/cli

# Run all tests in the workspace
test:
    cargo test --all

# Format all code in the workspace
fmt:
    cargo fmt --all

# Run clippy linter on all crates
clippy:
    cargo clippy --all -- -D warnings

# Clean build artifacts and generated outputs
clean:
    cargo clean
    rm -f fluent-builder
    find examples -type d -name "out" -exec rm -rf {} + 2>/dev/null || true
    find examples -type d -name "target" -exec rm -rf {} + 2>/dev/null || true


# --- Compilation ---
# Compile a contract. Accepts an optional contract name and extra flags for the CLI.
# Usage:
#   just compile                              # Compile default contract
#   just compile my-contract                  # Compile a specific contract
#   just compile --allow-dirty                # Compile default with flags
#   just compile my-contract --allow-dirty    # Compile specific contract with flags
compile *args: link
    #!/usr/bin/env bash
    set -euo pipefail

    CONTRACT="{{default_contract}}"
    EXTRA_ARGS=() # Create an empty array for flags

    # Parse arguments: anything not starting with '-' is the contract name.
    # All other arguments are treated as flags.
    for arg in {{args}}; do
      if [[ "$arg" != -* ]]; then
        CONTRACT="$arg"
      else
        EXTRA_ARGS+=("$arg")
      fi
    done

    # The ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"} syntax safely handles an empty array with `set -u`
    echo "📦 Compiling '$CONTRACT' with flags: ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}"
    ./fluent-builder compile "examples/$CONTRACT" ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}

# --- Verification & Deployment ---

# Verify a deployed contract against local source code
# Usage: just verify-contract power-calculator 0x...
verify-contract contract address: link
    #!/usr/bin/env bash
    set -euo pipefail
    # Use environment variables or fallback to defaults
    CHAIN_ID="${CHAIN_ID:-20993}"
    RPC_URL="${RPC_URL:-https://rpc.dev.gblend.xyz}"
    
    echo "🔍 Verifying '{{contract}}' at address {{address}} on chain $CHAIN_ID..."
    ./fluent-builder verify examples/{{contract}} \
        --address {{address}} \
        --chain-id "$CHAIN_ID" \
        --rpc "$RPC_URL"

# Full workflow: compile, deploy, and verify an example contract.
# Requires `gblend` CLI and `DEPLOY_PRIVATE_KEY` environment variable.
deploy-and-verify contract=default_contract:
    #!/usr/bin/env bash
    set -euo pipefail
    
    if [ -z "${DEPLOY_PRIVATE_KEY:-}" ]; then
        echo "❌ Error: DEPLOY_PRIVATE_KEY environment variable is not set."
        exit 1
    fi
    
    # 1. Compile the contract (using --allow-dirty for easy local dev)
    just compile {{contract}} --allow-dirty
    
    # 2. Deploy the contract using gblend
    echo "🚀 Deploying '{{contract}}'..."
    DEPLOY_OUTPUT=$(gblend deploy \
        --private-key $DEPLOY_PRIVATE_KEY \
        --chain-id "${CHAIN_ID:-20993}" \
        --gas-limit 30000000 \
        --rpc "${RPC_URL:-https://rpc.dev.gblend.xyz}" \
        examples/{{contract}}/out/{{contract}}.wasm/lib.wasm 2>&1)
    
    echo "$DEPLOY_OUTPUT"
    
    CONTRACT_ADDRESS=$(echo "$DEPLOY_OUTPUT" | grep "Contract address:" | sed 's/.*Contract address: //')
    
    if [ -z "$CONTRACT_ADDRESS" ]; then
        echo "❌ Failed to extract contract address from deployment output."
        exit 1
    fi
    echo "✅ Contract deployed at: $CONTRACT_ADDRESS"
    
    # Small delay to ensure the transaction is indexed on-chain
    sleep 2
    
    # 3. Verify the deployed contract
    just verify-contract {{contract}} "$CONTRACT_ADDRESS"
    
    echo "🎉 Success! Contract '{{contract}}' was deployed and verified."


# --- Helpers ---

# Show the build metadata for a compiled contract
show-metadata contract=default_contract:
    @echo "📋 Metadata for '{{contract}}':"
    @cat examples/{{contract}}/out/{{contract}}.wasm/metadata.json 2>/dev/null | jq '.' || echo "No metadata found. Run 'just compile' first."

# Show common example commands
examples:
    @echo "📚 Example Commands:"
    @echo ""
    @echo "  # Build and link the CLI for local use"
    @echo "  just link"
    @echo ""
    @echo "  # Compile an example using Git source (default)"
    @echo "  just compile"
    @echo ""
    @echo "  # Compile an example, allowing uncommitted changes"
    @echo "  just compile --allow-dirty"
    @echo ""
    @echo "  # Full automated workflow (compile, deploy, verify)"
    @echo "  just deploy-and-verify"
    @echo ""
    @echo "  # Verify a specific, already-deployed contract"
    @echo "  just verify-contract power-calculator 0xYourContractAddress"