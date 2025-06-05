# `fluent-compiler`

The core Rust library for compiling smart contracts into WASM and rWASM for the Fluent ecosystem. It provides a flexible and powerful API for integrating a standardized, reproducible contract compilation workflow into developer tools and verification services.

## Purpose

This library is the engine that powers tools like `fluent-compiler-cli`. It automates the entire build process, from analyzing Rust source code to generating a full suite of artifacts required for on-chain deployment and verification. It handles compiling Rust projects to WASM, converting WASM to rWASM, and generating all necessary metadata.

## Key Features

* **Dual-Format Compilation:** Compiles Rust contracts to both standard WASM and the execution-optimized rWASM format in a single, streamlined operation.
* **Automated Artifact Generation:** Produces essential development and verification artifacts, including a Solidity-compatible ABI, a Solidity `interface` file, and detailed, reproducible build metadata.
* **Source Code Parsing:** Analyzes Rust source code using `syn` to automatically generate ABIs from `#[router]` macro attributes, eliminating the need for manual ABI definitions.
* **Verifiable Archiving:** Creates source code archives (`.tar.gz` or `.zip`) that bundle all necessary files for reproducible, verifiable builds, a critical feature for verifiers.
* **Flexible Configuration:** Offers granular control over the compilation process through the `CompileConfig` struct, including build profiles, features, target triples, and artifact generation options.

## Basic Usage

The library can be used to compile a contract and either process the results in-memory or save them to disk.

**1. Compile and Access Artifacts In-Memory**

This example shows how to compile a contract and access the resulting bytecodes and ABI directly.

```rust
use fluent_compiler::{compile, CompileConfig};
use std::path::PathBuf;

// 1. Configure the compilation for a specific project.
let mut config = CompileConfig::default();
config.project_root = PathBuf::from("./path/to/my-contract");

// It's good practice to validate the configuration and project path.
config.validate().expect("Invalid configuration");

// 2. Run the compilation.
match compile(&config) {
    Ok(output) => {
        println!("✅ Compilation successful in {:?}", output.duration);

        // Access the compiled bytecodes.
        let wasm_len = output.result.outputs.wasm.len();
        let rwasm_len = output.result.outputs.rwasm.len();
        println!("- WASM size: {} bytes", wasm_len);
        println!("- rWASM size: {} bytes", rwasm_len);

        // Access the generated ABI.
        let abi_json = serde_json::to_string_pretty(&output.result.artifacts.abi).unwrap();
        println!("- Generated ABI:\n{}", abi_json);
    }
    Err(e) => {
        eprintln!("🔥 Compilation failed: {:?}", e);
    }
}
```

**2. Compile and Save All Artifacts to Disk**

This is a common use case for local development or CI pipelines that need to store build artifacts.

```rust
use fluent_compiler::{compile, save_artifacts, ArtifactWriterOptions, CompileConfig};
use std::path::PathBuf;

// Define the project to compile.
let config = CompileConfig {
    project_root: PathBuf::from("./path/to/my-contract"),
    ..Default::default()
};
config.validate().unwrap();

// First, compile the project to get the results.
let output = compile(&config).expect("Compilation failed");

// Next, configure how and where to save the artifacts.
let save_options = ArtifactWriterOptions {
    output_dir: PathBuf::from("./out"),
    create_archive: true, // Also create a source archive for verification.
    ..Default::default()
};

// Finally, save the artifacts to disk.
let saved_paths = save_artifacts(&output.result, &save_options, &config.artifacts)
    .expect("Failed to save artifacts");

println!("✅ All artifacts saved to: {}", saved_paths.output_dir.display());
println!("  - rWASM: {}", saved_paths.rwasm_path.display());
if let Some(metadata_path) = saved_paths.metadata_path {
    println!("  - Metadata: {}", metadata_path.display());
}
if let Some(archive_path) = saved_paths.archive_path {
    println!("  - Archive: {}", archive_path.display());
}
```

## Core Configuration

The compilation process is primarily controlled by the `CompileConfig` struct. You can create one using `CompileConfig::default()` and then customize its fields to fit your needs. It contains nested structs like `WasmConfig` and `ArtifactsConfig` to manage specific aspects of the build.

```rust
use fluent_compiler::{BuildProfile, config::{WasmConfig, RwasmConfig, ArtifactsConfig}};
use std::path::PathBuf;

// Key fields in fluent_compiler::CompileConfig
pub struct CompileConfig {
    /// Path to the Rust contract project root.
    pub project_root: PathBuf,
    /// Base directory for all generated artifacts.
    pub output_dir: PathBuf,
    /// WASM compilation settings (profile, features, etc.).
    pub wasm: WasmConfig,
    /// rWASM conversion settings.
    pub rwasm: RwasmConfig,
    /// Artifact generation settings (ABI, interface, etc.).
    pub artifacts: ArtifactsConfig,
}
```
