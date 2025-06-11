//! Contract verification functionality

use crate::{compile, CompilationResult, CompileConfig};
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for contract verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyConfig {
    /// Path to the project root directory
    pub project_root: PathBuf,

    /// Deployed rWASM bytecode hash to verify against
    pub deployed_bytecode_hash: String,

    /// Optional compilation settings override
    pub compile_settings: Option<CompileSettings>,

    /// Optional metadata for the verification
    pub metadata: Option<VerificationMetadata>,
}

/// Optional metadata about the deployed contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationMetadata {
    /// Contract address
    pub address: String,

    /// Deployment transaction hash
    pub deployment_tx_hash: Option<String>,

    /// Chain ID
    pub chain_id: u64,

    /// Block number of deployment
    pub block_number: Option<u64>,
}

/// Simplified compilation settings for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileSettings {
    /// Build profile
    pub profile: String,

    /// Features to enable
    pub features: Vec<String>,

    /// Whether to disable default features
    pub no_default_features: bool,
}

/// Result of contract verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Verification status
    pub status: VerificationStatus,

    /// Contract name
    pub contract_name: String,

    /// Details about the verification
    pub details: VerificationDetails,

    /// Compilation result (if successful)
    pub compilation_result: Option<CompilationResult>,
}

/// Verification status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum VerificationStatus {
    /// Contract verified successfully
    Success,
    /// Bytecode mismatch
    BytecodeMismatch,
    /// Compilation failed
    CompilationFailed,
    /// Invalid configuration
    InvalidConfig,
}

impl VerificationStatus {
    /// Check if verification was successful
    pub fn is_success(&self) -> bool {
        matches!(self, VerificationStatus::Success)
    }
}

/// Detailed verification information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationDetails {
    /// Expected rwasm hash (from deployment)
    pub expected_hash: String,

    /// Actual bytecode hash (from compilation)
    pub actual_hash: Option<String>,

    /// Compiler version used
    pub compiler_version: Option<String>,

    /// SDK version used
    pub sdk_version: Option<String>,

    /// Build profile used
    pub build_profile: Option<String>,

    /// Error message if verification failed
    pub error_message: Option<String>,

    /// Compilation duration
    pub compilation_duration_ms: Option<u64>,

    /// Timestamp of verification
    pub timestamp: u64,
}

/// Verify a deployed contract against source code
pub fn verify_contract(config: VerifyConfig) -> Result<VerificationResult> {
    let start_time = std::time::Instant::now();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Build compilation config
    let compile_config = match build_compile_config(&config) {
        Ok(cfg) => cfg,
        Err(e) => {
            return Ok(VerificationResult {
                status: VerificationStatus::InvalidConfig,
                contract_name: String::new(),
                details: VerificationDetails {
                    expected_hash: normalize_hash(&config.deployed_bytecode_hash),
                    actual_hash: None,
                    compiler_version: None,
                    sdk_version: None,
                    build_profile: None,
                    error_message: Some(format!("Invalid configuration: {}", e)),
                    compilation_duration_ms: None,
                    timestamp,
                },
                compilation_result: None,
            });
        }
    };

    // Compile the contract
    let compilation_output = match compile(&compile_config) {
        Ok(output) => output,
        Err(e) => {
            return Ok(VerificationResult {
                status: VerificationStatus::CompilationFailed,
                contract_name: String::new(),
                details: VerificationDetails {
                    expected_hash: normalize_hash(&config.deployed_bytecode_hash),
                    actual_hash: None,
                    compiler_version: None,
                    sdk_version: None,
                    build_profile: None,
                    error_message: Some(format!("Compilation failed: {}", e)),
                    compilation_duration_ms: Some(start_time.elapsed().as_millis() as u64),
                    timestamp,
                },
                compilation_result: None,
            });
        }
    };

    let compilation_result = compilation_output.result;

    // Compare hashes
    let expected_hash = normalize_hash(&config.deployed_bytecode_hash);
    let actual_hash = normalize_hash(&compilation_result.rwasm_hash());

    let (status, error_message) = if expected_hash == actual_hash {
        (VerificationStatus::Success, None)
    } else {
        (
            VerificationStatus::BytecodeMismatch,
            Some(format!(
                "Bytecode mismatch: expected {}, got {}",
                expected_hash, actual_hash
            )),
        )
    };

    Ok(VerificationResult {
        status,
        contract_name: compilation_result.contract_info.name.clone(),
        details: VerificationDetails {
            expected_hash,
            actual_hash: Some(actual_hash),
            compiler_version: Some(compilation_result.build_metadata.rustc_version.clone()),
            sdk_version: compilation_result.contract_info.sdk_version.clone(),
            build_profile: Some(compilation_result.build_metadata.profile.clone()),
            error_message,
            compilation_duration_ms: Some(start_time.elapsed().as_millis() as u64),
            timestamp,
        },
        compilation_result: Some(compilation_result),
    })
}

/// Build compilation config from verification config
pub fn build_compile_config(verify_config: &VerifyConfig) -> Result<CompileConfig> {
    let mut builder = CompileConfig::builder()
        .project_root(verify_config.project_root.clone())
        .abi_only(); // Only need ABI for verification

    // Apply overrides if provided
    if let Some(settings) = &verify_config.compile_settings {
        builder = builder
            .profile(&settings.profile)
            .features(settings.features.clone())
            .no_default_features(settings.no_default_features);
    }

    builder.build()
}

/// Normalize hash format (remove 0x prefix, lowercase)
pub fn normalize_hash(hash: &str) -> String {
    hash.trim()
        .strip_prefix("0x")
        .unwrap_or(hash)
        .to_lowercase()
}

/// Builder for creating VerifyConfig
pub struct VerifyConfigBuilder {
    project_root: Option<PathBuf>,
    deployed_bytecode_hash: Option<String>,
    compile_settings: Option<CompileSettings>,
    metadata: Option<VerificationMetadata>,
}

impl VerifyConfigBuilder {
    pub fn new() -> Self {
        Self {
            project_root: None,
            deployed_bytecode_hash: None,
            compile_settings: None,
            metadata: None,
        }
    }

    pub fn project_root(mut self, path: PathBuf) -> Self {
        self.project_root = Some(path);
        self
    }

    pub fn deployed_bytecode_hash(mut self, hash: String) -> Self {
        self.deployed_bytecode_hash = Some(hash);
        self
    }

    pub fn with_metadata(mut self, address: String, chain_id: u64) -> Self {
        self.metadata = Some(VerificationMetadata {
            address,
            chain_id,
            deployment_tx_hash: None,
            block_number: None,
        });
        self
    }

    pub fn with_full_metadata(
        mut self,
        address: String,
        chain_id: u64,
        tx_hash: String,
        block_number: u64,
    ) -> Self {
        self.metadata = Some(VerificationMetadata {
            address,
            chain_id,
            deployment_tx_hash: Some(tx_hash),
            block_number: Some(block_number),
        });
        self
    }

    pub fn with_compile_settings(
        mut self,
        profile: &str,
        features: Vec<String>,
        no_default_features: bool,
    ) -> Self {
        self.compile_settings = Some(CompileSettings {
            profile: profile.to_string(),
            features,
            no_default_features,
        });
        self
    }

    pub fn build(self) -> Result<VerifyConfig> {
        Ok(VerifyConfig {
            project_root: self
                .project_root
                .ok_or_else(|| eyre::eyre!("project_root is required"))?,
            deployed_bytecode_hash: self
                .deployed_bytecode_hash
                .ok_or_else(|| eyre::eyre!("deployed_bytecode_hash is required"))?,
            compile_settings: self.compile_settings,
            metadata: self.metadata,
        })
    }
}

impl Default for VerifyConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_hash() {
        assert_eq!(normalize_hash("0xABCDEF123456"), "abcdef123456");
        assert_eq!(normalize_hash("abcdef123456"), "abcdef123456");
        assert_eq!(normalize_hash("  0xABCDEF123456  "), "abcdef123456");
    }

    #[test]
    fn test_verify_config_builder() {
        let config = VerifyConfigBuilder::new()
            .project_root(PathBuf::from("/test"))
            .deployed_bytecode_hash("0xabc123".to_string())
            .with_metadata("0x123".to_string(), 1)
            .build()
            .unwrap();

        assert_eq!(config.project_root, PathBuf::from("/test"));
        assert_eq!(config.deployed_bytecode_hash, "0xabc123");
        assert!(config.metadata.is_some());

        let metadata = config.metadata.unwrap();
        assert_eq!(metadata.address, "0x123");
        assert_eq!(metadata.chain_id, 1);
    }

    #[test]
    fn test_verification_status_is_success() {
        assert!(VerificationStatus::Success.is_success());
        assert!(!VerificationStatus::BytecodeMismatch.is_success());
        assert!(!VerificationStatus::CompilationFailed.is_success());
        assert!(!VerificationStatus::InvalidConfig.is_success());
    }
}
