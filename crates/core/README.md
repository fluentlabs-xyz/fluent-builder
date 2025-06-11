# fluent-compiler

Core Rust library for compiling smart contracts to WASM and rWASM for the Fluent blockchain. Provides a comprehensive API for contract compilation, artifact generation, and verification.

## Features

- **Dual-format compilation**: Compiles Rust contracts to both WASM and rWASM in a single operation
- **Automatic ABI generation**: Extracts ABI from Rust source using `#[router]` macros
- **Artifact generation**: Creates Solidity interfaces, metadata, and verification archives
- **Contract verification**: Verify deployed contracts against source code
- **Flexible configuration**: Fine-grained control over compilation and artifact generation

## Installation

```toml
[dependencies]
fluent-compiler = "0.1"

# For blockchain integration (verification)
fluent-compiler = { version = "0.1", features = ["ethers"] }
```

## Basic Usage

### Simple Compilation

```rust
use fluent_compiler::{compile, CompileConfig};

// Compile with default settings
let config = CompileConfig::default();
let output = compile(&config)?;

println!("Contract: {}", output.result.contract_info.name);
println!("WASM size: {} bytes", output.result.outputs.wasm.len());
println!("rWASM hash: {}", output.result.rwasm_hash());
```

### Using the Builder

```rust
use fluent_compiler::{compile, CompileConfig};
use std::path::PathBuf;

let config = CompileConfig::builder()
    .project_root(PathBuf::from("./my-contract"))
    .output_dir(PathBuf::from("./build"))
    .profile("release")
    .features(vec!["production".to_string()])
    .no_default_features(true)
    .build()?;

let output = compile(&config)?;
```

### Saving Artifacts

```rust
use fluent_compiler::{compile, save_artifacts, ArtifactWriterOptions, CompileConfig};

// Compile the contract
let config = CompileConfig::default();
let output = compile(&config)?;

// Save all artifacts to disk
let options = ArtifactWriterOptions {
    output_dir: "./out".into(),
    create_archive: true,
    ..Default::default()
};

let saved = save_artifacts(&output.result, &options, &config.artifacts)?;
println!("Artifacts saved to: {}", saved.output_dir.display());
```

### Contract Verification

```rust
use fluent_compiler::{
    verify::{VerifyConfigBuilder, verify_contract},
    blockchain::NetworkConfig,
};

// Build verification config
let config = VerifyConfigBuilder::new()
    .project_root("./my-contract".into())
    .deployed_bytecode_hash("0xabcd...".to_string())
    .with_metadata("0x1234...".to_string(), 20993)
    .build()?;

// Run verification
let result = verify_contract(config)?;

if result.status.is_success() {
    println!("✅ Contract verified!");
} else {
    println!("❌ Verification failed: {:?}", result.details.error_message);
}
```

### Blockchain Integration

With the `ethers` feature enabled:

```rust
use fluent_compiler::blockchain::{NetworkConfig, ethers::fetch_bytecode_hash};

// Fetch deployed contract bytecode hash
let network = NetworkConfig::fluent_dev();
let hash = fetch_bytecode_hash(&network, "0x1234...").await?;
println!("Deployed bytecode hash: {}", hash);
```

## Configuration

### CompileConfig

The main configuration struct with builder pattern support:

```rust
pub struct CompileConfig {
    /// Project root directory
    pub project_root: PathBuf,
    /// Output directory for artifacts
    pub output_dir: PathBuf,
    /// Build profile (Debug/Release/Custom)
    pub profile: BuildProfile,
    /// Features to enable
    pub features: Vec<String>,
    /// Disable default features
    pub no_default_features: bool,
    /// Use --locked flag
    pub locked: bool,
    /// Artifact generation settings
    pub artifacts: ArtifactsConfig,
}
```

### Builder Methods

- `.project_root(path)` - Set project directory
- `.output_dir(path)` - Set output directory
- `.profile(name)` - Set build profile ("debug"/"release"/custom)
- `.features(vec)` - Set features to enable
- `.no_default_features(bool)` - Disable default features
- `.locked(bool)` - Use locked dependencies
- `.no_artifacts()` - Disable all artifact generation
- `.abi_only()` - Generate only ABI (useful for verification)

## Output Structure

Compilation produces:

```
out/
└── my-contract.wasm/
    ├── lib.wasm         # WASM bytecode
    ├── lib.rwasm        # rWASM bytecode  
    ├── abi.json         # Solidity ABI
    ├── interface.sol    # Solidity interface
    ├── metadata.json    # Build metadata
    └── sources.tar.gz   # Source archive (optional)
```

## Compilation Result

```rust
pub struct CompilationResult {
    /// Contract info from Cargo.toml
    pub contract_info: ContractInfo,
    /// Compiled bytecodes
    pub outputs: CompilationOutputs,
    /// Generated artifacts
    pub artifacts: ContractArtifacts,
    /// Build metadata
    pub build_metadata: BuildMetadata,
}

// Helper methods
result.rwasm_hash() // SHA256 hash of rWASM
result.wasm_hash()  // SHA256 hash of WASM
```

## Error Handling

The library uses `eyre::Result` with contextual error messages:

```rust
compile(&config)
    .context("Failed to compile contract")?;
```

Common error types:

- `Not a Fluent contract` - Missing fluentbase-sdk dependency
- `Compilation failed` - Cargo build errors
- `Invalid configuration` - Project root or Cargo.toml issues

## Requirements

- Rust toolchain with `wasm32-unknown-unknown` target
- Contract must depend on `fluentbase-sdk`

## Examples

See the [examples directory](examples/) for more detailed usage examples.
