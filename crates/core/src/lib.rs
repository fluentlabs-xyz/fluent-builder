//! WASM contract compilation library for Fluent
pub mod archive;
pub mod artifacts;
mod compiler;
pub mod config;
pub mod contract;
pub mod parser;
mod utils;

pub use artifacts::{save_artifacts, Abi, ArtifactWriterOptions, Metadata, SavedArtifacts};

pub use compiler::{compile, CompilationResult, CompilationSettings, CompileOutput, ContractInfo};
pub use config::{BuildProfile, CompileConfig};
pub use contract::WasmContract;
