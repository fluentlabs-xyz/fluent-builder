//! Contract verification functionality

use crate::{build, CompilationResult, CompileConfig};
use eyre::Result;
use std::path::PathBuf;

/// Configuration for contract verification
pub struct VerifyConfig {
    /// Path to the project root directory
    pub project_root: PathBuf,

    /// Deployed bytecode hash to verify against
    pub deployed_bytecode_hash: String,

    /// Optional compilation config override
    pub compile_config: Option<CompileConfig>,
}

/// Result of contract verification
pub struct VerificationResult {
    /// Verification status
    pub status: VerificationStatus,

    /// Contract name (if compilation succeeded)
    pub contract_name: String,

    /// Full compilation result (if needed for debugging)
    pub compilation_result: Option<CompilationResult>,
}

/// Verification status
#[derive(Debug, Clone, PartialEq)]
pub enum VerificationStatus {
    /// Contract verified successfully
    Success,

    /// Bytecode mismatch
    Mismatch { expected: String, actual: String },

    /// Compilation failed
    CompilationFailed(String),
}

impl VerificationStatus {
    /// Check if verification was successful
    pub fn is_success(&self) -> bool {
        matches!(self, VerificationStatus::Success)
    }
}

/// Verify that source code matches deployed bytecode
pub fn verify(config: VerifyConfig) -> Result<VerificationResult> {
    // Build compilation config
    let compile_config = config
        .compile_config
        .unwrap_or_else(|| CompileConfig::new(config.project_root.clone()));

    // Compile the contract
    let compilation_result = match build(&compile_config) {
        Ok(result) => result,
        Err(e) => {
            return Ok(VerificationResult {
                status: VerificationStatus::CompilationFailed(e.to_string()),
                contract_name: String::new(),
                compilation_result: None,
            });
        }
    };

    // Get hashes
    let expected_hash = normalize_hash(&config.deployed_bytecode_hash);
    let actual_hash = normalize_hash(&get_rwasm_hash(&compilation_result));

    // Compare
    let status = if expected_hash == actual_hash {
        VerificationStatus::Success
    } else {
        VerificationStatus::Mismatch {
            expected: expected_hash,
            actual: actual_hash,
        }
    };

    Ok(VerificationResult {
        status,
        contract_name: compilation_result.contract.name.clone(),
        compilation_result: Some(compilation_result),
    })
}

/// Normalize hash format (remove 0x prefix, lowercase)
pub fn normalize_hash(hash: &str) -> String {
    hash.trim()
        .strip_prefix("0x")
        .unwrap_or(hash)
        .to_lowercase()
}

/// Get rWASM hash from compilation result
fn get_rwasm_hash(result: &CompilationResult) -> String {
    crate::builder::hash_bytes(&result.outputs.rwasm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_hash() {
        assert_eq!(normalize_hash("0xABCDEF123456"), "abcdef123456");
        assert_eq!(normalize_hash("abcdef123456"), "abcdef123456");
        assert_eq!(normalize_hash("  0xABCDEF123456  "), "abcdef123456");
        assert_eq!(normalize_hash("ABCDEF123456"), "abcdef123456");
    }

    #[test]
    fn test_verification_status_is_success() {
        assert!(VerificationStatus::Success.is_success());
        assert!(!VerificationStatus::Mismatch {
            expected: "abc".to_string(),
            actual: "def".to_string(),
        }
        .is_success());
        assert!(!VerificationStatus::CompilationFailed("error".to_string()).is_success());
    }
}
