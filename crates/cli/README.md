# fluent-compiler-cli

Command-line interface for compiling Rust smart contracts to WASM/rWASM bytecode for the Fluent blockchain.

## Installation

```bash
cargo install fluent-compiler-cli
```

## Basic Usage

### Compile

```bash
# Compile current directory
fluent-compiler compile

# Compile specific project
fluent-compiler compile ./my-contract

# Custom output directory
fluent-compiler compile -o ./build

# Create source archive 
fluent-compiler compile --archive

# Output JSON (for CI/CD)
fluent-compiler compile --json

# Debug build
fluent-compiler compile --profile debug

# With features
fluent-compiler compile --features "feature1 feature2"
```

### Verify

```bash
# Verify deployed contract
fluent-compiler verify \
  --address 0x1234... \
  --chain-id 20993 \
  --rpc https://rpc.dev.gblend.xyz

# Verify with custom settings
fluent-compiler verify \
  --address 0x1234... \
  --chain-id 20993 \
  --rpc https://rpc.dev.gblend.xyz \
  --profile release \
  --features "production"

# JSON output
fluent-compiler verify --address 0x1234... --chain-id 20993 --rpc https://... --json
```

## Output Structure

When compiling, creates:

```
out/
└── my-contract.wasm/
    ├── lib.wasm         # WASM bytecode
    ├── lib.rwasm        # rWASM bytecode
    ├── abi.json         # Solidity ABI
    ├── interface.sol    # Solidity interface
    ├── metadata.json    # Build metadata
    └── sources.tar.gz   # Source archive (if --archive)
```

## Common Options

- `-v, --verbose` - Enable debug logging
- `-q, --quiet` - Suppress all output except errors
- `--profile <PROFILE>` - Build profile (debug/release)
- `--features <FEATURES>` - Space-separated list of features
- `--no-default-features` - Disable default features

## JSON Output

For CI/CD integration, use `--json` flag:

```bash
# Compile
fluent-compiler compile --json | jq -r '.data.rwasm_hash'

# Verify
fluent-compiler verify --json --address 0x... --chain-id 20993 --rpc https://...
```

Output format:

```json
{
  "status": "success",
  "command": "compile",
  "contract_name": "my-contract",
  "rwasm_hash": "0x1234...",
  "wasm_size": 12345,
  "rwasm_size": 10000,
  "has_abi": true
}
```

## Error Handling

Errors are output as JSON to stderr:

```json
{
  "status": "error",
  "error_type": "compilation_failed",
  "message": "Detailed error message"
}
```

## Requirements

- Rust toolchain with `wasm32-unknown-unknown` target
- Contract must have `fluentbase-sdk` dependency

## Examples

### Development Workflow

```bash
# Compile with debug info
fluent-compiler compile --profile debug

# Deploy contract (using other tools)
# ...

# Verify deployment
fluent-compiler verify \
  --address $CONTRACT_ADDRESS \
  --chain-id 20993 \
  --rpc https://rpc.dev.gblend.xyz
```

### CI/CD Pipeline

```bash
#!/bin/bash
# Compile and extract hash
HASH=$(fluent-compiler compile --json | jq -r '.data.rwasm_hash')

# Deploy and verify
if fluent-compiler verify --json \
  --address $DEPLOYED_ADDRESS \
  --chain-id $CHAIN_ID \
  --rpc $RPC_URL; then
  echo "Verification successful"
else
  echo "Verification failed"
  exit 1
fi
```
