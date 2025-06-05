//! Artifact generation for compiled WASM contracts

use crate::{config::CompileConfig, contract::WasmContract};
use eyre::Result;

pub mod abi;
pub mod interface;
pub mod metadata;
pub mod writer;

// Re-export commonly used types
pub use abi::Abi;
pub use metadata::Metadata;
use serde::{Deserialize, Serialize};

pub use writer::{save_artifacts, ArtifactWriterOptions, SavedArtifacts};

/// All artifacts generated for a compiled contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractArtifacts {
    /// Solidity ABI
    pub abi: Abi,
    /// Solidity interface source code
    pub interface: String,
    /// Compilation metadata (Foundry format)
    pub metadata: Metadata,
}

/// Input data for artifact generation
pub struct ArtifactContext<'a> {
    /// Contract name
    pub name: &'a str,
    /// WASM bytecode
    pub bytecode: &'a [u8],
    /// Deployed bytecode (rWASM)
    pub deployed_bytecode: &'a [u8],
    /// Parsed routers
    pub routers: &'a [fluentbase_sdk_derive_core::router::Router],
    /// Contract information
    pub contract: &'a WasmContract,
    /// Compilation info
    pub build_info: BuildInfo,
}

/// Build information for verification
#[derive(Debug, Clone)]
pub struct BuildInfo {
    /// Rust compiler version
    pub rustc_version: String,
    /// Target triple
    pub target: String,
    /// Build profile
    pub profile: String,
    /// Enabled features
    pub features: Vec<String>,
    /// Source code hash
    #[allow(dead_code)] // Used in metadata.rs through ArtifactContext
    pub source_hash: String,
    /// Compilation configuration used
    pub compile_config: CompileConfig,
}

/// Generates all artifacts for a contract
pub fn generate(ctx: &ArtifactContext<'_>) -> Result<ContractArtifacts> {
    let abi = abi::generate(ctx.routers)?;
    let interface = interface::generate(ctx.name, &abi)?;
    let metadata = metadata::generate(ctx, &abi)?;

    Ok(ContractArtifacts {
        abi,
        interface,
        metadata,
    })
}
