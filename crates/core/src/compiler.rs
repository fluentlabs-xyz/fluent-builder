//! Core WASM compilation logic

use crate::{
    artifacts::{self, ArtifactContext, BuildInfo},
    config::CompileConfig,
    parser, utils,
};
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
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
    /// Compilation settings used
    pub settings: CompilationSettings,
}

/// Settings that affect compilation output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationSettings {
    /// Target triple
    pub target: String,
    /// Build profile
    pub profile: String,
    /// Features enabled
    pub features: Vec<String>,
    /// Whether default features were disabled
    pub no_default_features: bool,
    /// Additional cargo flags used
    pub cargo_flags: Vec<String>,
    /// RUSTFLAGS value if set
    pub rustflags: Option<String>,
}

/// Compiles a Rust contract project
pub fn compile(config: &CompileConfig) -> Result<CompileOutput> {
    let start = Instant::now();
    tracing::debug!(
        "Running cargo build with: features={:?}, no_default_features={}",
        config.wasm.features,
        config.wasm.no_default_features
    );

    // Validate configuration
    config.validate()?;

    // Extract contract info from Cargo.toml
    let contract_info = extract_contract_info(&config.project_root)?;

    // Ensure this is a Fluent contract
    if contract_info.sdk_version.is_none() {
        return Err(eyre::eyre!(
            "Not a Fluent contract - no fluentbase-sdk dependency found"
        ));
    }

    tracing::info!(
        "Compiling {} v{}",
        contract_info.name,
        contract_info.version
    );

    // Compile to WASM
    let wasm_bytecode = compile_to_wasm(config)?;

    // Parse source for ABI generation
    let main_source = find_main_source(&config.project_root)?;
    let routers = parser::parse_routers(&main_source).unwrap_or_default();

    // Compile to rWASM
    let rwasm_bytecode = compile_to_rwasm(&wasm_bytecode, &config.rwasm)?;

    // Calculate source hash
    let source_hash = calculate_source_hash(&config.project_root)?;

    // Build metadata
    let build_metadata = BuildMetadata {
        rustc_version: utils::get_rust_version(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        source_hash,
        settings: CompilationSettings {
            target: config.wasm.target.clone(),
            profile: config.wasm.profile_name().to_string(),
            features: config.wasm.features.clone(),
            no_default_features: config.wasm.no_default_features,
            cargo_flags: config.wasm.cargo_flags.clone(),
            rustflags: config.wasm.rustflags.clone(),
        },
    };

    // Generate artifacts based on configuration
    let artifacts = generate_artifacts(
        config,
        &contract_info,
        &wasm_bytecode,
        &rwasm_bytecode,
        &routers,
        &build_metadata,
    )?;

    let result = CompilationResult {
        contract_info,
        outputs: CompilationOutputs {
            wasm: wasm_bytecode,
            rwasm: rwasm_bytecode,
        },
        artifacts,
        build_metadata,
    };

    Ok(CompileOutput {
        result,
        duration: start.elapsed(),
    })
}

/// Generate artifacts based on configuration settings
fn generate_artifacts(
    config: &CompileConfig,
    contract_info: &ContractInfo,
    wasm_bytecode: &[u8],
    rwasm_bytecode: &[u8],
    routers: &[fluentbase_sdk_derive_core::router::Router],
    build_metadata: &BuildMetadata,
) -> Result<artifacts::ContractArtifacts> {
    // Check if any artifacts should be generated
    if !should_generate_artifacts(&config.artifacts) {
        // Return minimal artifacts structure
        return Ok(artifacts::ContractArtifacts {
            abi: vec![],
            interface: String::new(),
            metadata: artifacts::metadata::Metadata {
                contract_name: contract_info.name.clone(),
                abi: vec![],
                method_identifiers: BTreeMap::new(),
                bytecodes: artifacts::metadata::Bytecodes {
                    wasm: artifacts::metadata::BytecodeInfo {
                        object: String::new(),
                        hash: String::new(),
                        size: 0,
                    },
                    rwasm: artifacts::metadata::BytecodeInfo {
                        object: String::new(),
                        hash: String::new(),
                        size: 0,
                    },
                },
                build_metadata: artifacts::metadata::BuildMetadata {
                    compiler: artifacts::metadata::CompilerInfo {
                        name: "rustc".to_string(),
                        version: build_metadata.rustc_version.clone(),
                        commit: None,
                    },
                    language: "Rust".to_string(),
                    output: artifacts::metadata::BuildOutputInfo {
                        wasm: artifacts::metadata::WasmArtifactInfo {
                            hash: String::new(),
                            size: 0,
                        },
                        rwasm: artifacts::metadata::WasmArtifactInfo {
                            hash: String::new(),
                            size: 0,
                        },
                    },
                    settings: artifacts::metadata::BuildSettings {
                        target_triple: build_metadata.settings.target.clone(),
                        profile: build_metadata.settings.profile.clone(),
                        features: build_metadata.settings.features.clone(),
                        no_default_features: build_metadata.settings.no_default_features,
                        contract_info: artifacts::metadata::ContractBuildInfo {
                            path_to_cargo_toml: "Cargo.toml".to_string(),
                            name: contract_info.name.clone(),
                            version: contract_info.version.clone(),
                            sdk_version: contract_info.sdk_version.clone(),
                        },
                        build_time_utc_seconds: build_metadata.timestamp,
                        cargo_flags_used: build_metadata.settings.cargo_flags.clone(),
                        rustflags: build_metadata.settings.rustflags.clone(),
                    },
                    sources: BTreeMap::new(),
                    metadata_format_version: 1,
                },
            },
        });
    }

    // Generate full artifacts
    let build_info = BuildInfo {
        rustc_version: build_metadata.rustc_version.clone(),
        target: build_metadata.settings.target.clone(),
        profile: build_metadata.settings.profile.clone(),
        features: build_metadata.settings.features.clone(),
        source_hash: build_metadata.source_hash.clone(),
        compile_config: config.clone(),
    };

    let wasm_contract = create_wasm_contract(&config.project_root, contract_info);

    artifacts::generate(&ArtifactContext {
        name: &contract_info.name,
        bytecode: wasm_bytecode,
        deployed_bytecode: rwasm_bytecode,
        routers,
        contract: &wasm_contract,
        build_info,
    })
}

/// Extract contract information from Cargo.toml
fn extract_contract_info(project_root: &Path) -> Result<ContractInfo> {
    let cargo_toml_path = project_root.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("Failed to read {}", cargo_toml_path.display()))?;

    let cargo_toml: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", cargo_toml_path.display()))?;

    // Extract package info
    let package = cargo_toml
        .get("package")
        .and_then(|p| p.as_table())
        .ok_or_else(|| eyre::eyre!("No [package] section found in Cargo.toml"))?;

    let name = package
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| eyre::eyre!("No package.name found"))?
        .to_string();

    let version = package
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.1.0")
        .to_string();

    // Extract SDK version
    let sdk_version = cargo_toml
        .get("dependencies")
        .and_then(|d| d.as_table())
        .and_then(|deps| deps.get("fluentbase-sdk"))
        .and_then(extract_dependency_version);

    Ok(ContractInfo {
        name,
        version,
        sdk_version,
    })
}

/// Compile project to WASM
fn compile_to_wasm(config: &CompileConfig) -> Result<Vec<u8>> {
    tracing::debug!(
        "Running cargo build with: features={:?}, no_default_features={}",
        config.wasm.features,
        config.wasm.no_default_features
    );
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&config.project_root)
        .args(["build", "--target", &config.wasm.target]);

    // Add profile
    match config.wasm.profile_name() {
        "release" => cmd.arg("--release"),
        "debug" => &cmd, // Debug is default
        profile => cmd.args(["--profile", profile]),
    };

    // Add features and flags
    if config.wasm.no_default_features {
        cmd.arg("--no-default-features");
    }

    if !config.wasm.features.is_empty() {
        cmd.arg("--features").arg(config.wasm.features.join(","));
    }

    if config.wasm.locked {
        cmd.arg("--locked");
    }

    // Add custom cargo flags
    cmd.args(&config.wasm.cargo_flags);

    // Set RUSTFLAGS if needed
    if let Some(rustflags) = config.wasm.build_rustflags() {
        cmd.env("RUSTFLAGS", rustflags);
    }

    // Run build
    let output = cmd.output().with_context(|| "Failed to run cargo build")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre::eyre!("Cargo build failed:\n{}", stderr));
    }

    // Find and read the WASM file
    let wasm_path = find_wasm_output(config)?;
    std::fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM file: {}", wasm_path.display()))
}

/// Compile WASM to rWASM
fn compile_to_rwasm(wasm_bytecode: &[u8], _config: &crate::config::RwasmConfig) -> Result<Vec<u8>> {
    // TODO(d1r1): do we actually need to use our custom RWASM config here?
    let result = fluentbase_types::compile_wasm_to_rwasm(wasm_bytecode)
        .map_err(|e| eyre::eyre!("Failed to compile to rWASM: {:?}", e))?;

    Ok(result.rwasm_bytecode.to_vec())
}

/// Find the compiled WASM file
fn find_wasm_output(config: &CompileConfig) -> Result<PathBuf> {
    // Extract package name for the output file
    let contract_info = extract_contract_info(&config.project_root)?;
    let wasm_filename = format!("{}.wasm", contract_info.name.replace('-', "_"));

    let wasm_path = config
        .project_root
        .join("target")
        .join(&config.wasm.target)
        .join(config.wasm.profile_name())
        .join(wasm_filename);

    if !wasm_path.exists() {
        return Err(eyre::eyre!(
            "Expected WASM file not found at: {}",
            wasm_path.display()
        ));
    }

    Ok(wasm_path)
}

/// Find main source file (lib.rs or main.rs)
fn find_main_source(project_root: &Path) -> Result<PathBuf> {
    let candidates = [
        project_root.join("src/lib.rs"),
        project_root.join("src/main.rs"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    Err(eyre::eyre!(
        "No main source file found. Expected src/lib.rs or src/main.rs"
    ))
}

/// Calculate hash of all source files
fn calculate_source_hash(project_root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();

    for entry in walkdir::WalkDir::new(project_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip target directory and hidden directories
        if path.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s == "target" || s.starts_with('.')
        }) {
            continue;
        }

        // Include only relevant files
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
            }
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Check if any artifacts should be generated
fn should_generate_artifacts(config: &crate::config::ArtifactsConfig) -> bool {
    config.generate_abi || config.generate_interface || config.generate_metadata
}

/// Extract version from dependency value
fn extract_dependency_version(dep_value: &toml::Value) -> Option<String> {
    match dep_value {
        toml::Value::String(version) => Some(version.clone()),
        toml::Value::Table(table) => {
            if let Some(toml::Value::String(version)) = table.get("version") {
                Some(version.clone())
            } else if table.contains_key("path") {
                Some("path".to_string())
            } else if table.contains_key("git") {
                Some("git".to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Helper function for backward compatibility
fn create_wasm_contract(project_root: &Path, info: &ContractInfo) -> crate::contract::WasmContract {
    crate::contract::WasmContract {
        path: project_root.to_path_buf(),
        name: info.name.clone(),
        version: info.version.clone(),
        sdk_version: info.sdk_version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_contract(dir: &Path) -> Result<()> {
        // Create minimal contract structure
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
    fn test_extract_contract_info() {
        let temp_dir = TempDir::new().unwrap();
        create_test_contract(temp_dir.path()).unwrap();

        let info = extract_contract_info(temp_dir.path()).unwrap();
        assert_eq!(info.name, "test-contract");
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.sdk_version, Some("0.1.0".to_string()));
    }

    #[test]
    fn test_calculate_source_hash() {
        let temp_dir = TempDir::new().unwrap();
        create_test_contract(temp_dir.path()).unwrap();

        let hash1 = calculate_source_hash(temp_dir.path()).unwrap();
        let hash2 = calculate_source_hash(temp_dir.path()).unwrap();

        // Hash should be deterministic
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
        assert_eq!(hash1.len(), 64); // SHA256 produces 64 hex chars
    }

    #[test]
    fn test_calculate_source_hash_changes() {
        let temp_dir = TempDir::new().unwrap();
        create_test_contract(temp_dir.path()).unwrap();

        let hash1 = calculate_source_hash(temp_dir.path()).unwrap();

        // Modify a source file
        fs::write(
            temp_dir.path().join("src/lib.rs"),
            r#"
#[no_mangle]
pub extern "C" fn deploy() {
    // Modified content
}
"#,
        )
        .unwrap();

        let hash2 = calculate_source_hash(temp_dir.path()).unwrap();

        // Hash should change when source changes
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_find_main_source() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        // Test with lib.rs
        fs::write(temp_dir.path().join("src/lib.rs"), "").unwrap();
        let source = find_main_source(temp_dir.path()).unwrap();
        assert_eq!(source.file_name().unwrap(), "lib.rs");

        // Test with main.rs
        fs::remove_file(temp_dir.path().join("src/lib.rs")).unwrap();
        fs::write(temp_dir.path().join("src/main.rs"), "").unwrap();
        let source = find_main_source(temp_dir.path()).unwrap();
        assert_eq!(source.file_name().unwrap(), "main.rs");

        // Test with no source
        fs::remove_file(temp_dir.path().join("src/main.rs")).unwrap();
        let result = find_main_source(temp_dir.path());
        assert!(result.is_err());
    }
}
