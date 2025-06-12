//! Rust Smart Contract Compilation Library for Fluent Blockchain
//!
//! This library provides tools for compiling Rust smart contracts to WASM/rWASM,
//! generating Solidity-compatible interfaces, and verifying deployed contracts.

// Internal modules
mod archive;
mod artifacts;
mod compiler;
mod config;
mod git;
mod parser;
mod verify;

// Public API - only expose what's necessary

// Core compilation
pub use compiler::{compile, get_rwasm_hash, get_wasm_hash, CompilationResult, ContractInfo};
pub use config::{ArtifactsConfig, CompileConfig};

// Artifact management
pub use artifacts::{metadata::Source, save_artifacts, Abi, ContractArtifacts, SavedPaths};

// Verification
pub use verify::{verify, VerificationResult, VerificationStatus, VerifyConfig};

pub use archive::{create_verification_archive, ArchiveFormat, ArchiveInfo, ArchiveOptions};
pub use git::{detect_git_info, get_project_path_in_repo, GitInfo};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compile a contract at the given path with default settings
///
/// # Example
/// ```no_run
/// use fluent_builder::compile_at;
///
/// let result = compile_at("./my-contract").unwrap();
/// println!("Compiled: {} v{}", result.contract.name, result.contract.version);
/// ```
pub fn compile_at(project_root: impl Into<std::path::PathBuf>) -> eyre::Result<CompilationResult> {
    let config = CompileConfig::new(project_root);
    compile(&config)
}

/// Verify a deployed contract matches the source code
///
/// # Example
/// ```no_run
/// use fluent_builder::verify_at;
///
/// let matches = verify_at("./my-contract", "0xabc123...").unwrap();
/// assert!(matches);
/// ```
pub fn verify_at(
    project_root: impl Into<std::path::PathBuf>,
    deployed_bytecode_hash: &str,
) -> eyre::Result<bool> {
    use verify::VerifyConfig;

    let config = VerifyConfig {
        project_root: project_root.into(),
        deployed_bytecode_hash: deployed_bytecode_hash.to_string(),
        compile_config: None,
    };

    let result = verify(config)?;
    Ok(result.status == VerificationStatus::Success)
}
