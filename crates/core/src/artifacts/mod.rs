//! Artifact generation for compiled contracts

use crate::{
    builder::{hash_bytes, ContractInfo, RuntimeInfo},
    config::CompileConfig,
};
use eyre::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod abi;
pub mod interface;
pub mod metadata;

/// Solidity ABI represented as JSON values
pub type Abi = Vec<Value>;

/// All artifacts generated for a compiled contract
#[derive(Debug)]
pub struct ContractArtifacts {
    pub abi: Abi,
    pub interface: String,
    pub metadata: metadata::Metadata,
}

/// Generate all artifacts from compilation data
pub fn generate(
    contract: &ContractInfo,
    wasm: &[u8],
    rwasm: &[u8],
    routers: &[fluentbase_sdk_derive_core::router::Router],
    project_root: &Path,
    config: &CompileConfig,
    runtime_info: &RuntimeInfo,
    source: metadata::Source,
) -> Result<ContractArtifacts> {
    // Generate ABI
    let abi = abi::generate(routers)?;

    // Generate Solidity interface
    let interface = if !abi.is_empty() {
        interface::generate(&contract.name, &abi)?
    } else {
        String::new()
    };

    // Create metadata
    let metadata = create_metadata(
        contract,
        config,
        runtime_info,
        wasm,
        rwasm,
        &abi,
        project_root,
        source,
    )?;

    Ok(ContractArtifacts {
        abi,
        interface,
        metadata,
    })
}

/// Create metadata structure
fn create_metadata(
    contract: &ContractInfo,
    config: &CompileConfig,
    runtime_info: &RuntimeInfo,
    wasm: &[u8],
    rwasm: &[u8],
    abi: &Abi,
    project_root: &Path,
    source: metadata::Source,
) -> Result<metadata::Metadata> {
    // Calculate Cargo.lock hash
    let cargo_lock_hash = calculate_cargo_lock_hash(project_root)?;

    // Calculate toolchain hash
    let toolchain_hash = calculate_toolchain_hash(
        &runtime_info.rust.version,
        &runtime_info.sdk.tag,
        &runtime_info.sdk.commit,
    );

    Ok(metadata::Metadata {
        schema_version: 1,
        contract: contract.clone(),
        source,
        compilation_settings: metadata::CompilationSettings {
            rust: runtime_info.rust.clone(),
            sdk: runtime_info.sdk.clone(),
            build_cfg: metadata::BuildConfig::from(config),
        },
        built_at: runtime_info.built_at,
        bytecode: metadata::BytecodeInfo {
            wasm: metadata::ArtifactInfo {
                hash: format!("sha256:{}", hash_bytes(wasm)),
                size: wasm.len(),
                path: "lib.wasm".to_string(),
            },
            rwasm: metadata::ArtifactInfo {
                hash: format!("sha256:{}", hash_bytes(rwasm)),
                size: rwasm.len(),
                path: "lib.rwasm".to_string(),
            },
        },
        solidity_compatibility: if abi.is_empty() {
            None
        } else {
            Some(metadata::SolidityCompatibility {
                abi_path: "abi.json".to_string(),
                interface_path: "interface.sol".to_string(),
                function_selectors: extract_function_selectors(abi),
            })
        },
        dependencies: metadata::Dependencies {
            cargo_lock_hash: format!("sha256:{}", cargo_lock_hash),
        },
        workspace_root: None,
        toolchain_hash,
        source_tree_hash: format!("sha256:{}", runtime_info.source_tree_hash),
    })
}

/// Calculate Cargo.lock hash
fn calculate_cargo_lock_hash(project_root: &Path) -> Result<String> {
    let cargo_lock_path = project_root.join("Cargo.lock");
    if cargo_lock_path.exists() {
        let content = std::fs::read(&cargo_lock_path)?;
        Ok(hash_bytes(&content))
    } else {
        Ok("no-cargo-lock".to_string())
    }
}

/// Calculate combined toolchain hash
fn calculate_toolchain_hash(rustc_version: &str, sdk_tag: &str, sdk_commit: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(rustc_version.as_bytes());
    hasher.update(sdk_tag.as_bytes());
    hasher.update(sdk_commit.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Extract function selectors from ABI
fn extract_function_selectors(abi: &Abi) -> BTreeMap<String, String> {
    let mut selectors = BTreeMap::new();

    for func in abi.iter().filter(|e| e["type"] == "function") {
        if let Some(name) = func["name"].as_str() {
            let empty_vec = vec![];
            let inputs = func["inputs"].as_array().unwrap_or(&empty_vec);
            let types: Vec<String> = inputs
                .iter()
                .filter_map(|i| i["type"].as_str())
                .map(|s| s.to_string())
                .collect();

            let signature = format!("{}({})", name, types.join(","));
            let hash = Keccak256::digest(signature.as_bytes());
            let selector = format!("0x{}", hex::encode(&hash[..4]));

            selectors.insert(signature, selector);
        }
    }

    selectors
}

/// Information about saved artifact files
pub struct SavedPaths {
    pub output_dir: PathBuf,
    pub wasm_path: PathBuf,
    pub rwasm_path: PathBuf,
    pub abi_path: Option<PathBuf>,
    pub interface_path: Option<PathBuf>,
    pub metadata_path: Option<PathBuf>,
}

/// Save artifacts to disk
pub fn save_artifacts(
    artifacts: &ContractArtifacts,
    contract_name: &str,
    wasm: &[u8],
    rwasm: &[u8],
    output_dir: &Path,
    config: &crate::config::ArtifactsConfig,
) -> Result<SavedPaths> {
    // Create contract-specific directory
    let contract_dir = output_dir.join(format!("{}.wasm", contract_name));
    std::fs::create_dir_all(&contract_dir)
        .with_context(|| format!("Failed to create directory: {}", contract_dir.display()))?;

    // Always save bytecode
    let wasm_path = contract_dir.join("lib.wasm");
    std::fs::write(&wasm_path, wasm)?;

    let rwasm_path = contract_dir.join("lib.rwasm");
    std::fs::write(&rwasm_path, rwasm)?;

    let mut saved = SavedPaths {
        output_dir: contract_dir.clone(),
        wasm_path,
        rwasm_path,
        abi_path: None,
        interface_path: None,
        metadata_path: None,
    };

    // Save ABI if requested and not empty
    if config.generate_abi && !artifacts.abi.is_empty() {
        let abi_path = contract_dir.join("abi.json");
        let json = if config.pretty_json {
            serde_json::to_string_pretty(&artifacts.abi)?
        } else {
            serde_json::to_string(&artifacts.abi)?
        };
        std::fs::write(&abi_path, json)?;
        saved.abi_path = Some(abi_path);
    }

    // Save interface if requested and not empty
    if config.generate_interface && !artifacts.interface.is_empty() {
        let interface_path = contract_dir.join("interface.sol");
        std::fs::write(&interface_path, &artifacts.interface)?;
        saved.interface_path = Some(interface_path);
    }

    // Save metadata if requested
    if config.generate_metadata {
        let metadata_path = contract_dir.join("metadata.json");
        let json = if config.pretty_json {
            serde_json::to_string_pretty(&artifacts.metadata)?
        } else {
            serde_json::to_string(&artifacts.metadata)?
        };
        std::fs::write(&metadata_path, json)?;
        saved.metadata_path = Some(metadata_path);
    }

    tracing::info!("✅ Artifacts saved to: {}", contract_dir.display());

    Ok(saved)
}
