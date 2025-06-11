# fluent-compiler

Rust library and CLI for compiling and verifying smart contracts for the Fluent blockchain.

## Installation

```bash
# Prerequisites
rustup target add wasm32-unknown-unknown

# Install CLI
cargo install --path crates/cli

# Or for development
just link
```

## Usage

### Compile

```bash
# Compile contract
fluent-compiler compile ./my-contract

# With source archive for verification
fluent-compiler compile ./my-contract --archive

# JSON output for CI/CD
fluent-compiler compile ./my-contract --json
```

### Verify

```bash
# Verify deployed contract
fluent-compiler verify ./my-contract \
  --address 0x1234... \
  --chain-id 20993 \
  --rpc https://rpc.dev.gblend.xyz
```

## Quick Example

```bash
# Compile example contract
just compile power-calculator

# Deploy and verify (requires DEPLOY_PRIVATE_KEY)
just deploy-and-verify power-calculator
```

## Library Usage

```rust
use fluent_compiler::{compile, CompileConfig};

// Compile
let config = CompileConfig::builder()
    .project_root("./my-contract".into())
    .profile("release")
    .build()?;

let output = compile(&config)?;
println!("rWASM hash: {}", output.result.rwasm_hash());

// Verify
use fluent_compiler::verify::{VerifyConfigBuilder, verify_contract};

let config = VerifyConfigBuilder::new()
    .project_root("./my-contract".into())
    .deployed_bytecode_hash("0x...".to_string())
    .with_metadata("0xaddress".to_string(), 20993)
    .build()?;

let result = verify_contract(config)?;
```

## Output Structure

```
out/
└── ContractName.wasm/
    ├── lib.wasm         # WASM bytecode
    ├── lib.rwasm        # rWASM bytecode
    ├── abi.json         # Contract ABI
    ├── interface.sol    # Solidity interface
    ├── metadata.json    # Build metadata
    └── sources.tar.gz   # Source archive (--archive)
```

## CI/CD

```yaml
- name: Compile
  run: fluent-compiler compile ./contract --json > build.json

- name: Verify
  run: |
    fluent-compiler verify ./contract \
      --address $CONTRACT_ADDRESS \
      --chain-id $CHAIN_ID \
      --rpc $RPC_URL \
      --json
```

## Development

```bash
just          # Show commands
just build    # Build CLI
just test     # Run tests
just clean    # Clean artifacts
```
