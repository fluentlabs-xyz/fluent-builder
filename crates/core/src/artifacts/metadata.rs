//! Verification-focused metadata for reproducible builds

use super::{Abi, ArtifactContext};
use crate::utils;
use eyre::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Main metadata structure optimized for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// Contract name from Cargo.toml
    pub contract_name: String,

    /// Contract ABI
    pub abi: Abi,

    /// Method signatures to selectors mapping
    pub method_identifiers: BTreeMap<String, String>,

    /// Compiled bytecodes
    pub bytecodes: Bytecodes,

    /// Build metadata for reproducibility
    pub build_metadata: BuildMetadata,
}

/// Compiled bytecode information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bytecodes {
    /// WASM bytecode
    pub wasm: BytecodeInfo,
    /// rWASM bytecode (deployed)
    pub rwasm: BytecodeInfo,
}

/// Information about a bytecode artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BytecodeInfo {
    /// Hex-encoded bytecode with 0x prefix
    pub object: String,
    /// SHA256 hash of the bytecode
    pub hash: String,
    /// Size in bytes
    pub size: usize,
}

/// Build metadata matching the proto definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    /// Compiler information
    pub compiler: CompilerInfo,

    /// Programming language
    pub language: String,

    /// Output artifacts info (matches proto BuildOutputInfo)
    pub output: BuildOutputInfo,

    /// Build settings used
    pub settings: BuildSettings,

    /// Source files information
    pub sources: BTreeMap<String, SourceFileInfo>,

    /// Metadata format version
    pub metadata_format_version: u32,
}

/// Compiler information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerInfo {
    /// Compiler name (always "rustc" for Rust contracts)
    pub name: String,
    /// Full version string
    pub version: String,
    /// Commit hash if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// Build output information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutputInfo {
    /// WASM artifact info
    pub wasm: WasmArtifactInfo,
    /// rWASM artifact info
    pub rwasm: WasmArtifactInfo,
}

/// Information about a WASM/rWASM artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmArtifactInfo {
    /// SHA256 hash of the bytecode
    pub hash: String,
    /// Size in bytes
    pub size: usize,
}

/// Build settings for reproducibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSettings {
    /// Target triple
    pub target_triple: String,
    /// Build profile
    pub profile: String,
    /// Enabled features
    pub features: Vec<String>,
    /// Whether default features were disabled
    pub no_default_features: bool,
    /// Contract-specific information
    pub contract_info: ContractBuildInfo,
    /// Build timestamp (UTC seconds)
    pub build_time_utc_seconds: u64,
    /// Cargo flags used
    pub cargo_flags_used: Vec<String>,
    /// RUSTFLAGS if set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rustflags: Option<String>,
}

/// Contract build information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractBuildInfo {
    /// Path to Cargo.toml relative to source root
    pub path_to_cargo_toml: String,
    /// Package name
    pub name: String,
    /// Package version
    pub version: String,
    /// SDK version used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk_version: Option<String>,
}

/// Source file information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFileInfo {
    /// SHA256 hash of file content
    pub content_hash: String,
    /// SPDX license identifier if found
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_identifier: Option<String>,
}

/// Generates verification-optimized metadata
pub fn generate(ctx: &ArtifactContext<'_>, abi: &Abi) -> Result<Metadata> {
    let wasm_hash = utils::hash_bytes(ctx.bytecode);
    let rwasm_hash = utils::hash_bytes(ctx.deployed_bytecode);

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(Metadata {
        contract_name: ctx.name.to_string(),
        abi: abi.clone(),
        method_identifiers: extract_method_identifiers(abi),
        bytecodes: Bytecodes {
            wasm: BytecodeInfo {
                object: utils::bytes_to_hex(ctx.bytecode),
                hash: wasm_hash.clone(),
                size: ctx.bytecode.len(),
            },
            rwasm: BytecodeInfo {
                object: utils::bytes_to_hex(ctx.deployed_bytecode),
                hash: rwasm_hash.clone(),
                size: ctx.deployed_bytecode.len(),
            },
        },
        build_metadata: BuildMetadata {
            compiler: CompilerInfo {
                name: "rustc".to_string(),
                version: ctx.build_info.rustc_version.clone(),
                commit: extract_rustc_commit(&ctx.build_info.rustc_version),
            },
            language: "Rust".to_string(),
            output: BuildOutputInfo {
                wasm: WasmArtifactInfo {
                    hash: wasm_hash,
                    size: ctx.bytecode.len(),
                },
                rwasm: WasmArtifactInfo {
                    hash: rwasm_hash,
                    size: ctx.deployed_bytecode.len(),
                },
            },
            settings: BuildSettings {
                target_triple: ctx.build_info.target.clone(),
                profile: ctx.build_info.profile.clone(),
                features: ctx.build_info.features.clone(),
                no_default_features: ctx.build_info.compile_config.wasm.no_default_features,
                contract_info: ContractBuildInfo {
                    path_to_cargo_toml: "Cargo.toml".to_string(), // Relative to project root
                    name: ctx.contract.name.clone(),
                    version: ctx.contract.version.clone(),
                    sdk_version: ctx.contract.sdk_version.clone(),
                },
                build_time_utc_seconds: timestamp,
                cargo_flags_used: ctx.build_info.compile_config.wasm.cargo_flags.clone(),
                rustflags: ctx.build_info.compile_config.wasm.rustflags.clone(),
            },
            sources: collect_source_files(ctx)?,
            metadata_format_version: 1,
        },
    })
}

/// Extracts method identifiers from ABI
fn extract_method_identifiers(abi: &Abi) -> BTreeMap<String, String> {
    use sha3::{Digest, Keccak256};

    let mut identifiers = BTreeMap::new();

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
            let selector = hex::encode(&hash[..4]);

            identifiers.insert(signature, selector);
        }
    }

    identifiers
}

/// Collects source file information
fn collect_source_files(ctx: &ArtifactContext<'_>) -> Result<BTreeMap<String, SourceFileInfo>> {
    use walkdir::WalkDir;

    let mut sources = BTreeMap::new();
    let project_root = &ctx.build_info.compile_config.project_root;

    for entry in WalkDir::new(project_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip irrelevant directories
        if should_skip_path(path) {
            continue;
        }

        // Include only Rust source files and Cargo files
        if !is_relevant_source_file(path) {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(path) {
            // Calculate SHA256 (not Keccak for better compatibility)
            let hash = {
                let mut hasher = Sha256::new();
                hasher.update(content.as_bytes());
                format!("{:x}", hasher.finalize())
            };

            // Extract license
            let license = extract_spdx_license(&content);

            // Create relative path
            let relative_path = path
                .strip_prefix(project_root)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned();

            sources.insert(
                relative_path,
                SourceFileInfo {
                    content_hash: hash,
                    license_identifier: license,
                },
            );
        }
    }

    Ok(sources)
}

/// Checks if a path should be skipped during source collection
fn should_skip_path(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        name == "target" || name.starts_with('.') || name == "out"
    })
}

/// Checks if a file is relevant for source verification
fn is_relevant_source_file(path: &std::path::Path) -> bool {
    // Check extension
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext == "rs" {
            return true;
        }
    }

    // Check specific files
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(name, "Cargo.toml" | "Cargo.lock")
    } else {
        false
    }
}

/// Extracts rustc commit from version string
fn extract_rustc_commit(version: &str) -> Option<String> {
    // Format: "rustc 1.75.0 (82e1608df 2023-12-21)"
    let parts: Vec<&str> = version.split(' ').collect();
    if parts.len() >= 3 {
        parts[2].strip_prefix('(').map(|s| s.to_string())
    } else {
        None
    }
}

/// Extracts SPDX license from source
fn extract_spdx_license(content: &str) -> Option<String> {
    for line in content.lines().take(10) {
        if let Some(pos) = line.find("SPDX-License-Identifier:") {
            let license = line[pos + 24..]
                .trim()
                .trim_end_matches("*/")
                .trim_end_matches("-->")
                .trim_end_matches("//")
                .trim();

            if !license.is_empty() {
                return Some(license.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_method_identifiers() {
        let abi = vec![json!({
            "name": "transfer",
            "type": "function",
            "inputs": [
                {"type": "address"},
                {"type": "uint256"}
            ]
        })];

        let identifiers = extract_method_identifiers(&abi);
        assert_eq!(
            identifiers.get("transfer(address,uint256)"),
            Some(&"a9059cbb".to_string())
        );
    }

    #[test]
    fn test_metadata_structure_matches_proto() {
        // This test ensures our Rust structure can be serialized to match proto
        let metadata = Metadata {
            contract_name: "TestContract".to_string(),
            abi: vec![],
            method_identifiers: BTreeMap::new(),
            bytecodes: Bytecodes {
                wasm: BytecodeInfo {
                    object: "0x0061736d".to_string(),
                    hash: "abc123".to_string(),
                    size: 4,
                },
                rwasm: BytecodeInfo {
                    object: "0x0061736d01".to_string(),
                    hash: "def456".to_string(),
                    size: 5,
                },
            },
            build_metadata: BuildMetadata {
                compiler: CompilerInfo {
                    name: "rustc".to_string(),
                    version: "rustc 1.78.0 (9b00956e5 2024-04-29)".to_string(),
                    commit: Some("9b00956e5".to_string()),
                },
                language: "Rust".to_string(),
                output: BuildOutputInfo {
                    wasm: WasmArtifactInfo {
                        hash: "abc123".to_string(),
                        size: 4,
                    },
                    rwasm: WasmArtifactInfo {
                        hash: "def456".to_string(),
                        size: 5,
                    },
                },
                settings: BuildSettings {
                    target_triple: "wasm32-unknown-unknown".to_string(),
                    profile: "release".to_string(),
                    features: vec!["wasm".to_string()],
                    no_default_features: true,
                    contract_info: ContractBuildInfo {
                        path_to_cargo_toml: "Cargo.toml".to_string(),
                        name: "test-contract".to_string(),
                        version: "0.1.0".to_string(),
                        sdk_version: Some("0.5.0".to_string()),
                    },
                    build_time_utc_seconds: 1234567890,
                    cargo_flags_used: vec![],
                    rustflags: None,
                },
                sources: BTreeMap::new(),
                metadata_format_version: 1,
            },
        };

        // Should serialize without panic
        let json = serde_json::to_string_pretty(&metadata).unwrap();
        assert!(json.contains("test-contract"));

        // Should be able to access nested fields as proto expects
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["build_metadata"]["compiler"]["name"].as_str(),
            Some("rustc")
        );
        assert_eq!(
            value["build_metadata"]["settings"]["contract_info"]["sdk_version"].as_str(),
            Some("0.5.0")
        );
    }
}
