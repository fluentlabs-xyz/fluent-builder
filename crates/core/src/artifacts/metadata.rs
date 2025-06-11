//! Metadata structures for contract verification
//!
//! CRITICAL: The JSON schema produced by these structures is a contract
//! with external systems and must not be changed.

use crate::compiler::{ContractInfo, RustInfo, SdkInfo};
use crate::config::CompileConfig;
use serde::Serialize;
use std::collections::BTreeMap;

/// Root metadata structure for contract verification
///
/// This combines static config + runtime detected info to create
/// a complete picture for reproducible builds.
#[derive(Debug, Clone, Serialize)]
pub struct Metadata {
    pub schema_version: u32,
    pub contract: ContractInfo,
    pub source: Source,
    pub compilation_settings: CompilationSettings,
    pub built_at: u64,
    pub bytecode: BytecodeInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solidity_compatibility: Option<SolidityCompatibility>,
    pub dependencies: Dependencies,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    pub toolchain_hash: String,
    pub source_tree_hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Source {
    #[serde(rename = "archive")]
    Archive {
        archive_path: String,
        project_path: String,
    },
    #[serde(rename = "git")]
    Git {
        repository: String,
        commit: String,
        project_path: String,
    },
}

impl Default for Source {
    fn default() -> Self {
        Source::Archive {
            archive_path: String::new(),
            project_path: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompilationSettings {
    pub rust: RustInfo,
    pub sdk: SdkInfo,
    pub build_cfg: BuildConfig,
}

/// Build configuration from CompileConfig
#[derive(Debug, Clone, Serialize)]
pub struct BuildConfig {
    pub profile: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub features: Vec<String>,
    pub no_default_features: bool,
    pub locked: bool,
}

impl From<&CompileConfig> for BuildConfig {
    fn from(config: &CompileConfig) -> Self {
        Self {
            profile: config.profile.clone(),
            features: config.features.clone(),
            no_default_features: config.no_default_features,
            locked: config.locked,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BytecodeInfo {
    pub wasm: ArtifactInfo,
    pub rwasm: ArtifactInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactInfo {
    pub hash: String,
    pub size: usize,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SolidityCompatibility {
    pub abi_path: String,
    pub interface_path: String,
    pub function_selectors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dependencies {
    pub cargo_lock_hash: String,
}
