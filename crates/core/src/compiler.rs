//! Core WASM compilation logic

use crate::{
    artifacts::{self, ArtifactContext, BuildInfo},
    config::CompileConfig,
    contract::{self, WasmContract},
    parser, utils,
};
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// Result of compilation process
#[derive(Debug)]
pub struct CompileOutput {
    /// Successfully compiled contract
    pub result: CompilationResult,
    /// Total compilation time
    pub duration: Duration,
}

/// Complete compilation result with all outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationResult {
    /// Contract information extracted from Cargo.toml
    pub contract_info: ContractInfo,

    /// Compilation outputs
    pub outputs: CompilationOutputs,

    /// Generated artifacts (may be empty if artifact generation is disabled)
    pub artifacts: artifacts::ContractArtifacts,

    /// Build metadata for reproducibility
    pub build_metadata: BuildMetadata,
}

impl CompilationResult {
    /// Get SHA256 hash of rWASM bytecode
    pub fn rwasm_hash(&self) -> String {
        utils::hash_bytes(&self.outputs.rwasm)
    }

    /// Get SHA256 hash of WASM bytecode
    pub fn wasm_hash(&self) -> String {
        utils::hash_bytes(&self.outputs.wasm)
    }
}

/// Basic contract information from Cargo.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    /// Contract name from package.name
    pub name: String,
    /// Contract version from package.version
    pub version: String,
    /// SDK version from dependencies
    pub sdk_version: Option<String>,
}

/// Raw compilation outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationOutputs {
    /// WASM bytecode
    pub wasm: Vec<u8>,
    /// rWASM bytecode
    pub rwasm: Vec<u8>,
}

/// Build metadata for verification and reproducibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    /// Rust compiler version used
    pub rustc_version: String,
    /// Compilation timestamp
    pub timestamp: u64,
    /// Hash of all source files
    pub source_hash: String,
    /// Target triple used
    pub target: String,
    /// Build profile used
    pub profile: String,
    /// Features enabled
    pub features: Vec<String>,
    /// Whether default features were disabled
    pub no_default_features: bool,
}

/// Compiles a Rust contract project
pub fn compile(config: &CompileConfig) -> Result<CompileOutput> {
    let start = Instant::now();

    tracing::info!(
        "Starting compilation with profile={}, features={:?}",
        config.profile_name(),
        config.features
    );

    // Validate configuration
    config
        .validate()
        .context("Invalid compilation configuration")?;

    // Parse contract metadata
    let cargo_toml_path = config.project_root.join("Cargo.toml");
    let wasm_contract = contract::parse_contract_metadata(&cargo_toml_path)
        .context("Failed to parse contract metadata")?;

    let contract_info = ContractInfo {
        name: wasm_contract.name.clone(),
        version: wasm_contract.version.clone(),
        sdk_version: wasm_contract.sdk_version.clone(),
    };

    // Ensure this is a Fluent contract
    if contract_info.sdk_version.is_none() {
        return Err(eyre::eyre!("Not a Fluent contract"))
            .context("No fluentbase-sdk dependency found in Cargo.toml");
    }

    tracing::info!(
        "Compiling {} v{} (SDK: {})",
        contract_info.name,
        contract_info.version,
        contract_info.sdk_version.as_deref().unwrap_or("unknown")
    );

    // Compile to WASM
    let wasm_bytecode =
        compile_to_wasm(config, &contract_info.name).context("Failed to compile to WASM")?;

    tracing::debug!(
        "WASM compilation successful, size: {} bytes",
        wasm_bytecode.len()
    );

    // Parse source for ABI generation
    let main_source = wasm_contract
        .main_source_file()
        .context("Failed to find main source file")?;
    let routers = parser::parse_routers(&main_source).unwrap_or_else(|e| {
        tracing::warn!("Failed to parse routers: {}", e);
        vec![]
    });

    // Compile to rWASM
    let rwasm_bytecode = compile_to_rwasm(&wasm_bytecode).context("Failed to compile to rWASM")?;

    tracing::debug!(
        "rWASM compilation successful, size: {} bytes",
        rwasm_bytecode.len()
    );

    // Calculate source hash
    let source_hash =
        calculate_source_hash(&config.project_root).context("Failed to calculate source hash")?;

    // Build metadata
    let build_metadata = BuildMetadata {
        rustc_version: utils::get_rust_version(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        source_hash,
        target: config.target().to_string(),
        profile: config.profile_name().to_string(),
        features: config.features.clone(),
        no_default_features: config.no_default_features,
    };

    // Generate artifacts if enabled
    let artifacts = if should_generate_artifacts(&config.artifacts) {
        generate_artifacts(
            config,
            &wasm_contract,
            &wasm_bytecode,
            &rwasm_bytecode,
            &routers,
            &build_metadata,
        )
        .context("Failed to generate artifacts")?
    } else {
        // Return empty artifacts
        artifacts::ContractArtifacts {
            abi: vec![],
            interface: String::new(),
            metadata: Default::default(),
        }
    };

    let result = CompilationResult {
        contract_info,
        outputs: CompilationOutputs {
            wasm: wasm_bytecode,
            rwasm: rwasm_bytecode,
        },
        artifacts,
        build_metadata,
    };

    let duration = start.elapsed();
    tracing::info!(
        "Compilation completed successfully in {:.2}s",
        duration.as_secs_f64()
    );

    Ok(CompileOutput { result, duration })
}

/// Generate artifacts for the compiled contract
fn generate_artifacts(
    config: &CompileConfig,
    wasm_contract: &WasmContract,
    wasm_bytecode: &[u8],
    rwasm_bytecode: &[u8],
    routers: &[fluentbase_sdk_derive_core::router::Router],
    build_metadata: &BuildMetadata,
) -> Result<artifacts::ContractArtifacts> {
    let build_info = BuildInfo {
        rustc_version: build_metadata.rustc_version.clone(),
        target: build_metadata.target.clone(),
        profile: build_metadata.profile.clone(),
        features: build_metadata.features.clone(),
        source_hash: build_metadata.source_hash.clone(),
        compile_config: config.clone(),
    };

    artifacts::generate(&ArtifactContext {
        name: &wasm_contract.name,
        bytecode: wasm_bytecode,
        deployed_bytecode: rwasm_bytecode,
        routers,
        contract: wasm_contract,
        build_info,
    })
}

/// Compile project to WASM using cargo
fn compile_to_wasm(config: &CompileConfig, contract_name: &str) -> Result<Vec<u8>> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&config.project_root)
        .args(["build", "--target", config.target()]);

    // Add profile
    match config.profile_name() {
        "release" => {
            cmd.arg("--release");
        }
        "debug" => {
            // Debug is default, no flag needed
        }
        profile => {
            cmd.args(["--profile", profile]);
        }
    };

    // Add features and flags
    if config.no_default_features {
        cmd.arg("--no-default-features");
    }

    if !config.features.is_empty() {
        cmd.arg("--features").arg(config.features.join(","));
    }

    if config.locked {
        cmd.arg("--locked");
    }

    tracing::debug!("Running cargo command: {:?}", cmd);

    // Run build
    let output = cmd.output().context("Failed to execute cargo build")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        return Err(eyre::eyre!("Cargo build failed"))
            .context(format!("Exit code: {:?}", output.status.code()))
            .context(format!("stderr: {}", stderr))
            .context(format!("stdout: {}", stdout));
    }

    // Find and read the WASM file
    let wasm_path = find_wasm_output(config, contract_name)?;

    std::fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM file: {}", wasm_path.display()))
}

/// Compile WASM to rWASM
fn compile_to_rwasm(wasm_bytecode: &[u8]) -> Result<Vec<u8>> {
    let result = fluentbase_types::compile_wasm_to_rwasm(wasm_bytecode)
        .map_err(|e| eyre::eyre!("rWASM compilation error: {:?}", e))?;

    Ok(result.rwasm_bytecode.to_vec())
}

/// Find the compiled WASM file
fn find_wasm_output(config: &CompileConfig, contract_name: &str) -> Result<PathBuf> {
    let wasm_filename = format!("{}.wasm", contract_name.replace('-', "_"));

    let wasm_path = config
        .project_root
        .join("target")
        .join(config.target())
        .join(config.profile_name())
        .join(&wasm_filename);

    if !wasm_path.exists() {
        // Try to provide helpful error message
        let target_dir = config.project_root.join("target");
        let expected_dir = target_dir.join(config.target()).join(config.profile_name());

        let base_error = || eyre::eyre!("Expected WASM file not found: {}", wasm_path.display());

        if !expected_dir.exists() {
            return Err(base_error()).context(format!(
                "Build directory doesn't exist: {}",
                expected_dir.display()
            ));
        }

        return Err(base_error());
    }

    Ok(wasm_path)
}

/// Calculate SHA256 hash of all source files for reproducibility
fn calculate_source_hash(project_root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file_count = 0;

    for entry in walkdir::WalkDir::new(project_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip non-source directories
        if path.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s == "target" || s.starts_with('.') || s == "out"
        }) {
            continue;
        }

        // Include only source files
        let should_include = match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => true,
            _ => path
                .file_name()
                .map(|n| n == "Cargo.toml" || n == "Cargo.lock")
                .unwrap_or(false),
        };

        if should_include && path.is_file() {
            if let Ok(content) = std::fs::read(path) {
                hasher.update(&content);
                file_count += 1;
            }
        }
    }

    tracing::debug!("Calculated source hash from {} files", file_count);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Check if any artifacts should be generated
fn should_generate_artifacts(config: &crate::config::ArtifactsConfig) -> bool {
    config.generate_abi || config.generate_interface || config.generate_metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_contract(dir: &Path) -> Result<()> {
        fs::create_dir_all(dir.join("src"))?;

        fs::write(
            dir.join("Cargo.toml"),
            r#"
[package]
name = "test-contract"
version = "0.1.0"

[dependencies]
fluentbase-sdk = "0.1.0"
"#,
        )?;

        fs::write(
            dir.join("src/lib.rs"),
            r#"
#[no_mangle]
pub extern "C" fn deploy() {}
"#,
        )?;

        Ok(())
    }

    #[test]
    fn test_compilation_result_hashes() {
        let result = CompilationResult {
            contract_info: ContractInfo {
                name: "test".to_string(),
                version: "0.1.0".to_string(),
                sdk_version: Some("0.1.0".to_string()),
            },
            outputs: CompilationOutputs {
                wasm: vec![1, 2, 3, 4],
                rwasm: vec![5, 6, 7, 8],
            },
            artifacts: artifacts::ContractArtifacts {
                abi: vec![],
                interface: String::new(),
                metadata: Default::default(),
            },
            build_metadata: BuildMetadata {
                rustc_version: "1.75.0".to_string(),
                timestamp: 0,
                source_hash: "abc".to_string(),
                target: "wasm32-unknown-unknown".to_string(),
                profile: "release".to_string(),
                features: vec![],
                no_default_features: false,
            },
        };

        let wasm_hash = result.wasm_hash();
        let rwasm_hash = result.rwasm_hash();

        assert_eq!(wasm_hash.len(), 64); // SHA256 hex
        assert_eq!(rwasm_hash.len(), 64);
        assert_ne!(wasm_hash, rwasm_hash);
    }

    #[test]
    fn test_calculate_source_hash_deterministic() {
        let temp_dir = TempDir::new().unwrap();
        create_test_contract(temp_dir.path()).unwrap();

        let hash1 = calculate_source_hash(temp_dir.path()).unwrap();
        let hash2 = calculate_source_hash(temp_dir.path()).unwrap();

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }
}
