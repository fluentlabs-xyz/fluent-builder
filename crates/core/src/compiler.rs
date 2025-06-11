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
    pub version: String, // Full version string like "rustc 1.75.0 (82e1608df 2023-12-21)"
    pub commit: String,  // Extracted commit like "82e1608df"
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
pub fn compile(config: &CompileConfig) -> Result<CompilationResult> {
    let start = std::time::Instant::now();

    // Validate configuration
    config.validate()?;

    // Parse contract name and version from Cargo.toml
    let cargo_toml_path = config.project_root.join("Cargo.toml");
    let (name, version) = parse_cargo_toml(&cargo_toml_path)?;

    let contract = ContractInfo {
        name: name.clone(),
        version,
    };

    // CRITICAL: Get SDK version from Cargo.lock - no defaults
    let sdk_version_string = read_sdk_version_from_cargo_lock(&config.project_root).context(
        "Failed to read SDK version from Cargo.lock. Is the project built with 'cargo build'?",
    )?;

    let sdk = parse_sdk_version(&sdk_version_string);

    tracing::info!(
        "Compiling {} v{} (SDK: {})",
        contract.name,
        contract.version,
        sdk_version_string
    );

    // Detect Git information
    let git_info = crate::git::detect_git_info(&config.project_root)?;

    // Log Git status if available
    if let Some(git) = &git_info {
        tracing::info!(
            "Git: {} @ {} ({})",
            git.branch,
            git.commit_hash_short,
            if git.is_dirty {
                format!("{} uncommitted changes", git.dirty_files_count)
            } else {
                "clean".to_string()
            }
        );

        // Warn if repository is dirty
        if git.is_dirty {
            tracing::warn!(
                "Git repository has {} uncommitted changes. \
                 Contract verification may fail due to source mismatch.",
                git.dirty_files_count
            );
        }
    } else {
        tracing::debug!("No Git repository detected");
    }

    // Compile to WASM
    let wasm_bytecode = compile_to_wasm(config, &name)?;
    tracing::info!("WASM size: {} bytes", wasm_bytecode.len());

    // Compile to rWASM
    let rwasm_bytecode = compile_to_rwasm(&wasm_bytecode)?;
    tracing::info!("rWASM size: {} bytes", rwasm_bytecode.len());

    // CRITICAL: Detect Rust version - no defaults
    let rustc_version = detect_rust_version()
        .ok_or_else(|| eyre::eyre!("Could not detect Rust compiler version. Is rustc in PATH?"))?;

    let rust = RustInfo {
        version: rustc_version.clone(),
        commit: extract_rustc_commit(&rustc_version),
        target: config.target().to_string(),
    };

    // Calculate source hash for reproducibility
    let source_tree_hash = calculate_source_hash(&config.project_root)?;

    let runtime_info = RuntimeInfo {
        rust,
        sdk,
        built_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        source_tree_hash,
    };

    // Generate artifacts if requested
    let artifacts = if should_generate_artifacts(&config.artifacts) {
        // Find main source file and parse routers
        let main_source = find_main_source(&config.project_root)?;
        let routers = parser::parse_routers(&main_source).unwrap_or_else(|e| {
            tracing::warn!("Failed to parse routers: {}", e);
            vec![]
        });

        // Determine source type based on Git availability and state
        let source = if let Some(git) = &git_info {
            if git.is_dirty {
                // If repository is dirty, fall back to archive mode
                tracing::debug!("Using archive source due to uncommitted changes");
                artifacts::metadata::Source::Archive {
                    archive_path: "./source.tar.gz".to_string(),
                    project_path: ".".to_string(),
                }
            } else {
                // Clean Git repository - use Git source
                let project_path = crate::git::get_project_path_in_repo(&config.project_root)
                    .unwrap_or_else(|e| {
                        tracing::warn!("Failed to get project path in repo: {}", e);
                        ".".to_string()
                    });

                tracing::debug!(
                    "Using Git source: {} @ {}",
                    git.remote_url,
                    git.commit_hash_short
                );
                artifacts::metadata::Source::Git {
                    repository: git.remote_url.clone(),
                    commit: git.commit_hash.clone(),
                    project_path,
                }
            }
        } else {
            // No Git repository - use archive
            artifacts::metadata::Source::Archive {
                archive_path: "./source.tar.gz".to_string(),
                project_path: ".".to_string(),
            }
        };

        Some(artifacts::generate(
            &contract,
            &wasm_bytecode,
            &rwasm_bytecode,
            &routers,
            &config.project_root,
            config,
            &runtime_info,
            source,
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

/// Parse contract name and version from Cargo.toml
fn parse_cargo_toml(cargo_toml_path: &Path) -> Result<(String, String)> {
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

    // Check that this is a Fluent contract (has SDK dependency)
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

    Ok((name, version))
}

/// Read SDK version from Cargo.lock - CRITICAL for reproducibility
fn read_sdk_version_from_cargo_lock(project_root: &Path) -> Result<String> {
    let cargo_lock_path = project_root.join("Cargo.lock");

    if !cargo_lock_path.exists() {
        return Err(eyre::eyre!(
            "Cargo.lock not found. Run 'cargo build' first to generate it."
        ));
    }

    let content = std::fs::read_to_string(&cargo_lock_path).context("Failed to read Cargo.lock")?;

    let lock_file: toml::Value = toml::from_str(&content).context("Failed to parse Cargo.lock")?;

    // Find fluentbase-sdk in packages
    let packages = lock_file
        .get("package")
        .and_then(|p| p.as_array())
        .ok_or_else(|| eyre::eyre!("Invalid Cargo.lock format: no [[package]] entries"))?;

    for package in packages {
        if package.get("name").and_then(|n| n.as_str()) == Some("fluentbase-sdk") {
            let version = package
                .get("version")
                .and_then(|v| v.as_str())
                .ok_or_else(|| eyre::eyre!("fluentbase-sdk found but has no version"))?;

            // If from git, append commit hash
            if let Some(source) = package.get("source").and_then(|s| s.as_str()) {
                if source.starts_with("git+") {
                    if let Some(hash) = source.split('#').nth(1) {
                        return Ok(format!("{}-{}", version, hash));
                    }
                }
            }

            return Ok(version.to_string());
        }
    }

    Err(eyre::eyre!(
        "fluentbase-sdk not found in Cargo.lock. Is it listed in dependencies?"
    ))
}

/// Parse SDK version into components
fn parse_sdk_version(version: &str) -> SdkInfo {
    if let Some((tag, commit)) = version.split_once('-') {
        SdkInfo {
            tag: tag.to_string(),
            commit: commit.to_string(),
        }
    } else {
        SdkInfo {
            tag: version.to_string(),
            commit: "unknown".to_string(),
        }
    }
}

/// Extract rustc commit from version string
fn extract_rustc_commit(version: &str) -> String {
    // Format: "rustc 1.75.0 (82e1608df 2023-12-21)"
    let parts: Vec<&str> = version.split(' ').collect();
    if parts.len() >= 3 {
        parts[2].trim_start_matches('(').to_string()
    } else {
        "unknown".to_string()
    }
}

/// Find the main source file (src/lib.rs or src/main.rs)
fn find_main_source(project_root: &Path) -> Result<PathBuf> {
    let lib_rs = project_root.join("src/lib.rs");
    if lib_rs.exists() {
        return Ok(lib_rs);
    }

    let main_rs = project_root.join("src/main.rs");
    if main_rs.exists() {
        return Ok(main_rs);
    }

    Err(eyre::eyre!(
        "No main source file found. Expected src/lib.rs or src/main.rs"
    ))
}

/// Compile Rust project to WASM using cargo
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
            "Expected WASM file not found: {}. \
             Make sure the crate type is 'cdylib' in Cargo.toml \
             and the package name matches the crate name.",
            wasm_path.display()
        ));
    }

    std::fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM file: {}", wasm_path.display()))
}

/// Convert WASM to rWASM using Fluent's compiler
fn compile_to_rwasm(wasm_bytecode: &[u8]) -> Result<Vec<u8>> {
    let result = fluentbase_types::compile_wasm_to_rwasm(wasm_bytecode)
        .map_err(|e| eyre::eyre!("rWASM compilation failed: {:?}", e))?;

    Ok(result.rwasm_bytecode.to_vec())
}

/// Detect the Rust compiler version - CRITICAL: no defaults
fn detect_rust_version() -> Option<String> {
    let output = Command::new("rustc").arg("--version").output().ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

/// Calculate SHA256 hash of all source files
fn calculate_source_hash(project_root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file_count = 0;

    for entry in WalkDir::new(project_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip build outputs and hidden directories
        if path.components().any(|c| {
            matches!(c.as_os_str().to_str(), Some("target" | "out"))
                || c.as_os_str().to_string_lossy().starts_with('.')
        }) {
            continue;
        }

        // Include only source files
        if path.is_file() {
            let include = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => true,
                _ => path
                    .file_name()
                    .map(|n| n == "Cargo.toml" || n == "Cargo.lock")
                    .unwrap_or(false),
            };

            if include {
                let content = std::fs::read(path)?;
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
