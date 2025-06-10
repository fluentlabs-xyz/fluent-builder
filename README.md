# `fluent-compiler`

A comprehensive Rust library and CLI for compiling and verifying smart contracts for the Fluent blockchain. It automates the entire workflow from source code compilation to on-chain verification, producing WASM/rWASM bytecode and all necessary artifacts.

## Overview

This project consists of two main components:

- **`fluent-compiler`** - Core library for contract compilation and verification
- **`fluent-compiler-cli`** - Command-line interface for easy interaction

## Key Features

### Compilation

- **Dual-Format Output:** Compiles Rust contracts to both standard WASM and execution-optimized rWASM format

- **Automated ABI Generation:** Extracts ABI from `#[router]` macro attributes in Rust source code
- **Complete Artifacts:** Generates Solidity-compatible ABI, interface files, and detailed build metadata
- **Source Archives:** Creates verifiable source archives (`.tar.gz` or `.zip`) for reproducible builds

### Verification

- **On-Chain Verification:** Verify deployed contracts by comparing source code against on-chain bytecode

- **Multiple Verification Methods:** Support for verification by contract address or bytecode hash
- **Network Support:** Built-in configurations for local, dev, and custom networks
- **ABI Export:** Extract and save ABI from successfully verified contracts

## Installation

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add WASM target
rustup target add wasm32-unknown-unknown
```

### Install CLI

```bash
# Clone the repository
git clone https://github.com/fluentlabs-xyz/fluent-compiler
cd fluent-compiler

# Install the CLI tool
just install-cli

# Or link for development
just link-cli
```

## CLI Usage

The CLI provides two main commands: `compile` and `verify`.

### Compile Command

Compile Rust smart contracts to WASM/rWASM:

```bash
# Compile current directory
fluent-compiler compile

# Compile specific project
fluent-compiler compile ./my-contract

# Output JSON only (for CI/CD)
fluent-compiler compile --json-only

# Create source archive for verification
fluent-compiler compile --archive

# Specify output directory
fluent-compiler compile -o ./build
```

### Verify Command

Verify deployed contracts against source code:

```bash
# Verify by contract address (fetches bytecode from chain)
fluent-compiler verify --address 0x1234...

# Verify by bytecode hash (if known)
fluent-compiler verify --deployed-hash 0xabcd...

# Verify on different networks
fluent-compiler verify --address 0x1234... --local     # localhost:8545
fluent-compiler verify --address 0x1234... --dev       # Fluent dev network
fluent-compiler verify --address 0x1234... --rpc https://rpc.example.com --chain-id 1

# Export ABI after successful verification
fluent-compiler verify --address 0x1234... --export-abi verified.abi.json

# Output in JSON format
fluent-compiler verify --address 0x1234... --format json
```

## Library Usage

### Basic Compilation

```rust
use fluent_compiler::{compile, CompileConfig};
use std::path::PathBuf;

// Configure compilation
let mut config = CompileConfig::default();
config.project_root = PathBuf::from("./my-contract");

// Compile the contract
let output = compile(&config)?;

// Access results
println!("Contract name: {}", output.result.contract_info.name);
println!("WASM size: {} bytes", output.result.outputs.wasm.len());
println!("rWASM size: {} bytes", output.result.outputs.rwasm.len());
```

### Contract Verification

```rust
use fluent_compiler::{verify_contract, VerifyConfigBuilder};

// Build verification config
let config = VerifyConfigBuilder::new()
    .project_root("./my-contract".into())
    .deployed_bytecode_hash("0x1234...".to_string())
    .with_metadata("0xcontract_address".to_string(), 1337) // chain_id
    .build()?;

// Run verification
let result = verify_contract(config)?;

if result.status.is_success() {
    println!("✅ Contract verified!");
    println!("Contract name: {}", result.contract_name);
} else {
    println!("❌ Verification failed: {:?}", result.details.error_message);
}
```

### Blockchain Integration

```rust
use fluent_compiler::blockchain::{NetworkConfig, ethers::fetch_bytecode_hash};

// Fetch deployed bytecode hash
let network = NetworkConfig::local(); // or ::fluent_dev() or ::custom(...)
let bytecode_hash = fetch_bytecode_hash(&network, "0xcontract_address").await?;

println!("Deployed bytecode hash: {}", bytecode_hash);
```

## Output Structure

When compiling with the default settings, the following directory structure is created:

```
out/
└── ContractName.wasm/
    ├── lib.wasm         # Standard WASM bytecode
    ├── lib.rwasm        # Fluent rWASM bytecode
    ├── abi.json         # Contract ABI
    ├── interface.sol    # Solidity interface
    ├── metadata.json    # Build metadata for verification
    └── sources.tar.gz   # Source archive (if --archive used)
```

## Development

The project uses `just` for task automation:

```bash
# Build everything
just build-all

# Run tests
just test

# Format code
just fmt

# Run clippy
just clippy

# Quick development cycle
just dev
```

### Examples

Compile and test the included example:

```bash
# Compile the power-calculator example
just compile-example power-calculator

# Test JSON output
just test-json-output power-calculator

# Test verification
just verify-by-hash power-calculator 0x123abc
```

## Configuration

### Compilation Config

```rust
use fluent_compiler::{CompileConfig, BuildProfile};

let config = CompileConfig {
    project_root: "./my-contract".into(),
    output_dir: "./build".into(),
    wasm: WasmConfig {
        profile: BuildProfile::Release,
        features: vec!["production".to_string()],
        no_default_features: true,
        ..Default::default()
    },
    artifacts: ArtifactsConfig {
        generate_abi: true,
        generate_interface: true,
        generate_metadata: true,
        pretty_json: true,
        ..Default::default()
    },
    ..Default::default()
};
```

### Network Configuration

```rust
use fluent_compiler::blockchain::NetworkConfig;

// Predefined networks
let local = NetworkConfig::local();      // localhost:8545, chain_id: 1337
let dev = NetworkConfig::fluent_dev();   // Fluent dev network

// Custom network
let custom = NetworkConfig::custom(
    "mainnet",                           // name
    "https://rpc.example.com",          // RPC URL
    1                                   // chain ID
);
```

## CI/CD Integration

### GitHub Actions Example

```yaml
- name: Compile Contract
  run: |
    fluent-compiler compile ./contract --json-only > build.json
    echo "RWASM_HASH=$(jq -r '.rwasm_bytecode_hex' build.json | sha256sum | cut -d' ' -f1)" >> $GITHUB_ENV

- name: Verify Contract
  run: |
    fluent-compiler verify ./contract \
      --deployed-hash "0x$RWASM_HASH" \
      --format json
```

### Extract Specific Values

```bash
# Get rWASM bytecode
fluent-compiler compile --json-only | jq -r '.rwasm_bytecode_hex'

# Get contract name
fluent-compiler compile --json-only | jq -r '.contract_name'

# Check verification status
fluent-compiler verify --deployed-hash 0x123 --format json | jq -r '.verified'
```

## License

This project is licensed under the MIT or Apache 2.0 license, at your option.
