//! WASM contract compilation library for Fluent
pub mod archive;
pub mod artifacts;
pub mod blockchain;
mod compiler;
pub mod config;
pub mod contract;
pub mod parser;
mod utils;
pub mod verify;

pub use artifacts::{save_artifacts, Abi, ArtifactWriterOptions, Metadata, SavedArtifacts};
pub use blockchain::{DeployedContractInfo, NetworkConfig};
pub use compiler::{compile, CompilationResult, CompileOutput, ContractInfo};
pub use config::{BuildProfile, CompileConfig};
pub use contract::WasmContract;
pub use verify::{
    verify_contract, VerificationMetadata, VerificationResult, VerificationStatus, VerifyConfig,
    VerifyConfigBuilder,
};
