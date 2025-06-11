//! Verification-focused metadata for reproducible builds

use super::{Abi, ArtifactContext};
use crate::{
    config::{BuildProfile, CompileConfig},
    contract::WasmContract,
    utils,
};
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Compact metadata structure optimized for verification
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metadata {
    /// Contract information from Cargo.toml
    pub contract: ContractMetadata,

    /// Bytecode hashes and sizes (no actual bytecode)
    pub bytecode: BytecodeMetadata,

    /// Contract ABI (only present if contract has #[router] macro)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub abi: Abi,

    /// Method signatures to selectors mapping
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub method_identifiers: BTreeMap<String, String>,

    /// Compiler and build environment information
    pub compiler: CompilerMetadata,

    /// Build configuration used
    pub build_config: BuildConfigMetadata,

    /// Metadata format version
    pub version: u32,
}

/// Contract metadata extracted from Cargo.toml and Cargo.lock
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContractMetadata {
    /// Contract name from package.name
    pub name: String,
    /// Contract version from package.version
    pub version: String,
    /// SDK version (required for verification)
    pub sdk_version: String,
}

impl From<&WasmContract> for ContractMetadata {
    fn from(contract: &WasmContract) -> Self {
        Self {
            name: contract.name.clone(),
            version: contract.version.clone(),
            sdk_version: contract.sdk_version.clone().unwrap_or_default(),
        }
    }
}

/// Bytecode information without actual bytecode
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BytecodeMetadata {
    /// WASM artifact info
    pub wasm: ArtifactInfo,
    /// rWASM artifact info (deployed bytecode)
    pub rwasm: ArtifactInfo,
}

/// Information about a single artifact
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactInfo {
    /// SHA256 hash of the bytecode
    pub hash: String,
    /// Size in bytes
    pub size: usize,
}

/// Compiler information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompilerMetadata {
    /// Rust compiler version (full string, e.g., "rustc 1.75.0 (82e1608df 2023-12-21)")
    pub rustc_version: String,
    /// Rust compiler commit hash extracted from version string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rustc_commit: Option<String>,
    /// Target triple (always "wasm32-unknown-unknown" for WASM)
    pub target: String,
    /// Compilation timestamp (UTC seconds)
    pub timestamp: u64,
}

/// Build configuration metadata (subset of CompileConfig relevant for verification)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildConfigMetadata {
    /// Build profile used
    pub profile: String,
    /// Features enabled during compilation
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub features: Vec<String>,
    /// Whether default features were disabled
    pub no_default_features: bool,
    /// Whether --locked flag was used
    pub locked: bool,
}

impl From<&CompileConfig> for BuildConfigMetadata {
    fn from(config: &CompileConfig) -> Self {
        Self {
            profile: match &config.profile {
                BuildProfile::Debug => "debug".to_string(),
                BuildProfile::Release => "release".to_string(),
                BuildProfile::Custom(name) => name.clone(),
            },
            features: config.features.clone(),
            no_default_features: config.no_default_features,
            locked: config.locked,
        }
    }
}

impl Metadata {
    /// Extract rustc version number (e.g., "1.75.0" from "rustc 1.75.0 (82e1608df 2023-12-21)")
    pub fn rustc_version_number(&self) -> Option<&str> {
        self.compiler.rustc_version.split_whitespace().nth(1)
    }

    /// Get SDK version
    pub fn sdk_version(&self) -> &str {
        &self.contract.sdk_version
    }

    /// Check if contract has ABI (i.e., has #[router] macro)
    pub fn has_abi(&self) -> bool {
        !self.abi.is_empty()
    }

    /// Get the deployed bytecode hash (rWASM)
    pub fn deployed_bytecode_hash(&self) -> &str {
        &self.bytecode.rwasm.hash
    }

    /// Convert metadata to proto CompileSettings for verification request
    pub fn to_compile_settings(&self) -> CompileSettings {
        CompileSettings {
            rustc_version: self.compiler.rustc_version.clone(),
            sdk_version: self.contract.sdk_version.clone(),
            profile: self.build_config.profile.clone(),
            features: self.build_config.features.clone(),
            no_default_features: self.build_config.no_default_features,
        }
    }

    /// Get ABI as JSON string
    pub fn abi_json(&self) -> Result<String> {
        if self.abi.is_empty() {
            Ok("[]".to_string())
        } else {
            serde_json::to_string(&self.abi).context("Failed to serialize ABI to JSON")
        }
    }
}

/// Proto-compatible compile settings structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileSettings {
    pub rustc_version: String,
    pub sdk_version: String,
    pub profile: String,
    pub features: Vec<String>,
    pub no_default_features: bool,
}

/// Generates verification-optimized metadata
pub fn generate(ctx: &ArtifactContext<'_>, abi: &Abi) -> Result<Metadata> {
    // Ensure SDK version is present
    if ctx.contract.sdk_version.is_none() {
        return Err(eyre::eyre!(
            "SDK version is required for metadata generation. \
             Ensure Cargo.lock exists and contains fluentbase-sdk"
        ));
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(Metadata {
        contract: ContractMetadata::from(ctx.contract),
        bytecode: BytecodeMetadata {
            wasm: ArtifactInfo {
                hash: utils::hash_bytes(ctx.bytecode),
                size: ctx.bytecode.len(),
            },
            rwasm: ArtifactInfo {
                hash: utils::hash_bytes(ctx.deployed_bytecode),
                size: ctx.deployed_bytecode.len(),
            },
        },
        abi: abi.clone(),
        method_identifiers: if abi.is_empty() {
            BTreeMap::new()
        } else {
            extract_method_identifiers(abi)
        },
        compiler: CompilerMetadata {
            rustc_version: ctx.build_info.rustc_version.clone(),
            rustc_commit: extract_rustc_commit(&ctx.build_info.rustc_version),
            target: ctx.build_info.target.clone(),
            timestamp,
        },
        build_config: BuildConfigMetadata::from(&ctx.build_info.compile_config),
        version: 2,
    })
}

/// Reads metadata from a JSON file
pub fn read_from_file(path: &std::path::Path) -> Result<Metadata> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read metadata from {}", path.display()))?;

    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse metadata from {}", path.display()))
}

/// Writes metadata to a JSON file
pub fn write_to_file(metadata: &Metadata, path: &std::path::Path, pretty: bool) -> Result<()> {
    let json = if pretty {
        serde_json::to_string_pretty(metadata)?
    } else {
        serde_json::to_string(metadata)?
    };

    std::fs::write(path, json)
        .with_context(|| format!("Failed to write metadata to {}", path.display()))?;

    Ok(())
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
    fn test_metadata_to_compile_settings() {
        let metadata = Metadata {
            contract: ContractMetadata {
                name: "test".to_string(),
                version: "0.1.0".to_string(),
                sdk_version: "0.5.0".to_string(),
            },
            compiler: CompilerMetadata {
                rustc_version: "rustc 1.75.0 (82e1608df 2023-12-21)".to_string(),
                rustc_commit: Some("82e1608df".to_string()),
                target: "wasm32-unknown-unknown".to_string(),
                timestamp: 1234567890,
            },
            build_config: BuildConfigMetadata {
                profile: "release".to_string(),
                features: vec!["production".to_string()],
                no_default_features: true,
                locked: false,
            },
            ..Default::default()
        };

        let settings = metadata.to_compile_settings();
        assert_eq!(
            settings.rustc_version,
            "rustc 1.75.0 (82e1608df 2023-12-21)"
        );
        assert_eq!(settings.sdk_version, "0.5.0");
        assert_eq!(settings.profile, "release");
        assert_eq!(settings.features, vec!["production"]);
        assert!(settings.no_default_features);
    }

    #[test]
    fn test_compact_metadata_size() {
        let metadata = Metadata {
            contract: ContractMetadata {
                name: "power-calculator".to_string(),
                version: "0.1.0".to_string(),
                sdk_version: "0.5.0".to_string(),
            },
            bytecode: BytecodeMetadata {
                wasm: ArtifactInfo {
                    hash: "a".repeat(64),
                    size: 37888,
                },
                rwasm: ArtifactInfo {
                    hash: "b".repeat(64),
                    size: 59392,
                },
            },
            compiler: CompilerMetadata {
                rustc_version: "rustc 1.75.0 (82e1608df 2023-12-21)".to_string(),
                rustc_commit: Some("82e1608df".to_string()),
                target: "wasm32-unknown-unknown".to_string(),
                timestamp: 1718148540,
            },
            build_config: BuildConfigMetadata {
                profile: "release".to_string(),
                features: vec![],
                no_default_features: true,
                locked: false,
            },
            abi: vec![json!({
                "name": "calculate_power",
                "type": "function",
                "inputs": [
                    {"name": "base", "type": "uint256"},
                    {"name": "exponent", "type": "uint256"}
                ],
                "outputs": [{"name": "", "type": "uint256"}],
                "stateMutability": "pure"
            })],
            method_identifiers: {
                let mut m = BTreeMap::new();
                m.insert(
                    "calculate_power(uint256,uint256)".to_string(),
                    "c4e41b3a".to_string(),
                );
                m
            },
            version: 2,
        };

        let json = serde_json::to_string_pretty(&metadata).unwrap();
        println!("Metadata JSON:\n{}", json);
        println!("\nMetadata size: {} bytes", json.len());

        // Should be compact without source hashes
        assert!(
            json.len() < 2500,
            "Metadata too large: {} bytes",
            json.len()
        );
    }

    #[test]
    fn test_abi_json() {
        let metadata = Metadata {
            abi: vec![json!({
                "name": "test",
                "type": "function"
            })],
            ..Default::default()
        };

        let abi_json = metadata.abi_json().unwrap();
        assert!(abi_json.contains("test"));

        let empty_metadata = Metadata::default();
        assert_eq!(empty_metadata.abi_json().unwrap(), "[]");
    }
}
