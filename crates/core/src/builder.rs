//! Core WASM compilation logic

use crate::{artifacts, config::CompileConfig, parser};
use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use walkdir::WalkDir;

/// Result of successful compilation
#[derive(Debug)]
pub struct CompilationResult {
    /// Contract information from Cargo.toml
    pub contract: ContractInfo,
    /// Raw bytecode outputs
    pub outputs: CompilationOutputs,
    /// Generated artifacts (None if generation is disabled)
    pub artifacts: Option<artifacts::ContractArtifacts>,
    /// Runtime information detected during build
    pub runtime_info: RuntimeInfo,
    /// Total compilation time
    pub duration: Duration,
}

/// Contract information from Cargo.toml (static info)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    pub name: String,
    pub version: String,
}

/// Runtime information detected during compilation
#[derive(Debug, Clone)]
pub struct RuntimeInfo {
    /// Rust compiler info
    pub rust: RustInfo,
    /// SDK version info
    pub sdk: SdkInfo,
    /// Build timestamp
    pub built_at: u64,
    /// Source tree hash
    pub source_tree_hash: String,
}

/// Rust compiler information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustInfo {
    pub version: String, // Version from rust-toolchain.toml like "1.83.0" or "nightly-2024-01-15"
    pub target: String,  // Always "wasm32-unknown-unknown" for now
}

/// SDK version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkInfo {
    pub tag: String,    // Version tag like "0.1.0"
    pub commit: String, // Git commit hash or "unknown"
}

/// Compiled bytecode outputs
#[derive(Debug, Clone)]
pub struct CompilationOutputs {
    pub wasm: Vec<u8>,
    pub rwasm: Vec<u8>,
}

/// Compile a Rust smart contract to WASM and rWASM
pub fn build(config: &CompileConfig) -> Result<CompilationResult> {
    let start = std::time::Instant::now();

    // Validate configuration
    config.validate()?;

    // Parse contract metadata and validate it's a Fluent contract
    let cargo_toml_path = config.project_root.join("Cargo.toml");
    let contract = parse_contract_info(&cargo_toml_path)?;

    // Get SDK version from Cargo.lock
    let sdk_version_string = read_sdk_version_from_cargo_lock(&config.project_root)?;
    let sdk = parse_sdk_version(&sdk_version_string);

    tracing::info!(
        "Compiling {} v{} (SDK: {})",
        contract.name,
        contract.version,
        sdk_version_string
    );

    // Detect Git information for source tracking
    let git_info = crate::git::detect_git_info(&config.project_root)?;
    log_git_status(&git_info);

    // Compile to WASM
    let wasm_bytecode = compile_to_wasm(config, &contract.name)?;
    tracing::info!("WASM size: {} bytes", wasm_bytecode.len());

    // Compile to rWASM
    let rwasm_bytecode = compile_to_rwasm(&wasm_bytecode)?;
    tracing::info!("rWASM size: {} bytes", rwasm_bytecode.len());

    // Read Rust version from rust-toolchain.toml
    let rust_version = read_rust_toolchain_version(&config.project_root)?;
    let rust = RustInfo {
        version: rust_version,
        target: config.target().to_string(),
    };

    // Build runtime info
    let runtime_info = RuntimeInfo {
        rust,
        sdk,
        built_at: current_timestamp(),
        source_tree_hash: calculate_source_hash(&config.project_root)?,
    };

    // Generate artifacts if requested
    let artifacts = if should_generate_artifacts(&config.artifacts) {
        Some(generate_contract_artifacts(
            &contract,
            &wasm_bytecode,
            &rwasm_bytecode,
            &cargo_toml_path,
            config,
            &runtime_info,
            &git_info,
        )?)
    } else {
        None
    };

    let duration = start.elapsed();
    tracing::info!("Compilation completed in {:.2}s", duration.as_secs_f64());

    Ok(CompilationResult {
        contract,
        outputs: CompilationOutputs {
            wasm: wasm_bytecode,
            rwasm: rwasm_bytecode,
        },
        artifacts,
        runtime_info,
        duration,
    })
}

/// Parse contract name and version from Cargo.toml and validate it's a Fluent contract
fn parse_contract_info(cargo_toml_path: &Path) -> Result<ContractInfo> {
    let content = std::fs::read_to_string(cargo_toml_path)
        .with_context(|| format!("Failed to read {}", cargo_toml_path.display()))?;

    let cargo_toml: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", cargo_toml_path.display()))?;

    // Extract package info
    let package = cargo_toml
        .get("package")
        .and_then(|p| p.as_table())
        .ok_or_else(|| eyre::eyre!("No [package] section in Cargo.toml"))?;

    let name = package
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| eyre::eyre!("No package.name in Cargo.toml"))?
        .to_string();

    let version = package
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| eyre::eyre!("No package.version in Cargo.toml"))?
        .to_string();

    // Validate it's a Fluent contract
    let has_sdk = cargo_toml
        .get("dependencies")
        .and_then(|d| d.as_table())
        .map(|deps| deps.contains_key("fluentbase-sdk"))
        .unwrap_or(false);

    if !has_sdk {
        return Err(eyre::eyre!(
            "Not a Fluent contract: no fluentbase-sdk dependency found in Cargo.toml"
        ));
    }

    Ok(ContractInfo { name, version })
}

/// Read SDK version from Cargo.lock
pub fn read_sdk_version_from_cargo_lock(project_root: &Path) -> Result<String> {
    let cargo_lock_path = project_root.join("Cargo.lock");

    if !cargo_lock_path.exists() {
        return Err(eyre::eyre!(
            "Cargo.lock not found. Run 'cargo build' first to generate it."
        ));
    }

    let content = std::fs::read_to_string(&cargo_lock_path)?;
    let lock_file: toml::Value = toml::from_str(&content)?;

    let packages = lock_file
        .get("package")
        .and_then(|p| p.as_array())
        .ok_or_else(|| eyre::eyre!("Invalid Cargo.lock format"))?;

    for package in packages {
        if package.get("name").and_then(|n| n.as_str()) == Some("fluentbase-sdk") {
            let version = package
                .get("version")
                .and_then(|v| v.as_str())
                .ok_or_else(|| eyre::eyre!("fluentbase-sdk found but has no version"))?;

            // If from git, append commit hash
            if let Some(source) = package.get("source").and_then(|s| s.as_str()) {
                if let Some(hash) = source
                    .strip_prefix("git+")
                    .and_then(|s| s.split('#').nth(1))
                {
                    return Ok(format!("{}-{}", version, &hash[..8.min(hash.len())]));
                }
            }

            return Ok(version.to_string());
        }
    }

    Err(eyre::eyre!("fluentbase-sdk not found in Cargo.lock"))
}

/// Parse SDK version into components
fn parse_sdk_version(version: &str) -> SdkInfo {
    match version.split_once('-') {
        Some((tag, commit)) => SdkInfo {
            tag: tag.to_string(),
            commit: commit.to_string(),
        },
        None => SdkInfo {
            tag: version.to_string(),
            commit: "unknown".to_string(),
        },
    }
}

/// Read Rust version from rust-toolchain.toml
pub fn read_rust_toolchain_version(project_root: &Path) -> Result<String> {
    // Try rust-toolchain.toml first
    let toolchain_toml = project_root.join("rust-toolchain.toml");
    if toolchain_toml.exists() {
        let content = std::fs::read_to_string(&toolchain_toml)?;
        let toml_value: toml::Value = toml::from_str(&content)?;

        let channel = toml_value
            .get("toolchain")
            .and_then(|t| t.get("channel"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                eyre::eyre!("Invalid rust-toolchain.toml: missing [toolchain].channel")
            })?;

        validate_rust_version(channel)?;
        return Ok(channel.to_string());
    }

    // Try legacy rust-toolchain file
    let legacy_toolchain = project_root.join("rust-toolchain");
    if legacy_toolchain.exists() {
        let channel = std::fs::read_to_string(&legacy_toolchain)?
            .trim()
            .to_string();
        validate_rust_version(&channel)?;
        return Ok(channel);
    }

    Err(eyre::eyre!(
        "No rust-toolchain.toml found in project root: {}.\n\
         For reproducible builds, create a rust-toolchain.toml file with:\n\
         [toolchain]\n\
         channel = \"1.83.0\"",
        project_root.display()
    ))
}

/// Validate that Rust version is pinned
fn validate_rust_version(channel: &str) -> Result<()> {
    if channel.is_empty() {
        return Err(eyre::eyre!("Rust toolchain channel cannot be empty"));
    }

    if ["stable", "beta", "nightly"].contains(&channel) {
        return Err(eyre::eyre!(
            "Rust toolchain must specify a pinned version for reproducible builds.\n\
             Found '{}' but expected a specific version like '1.83.0' or 'nightly-2024-01-15'",
            channel
        ));
    }

    Ok(())
}

/// Find the main source file, respecting custom paths in Cargo.toml
fn find_main_source(project_root: &Path, cargo_toml_path: &Path) -> Result<PathBuf> {
    let content = std::fs::read_to_string(cargo_toml_path)
        .with_context(|| format!("Failed to read {}", cargo_toml_path.display()))?;

    let cargo_toml: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", cargo_toml_path.display()))?;

    // Check for custom lib path
    if let Some(lib_path) = cargo_toml
        .get("lib")
        .and_then(|lib| lib.get("path"))
        .and_then(|path| path.as_str())
    {
        let custom_path = project_root.join(lib_path);
        if custom_path.exists() {
            return Ok(custom_path);
        }
        return Err(eyre::eyre!(
            "Custom lib path not found: {}",
            custom_path.display()
        ));
    }

    // Check standard and flat structure locations
    const POSSIBLE_PATHS: &[&str] = &["src/lib.rs", "src/main.rs", "lib.rs", "main.rs"];

    for path in POSSIBLE_PATHS {
        let full_path = project_root.join(path);
        if full_path.exists() {
            return Ok(full_path);
        }
    }

    Err(eyre::eyre!(
        "No main source file found. Expected one of: {}",
        POSSIBLE_PATHS.join(", ")
    ))
}

/// Compile Rust project to WASM
fn compile_to_wasm(config: &CompileConfig, contract_name: &str) -> Result<Vec<u8>> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&config.project_root)
        .args(["build", "--target", config.target()]);

    // Add profile
    match config.profile.as_str() {
        "release" => cmd.arg("--release"),
        "debug" => &cmd,
        profile => cmd.args(["--profile", profile]),
    };

    // Add features
    if config.no_default_features {
        cmd.arg("--no-default-features");
    }
    if !config.features.is_empty() {
        cmd.arg("--features").arg(config.features.join(","));
    }
    if config.locked {
        cmd.arg("--locked");
    }

    tracing::debug!("Running: {:?}", cmd);

    let output = cmd.output().context("Failed to execute cargo build")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre::eyre!("Cargo build failed:\n{}", stderr));
    }

    // Find the compiled WASM file
    let wasm_filename = format!("{}.wasm", contract_name.replace('-', "_"));
    let wasm_path = config
        .project_root
        .join("target")
        .join(config.target())
        .join(&config.profile)
        .join(&wasm_filename);

    if !wasm_path.exists() {
        return Err(eyre::eyre!(
            "Expected WASM file not found: {}.\n\
             Ensure crate-type includes 'cdylib' in Cargo.toml",
            wasm_path.display()
        ));
    }

    std::fs::read(&wasm_path).with_context(|| format!("Failed to read {}", wasm_path.display()))
}

/// Convert WASM to rWASM
fn compile_to_rwasm(wasm_bytecode: &[u8]) -> Result<Vec<u8>> {
    let result = fluentbase_types::compile_wasm_to_rwasm(wasm_bytecode)
        .map_err(|e| eyre::eyre!("rWASM compilation failed: {:?}", e))?;
    Ok(result.rwasm_bytecode.to_vec())
}

/// Calculate SHA256 hash of source files
fn calculate_source_hash(project_root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file_count = 0;

    // Files to include in hash
    const INCLUDE_EXTENSIONS: &[&str] = &["rs"];
    const INCLUDE_FILES: &[&str] = &[
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "rust-toolchain",
    ];

    for entry in WalkDir::new(project_root)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        // Skip build outputs and hidden directories
        if should_skip_path(path) {
            continue;
        }

        let should_include = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| INCLUDE_EXTENSIONS.contains(&ext))
            .unwrap_or(false)
            || path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|name| INCLUDE_FILES.contains(&name))
                .unwrap_or(false);

        if should_include {
            let content = std::fs::read(path)?;
            hasher.update(&content);
            file_count += 1;
        }
    }

    tracing::debug!("Calculated source hash from {} files", file_count);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Check if path should be skipped for source hashing
fn should_skip_path(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map(|s| s == "target" || s == "out" || s.starts_with('.'))
            .unwrap_or(false)
    })
}

/// Generate contract artifacts
fn generate_contract_artifacts(
    contract: &ContractInfo,
    wasm_bytecode: &[u8],
    rwasm_bytecode: &[u8],
    cargo_toml_path: &Path,
    config: &CompileConfig,
    runtime_info: &RuntimeInfo,
    git_info: &Option<crate::GitInfo>,
) -> Result<artifacts::ContractArtifacts> {
    // Find and parse routers
    let main_source = find_main_source(&config.project_root, cargo_toml_path)?;
    let routers = parser::parse_routers(&main_source).unwrap_or_else(|e| {
        tracing::warn!("Failed to parse routers: {}", e);
        vec![]
    });

    // Determine source type
    let source = determine_source_type(&config.project_root, git_info);

    artifacts::generate(
        contract,
        wasm_bytecode,
        rwasm_bytecode,
        &routers,
        &config.project_root,
        config,
        runtime_info,
        source,
    )
}

/// Determine source type based on Git state
fn determine_source_type(
    project_root: &Path,
    git_info: &Option<crate::GitInfo>,
) -> artifacts::metadata::Source {
    match git_info {
        Some(git) if !git.is_dirty => {
            let project_path = crate::git::get_project_path_in_repo(project_root)
                .unwrap_or_else(|_| ".".to_string());

            artifacts::metadata::Source::Git {
                repository: git.remote_url.clone(),
                commit: git.commit_hash.clone(),
                project_path,
            }
        }
        _ => artifacts::metadata::Source::Archive {
            archive_path: "./source.tar.gz".to_string(),
            project_path: ".".to_string(),
        },
    }
}

/// Log Git repository status
fn log_git_status(git_info: &Option<crate::GitInfo>) {
    match git_info {
        Some(git) => {
            let status = if git.is_dirty {
                format!("{} uncommitted changes", git.dirty_files_count)
            } else {
                "clean".to_string()
            };
            tracing::info!(
                "Git: {} @ {} ({})",
                git.branch,
                git.commit_hash_short,
                status
            );

            if git.is_dirty {
                tracing::warn!(
                    "Repository has uncommitted changes. \
                     Contract verification may fail due to source mismatch."
                );
            }
        }
        None => tracing::debug!("No Git repository detected"),
    }
}

/// Get current timestamp
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Check if any artifacts should be generated
fn should_generate_artifacts(config: &crate::config::ArtifactsConfig) -> bool {
    config.generate_abi || config.generate_interface || config.generate_metadata
}

/// Hash bytes to SHA256 hex string
pub fn hash_bytes(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// Get rWASM hash from compilation result
pub fn get_rwasm_hash(result: &CompilationResult) -> String {
    hash_bytes(&result.outputs.rwasm)
}

/// Get WASM hash from compilation result
pub fn get_wasm_hash(result: &CompilationResult) -> String {
    hash_bytes(&result.outputs.wasm)
}
