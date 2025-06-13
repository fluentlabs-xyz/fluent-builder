//! CLI for fluent-builder library
//!
//! Compiles and verifies Rust smart contracts for the Fluent blockchain.

mod docker;

use clap::{Parser, Subcommand};
use ethers::{
    providers::{Http, Middleware, Provider},
    types::Address,
};
use eyre::{Context, Result};
use fluent_builder::{
    build, create_verification_archive, save_artifacts, verify, ArchiveOptions,
    CompileConfig, GitInfo, VerificationStatus,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tracing::Level;

/// Fluent smart contract compiler and verifier
#[derive(Parser, Debug)]
#[command(name = "fluent-builder")]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress all logging except errors
    #[arg(short, long, global = true)]
    quiet: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile a Rust contract to WASM/rWASM
    Compile {
        /// Path to the project root
        #[arg(default_value = ".")]
        project_root: PathBuf,

        /// Output directory
        #[arg(short, long, default_value = "out")]
        output_dir: PathBuf,

        /// Build profile
        #[arg(long, default_value = "release")]
        profile: String,

        /// Space-separated list of features
        #[arg(long, value_delimiter = ' ')]
        features: Vec<String>,

        /// Do not activate default features
        #[arg(long, default_value_t = true)]
        no_default_features: bool,

        /// Allow compilation with uncommitted changes (uses archive source instead of git)
        #[arg(long)]
        allow_dirty: bool,

        /// Do not use Docker for compilation (faster but less reproducible)
        #[arg(long)]
        no_docker: bool,

        /// Output JSON to stdout
        #[arg(long)]
        json: bool,
    },

    /// Verify a deployed contract
    Verify {
        /// Path to the project root
        #[arg(default_value = ".")]
        project_root: PathBuf,

        /// Contract address
        #[arg(long)]
        address: String,

        /// Chain ID
        #[arg(long)]
        chain_id: u64,

        /// RPC endpoint
        #[arg(long)]
        rpc: String,

        /// Build profile
        #[arg(long, default_value = "release")]
        profile: String,

        /// Space-separated list of features
        #[arg(long, value_delimiter = ' ')]
        features: Vec<String>,

        /// Do not activate default features
        #[arg(long, default_value_t = true)]
        no_default_features: bool,

        /// Output JSON
        #[arg(long)]
        json: bool,
    },

    /// Docker-related utilities
    Docker {
        #[command(subcommand)]
        command: DockerCommands,
    },
}

#[derive(Subcommand, Debug)]
enum DockerCommands {
    /// Clean up old Docker images
    Clean {
        /// Number of recent images to keep
        #[arg(long, default_value = "5")]
        keep: usize,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status")]
enum Output {
    #[serde(rename = "success")]
    Success {
        #[serde(flatten)]
        data: SuccessData,
    },

    #[serde(rename = "error")]
    Error { error_type: String, message: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "command")]
enum SuccessData {
    #[serde(rename = "compile")]
    Compile {
        contract_name: String,
        rwasm_hash: String,
        wasm_size: usize,
        rwasm_size: usize,
        has_abi: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_dir: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        git_info: Option<GitInfoJson>,
        source_type: String,
    },

    #[serde(rename = "verify")]
    Verify {
        verified: bool,
        contract_name: String,
        expected_hash: String,
        actual_hash: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        abi: Option<serde_json::Value>,
        compiler_version: String,
        sdk_version: String,
    },
}

#[derive(Debug, Serialize)]
struct GitInfoJson {
    commit: String,
    branch: String,
    remote_url: String,
    is_clean: bool,
}

impl From<&GitInfo> for GitInfoJson {
    fn from(info: &GitInfo) -> Self {
        Self {
            commit: info.commit_hash_short.clone(),
            branch: info.branch.clone(),
            remote_url: info.remote_url.clone(),
            is_clean: !info.is_dirty,
        }
    }
}

fn main() {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.quiet {
        Level::ERROR
    } else if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let result = match cli.command {
        Commands::Compile {
            project_root,
            output_dir,
            profile,
            features,
            no_default_features,
            allow_dirty,
            no_docker,
            json,
        } => run_compile(
            project_root,
            output_dir,
            profile,
            features,
            no_default_features,
            allow_dirty,
            no_docker,
            json,
        ),
        Commands::Verify {
            project_root,
            address,
            chain_id,
            rpc,
            profile,
            features,
            no_default_features,
            json,
        } => {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create async runtime");
            runtime.block_on(run_verify(
                project_root,
                address,
                chain_id,
                rpc,
                profile,
                features,
                no_default_features,
                json,
            ))
        }
        Commands::Docker { command } => match command {
            DockerCommands::Clean { keep } => docker::cleanup_old_images(keep),
        },
    };

    if let Err(e) = result {
        output_error(e);
        std::process::exit(1);
    }
}

/// Early version detection for both Docker and local compilation
fn detect_project_versions(project_root: &PathBuf) -> Result<(String, String)> {
    // Read Rust version using existing function from builder
    let rust_version = fluent_builder::read_rust_toolchain_version(project_root)?;
    
    // Read SDK version using existing function from builder
    let sdk_version = fluent_builder::read_sdk_version_from_cargo_lock(project_root)?;
    
    tracing::info!("Detected Rust version: '{}'", rust_version);
    tracing::info!("Detected SDK version: '{}'", sdk_version);
    
    Ok((rust_version, sdk_version))
}

fn run_compile(
    project_root: PathBuf,
    output_dir: PathBuf,
    profile: String,
    features: Vec<String>,
    no_default_features: bool,
    allow_dirty: bool,
    no_docker: bool,
    json: bool,
) -> Result<()> {
    // Resolve project root to absolute path first
    let project_root = project_root
        .canonicalize()
        .context("Failed to resolve project path")?;
    
    // Early version detection - fail fast if prerequisites missing
    let (rust_version, sdk_version) = detect_project_versions(&project_root)?;
    
    tracing::info!("Detected Rust version: {}", rust_version);
    tracing::info!("Detected SDK version: {}", sdk_version);

    // If Docker is requested (default), run in container and exit
    if !no_docker {
        if !json {
            println!("🐳 Running compilation in Docker for reproducible builds...");
            println!("   (Use --no-docker for faster local compilation)");
            
            // Warn about non-reproducible nightly
            if rust_version == "nightly" {
                println!("⚠️  Warning: Using 'nightly' without a specific date may not be reproducible");
                println!("   Consider using 'nightly-YYYY-MM-DD' in rust-toolchain.toml");
            }
        }
        
        // Pass all CLI arguments to Docker along with detected versions
        let args: Vec<String> = std::env::args().skip(1).collect();
        return docker::run_reproducible(&project_root, &rust_version, &sdk_version, &args);
    }

    // --- Local compilation starts here ---
    
    // Create compilation config
    let mut config = CompileConfig::new(project_root);
    config.output_dir = output_dir;
    config.profile = profile;
    config.features = features;
    config.no_default_features = no_default_features;

    // Check Git repository status
    let git_info = fluent_builder::detect_git_info(&config.project_root)?;
    
    // Validate Git state unless --allow-dirty is specified
    if !allow_dirty {
        match &git_info {
            None => {
                return Err(eyre::eyre!(
                    "Project is not in a Git repository.\n\
                     Initialize a Git repository or use --allow-dirty flag."
                ));
            }
            Some(git) if git.is_dirty => {
                return Err(eyre::eyre!(
                    "Repository has {} uncommitted changes.\n\
                     \n\
                     To fix this:\n\
                     1. Commit your changes: git add . && git commit -m \"Your message\"\n\
                     2. Or stash them: git stash\n\
                     3. Or use --allow-dirty flag",
                    git.dirty_files_count
                ));
            }
            _ => {} // Clean repository, continue
        }
    }

    // Determine source type for metadata
    // - Clean Git repo → use Git source
    // - Dirty repo or --allow-dirty → use archive source
    config.use_git_source = match (&git_info, allow_dirty) {
        (Some(git), false) if !git.is_dirty => true,
        _ => false,
    };

    // Perform compilation
    let result = build(&config).context("Compilation failed")?;
    let rwasm_hash = format!("0x{:x}", Sha256::digest(&result.outputs.rwasm));

    // Output results based on format
    if json {
        output_json_results(&result, &rwasm_hash, &git_info, config.use_git_source)?;
    } else {
        output_human_results(&result, &rwasm_hash, &git_info, &config)?;
    }

    Ok(())
}

/// Output compilation results as JSON
fn output_json_results(
    result: &fluent_builder::CompilationResult,
    rwasm_hash: &str,
    git_info: &Option<GitInfo>,
    use_git_source: bool,
) -> Result<()> {
    let output = Output::Success {
        data: SuccessData::Compile {
            contract_name: result.contract.name.clone(),
            rwasm_hash: rwasm_hash.to_string(),
            wasm_size: result.outputs.wasm.len(),
            rwasm_size: result.outputs.rwasm.len(),
            has_abi: result
                .artifacts
                .as_ref()
                .map(|a| !a.abi.is_empty())
                .unwrap_or(false),
            output_dir: result.artifacts.as_ref().map(|_| {
                format!("{}.wasm", result.contract.name)
            }),
            git_info: git_info.as_ref().map(GitInfoJson::from),
            source_type: if use_git_source { "git" } else { "archive" }.to_string(),
        },
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

/// Output compilation results in human-readable format
fn output_human_results(
    result: &fluent_builder::CompilationResult,
    rwasm_hash: &str,
    git_info: &Option<GitInfo>,
    config: &CompileConfig,
) -> Result<()> {
    // Show Git repository info if available
    if let Some(git) = git_info {
        println!("📦 Git repository: {} @ {}", git.branch, git.commit_hash_short);
        if git.is_dirty {
            println!("⚠️  Warning: Compiling with uncommitted changes (archive source)");
        }
    }

    println!("✅ Successfully compiled {}", result.contract.name);
    println!("⏱️  Compilation time: {:.2}s", result.duration.as_secs_f64());

    // If artifacts were generated, save and display them
    if let Some(artifacts) = &result.artifacts {
        let saved = save_artifacts(
            artifacts,
            &result.contract.name,
            &result.outputs.wasm,
            &result.outputs.rwasm,
            &config.output_directory(),
            &config.artifacts,
        )?;

        // Display source type from metadata
        match &artifacts.metadata.source {
            fluent_builder::Source::Git { repository, commit, .. } => {
                println!("\n📦 Source type: Git");
                println!("   Repository: {}", repository);
                println!("   Commit: {}", &commit[..8]);
            }
            fluent_builder::Source::Archive { .. } => {
                println!("\n📦 Source type: Archive");
            }
        }
        
        // Display output location and files
        println!("\n📁 Output directory: {}", saved.output_dir.display());
        println!("📄 Generated files:");
        println!("   - lib.wasm ({} bytes)", result.outputs.wasm.len());
        println!("   - lib.rwasm ({} bytes)", result.outputs.rwasm.len());
        println!("   - rWASM hash: {}", rwasm_hash);
        
        // List optional artifacts
        if saved.abi_path.is_some() {
            println!("   - abi.json");
        }
        if saved.interface_path.is_some() {
            println!("   - interface.sol");
        }
        if saved.metadata_path.is_some() {
            println!("   - metadata.json");
        }

        // Create source archive if using archive source
        if !config.use_git_source {
            let archive_path = saved.output_dir.join("sources.tar.gz");
            let archive_options = ArchiveOptions::default();
            
            create_verification_archive(
                &config.project_root,
                &archive_path,
                &archive_options,
            )?;
            println!("   - sources.tar.gz");
        }
    } else {
        // Minimal output when artifacts are disabled
        println!("\n📊 Compilation results:");
        println!("   - WASM size: {} bytes", result.outputs.wasm.len());
        println!("   - rWASM size: {} bytes", result.outputs.rwasm.len());
        println!("   - rWASM hash: {}", rwasm_hash);
        println!("\n⚠️  No artifacts saved (generation disabled in config)");
    }

    Ok(())
}

async fn run_verify(
    project_root: PathBuf,
    address: String,
    chain_id: u64,
    rpc: String,
    profile: String,
    features: Vec<String>,
    no_default_features: bool,
    json: bool,
) -> Result<()> {
    // Fetch deployed bytecode hash
    let deployed_hash = fetch_bytecode_hash(&address, &rpc, chain_id).await?;

    // Build compilation config
    // Verify always uses the provided directory as-is (no git source)
    let mut compile_config = CompileConfig::new(project_root.clone());
    compile_config.profile = profile;
    compile_config.features = features;
    compile_config.no_default_features = no_default_features;
    compile_config.use_git_source = false; // Always use archive/plain directory for verify

    // Run verification
    let verify_config = fluent_builder::VerifyConfig {
        project_root,
        deployed_bytecode_hash: deployed_hash.clone(),
        compile_config: Some(compile_config),
    };

    let verification_result = verify(verify_config).context("Verification failed")?;

    if json {
        let output = Output::Success {
            data: SuccessData::Verify {
                verified: verification_result.status.is_success(),
                contract_name: verification_result.contract_name.clone(),
                expected_hash: match &verification_result.status {
                    VerificationStatus::Success => deployed_hash.clone(),
                    VerificationStatus::Mismatch { expected, .. } => expected.clone(),
                    _ => deployed_hash.clone(),
                },
                actual_hash: match &verification_result.status {
                    VerificationStatus::Success => deployed_hash.clone(),
                    VerificationStatus::Mismatch { actual, .. } => actual.clone(),
                    _ => String::new(),
                },
                abi: if verification_result.status.is_success() {
                    verification_result
                        .compilation_result
                        .as_ref()
                        .and_then(|r| r.artifacts.as_ref())
                        .filter(|a| !a.abi.is_empty())
                        .and_then(|a| serde_json::to_value(&a.abi).ok())
                } else {
                    None
                },
                compiler_version: verification_result
                    .compilation_result
                    .as_ref()
                    .map(|r| r.runtime_info.rust.version.clone())
                    .unwrap_or_default(),
                sdk_version: verification_result
                    .compilation_result
                    .as_ref()
                    .map(|r| format!("{}-{}", r.runtime_info.sdk.tag, r.runtime_info.sdk.commit))
                    .unwrap_or_default(),
            },
        };
        println!("{}", serde_json::to_string(&output)?);
    } else {
        if verification_result.status.is_success() {
            println!("✅ Contract verified successfully!");
            println!("📝 Contract name: {}", verification_result.contract_name);
            println!("🔍 Bytecode hash matches: {}", deployed_hash);
            
            println!("\n📋 Contract details:");
            println!("   Address: {}", address);
            println!("   Chain ID: {}", chain_id);

            if let Some(result) = &verification_result.compilation_result {
                println!("\n🛠️  Build details:");
                println!("   Compiler: {}", result.runtime_info.rust.version);
                println!(
                    "   SDK version: {}-{}",
                    result.runtime_info.sdk.tag, result.runtime_info.sdk.commit
                );
            }
        } else {
            println!("❌ Verification failed!");
            println!("📝 Contract name: {}", verification_result.contract_name);

            match &verification_result.status {
                VerificationStatus::Mismatch { expected, actual } => {
                    println!("\n🔍 Hash comparison:");
                    println!("   Expected: {}", expected);
                    println!("   Actual:   {}", actual);
                }
                VerificationStatus::CompilationFailed(error) => {
                    println!("⚠️  Compilation error: {}", error);
                }
                _ => {}
            }
        }
    }

    if !verification_result.status.is_success() {
        std::process::exit(1);
    }

    Ok(())
}

/// Fetch bytecode hash from deployed contract
async fn fetch_bytecode_hash(address: &str, rpc_url: &str, chain_id: u64) -> Result<String> {
    let provider = Provider::<Http>::try_from(rpc_url).context("Failed to create provider")?;

    // Verify chain ID matches
    let network_chain_id = provider
        .get_chainid()
        .await
        .context("Failed to get chain ID")?;

    if network_chain_id.as_u64() != chain_id {
        return Err(eyre::eyre!(
            "Chain ID mismatch: expected {}, got {}",
            chain_id,
            network_chain_id
        ));
    }

    // Parse address
    let contract_address: Address = address.parse().context("Invalid contract address")?;

    // Get bytecode
    let bytecode = provider
        .get_code(contract_address, None)
        .await
        .context("Failed to fetch contract bytecode")?;

    if bytecode.is_empty() {
        return Err(eyre::eyre!("No bytecode found at address {}", address));
    }

    // Calculate hash
    let hash = format!("0x{:x}", Sha256::digest(&bytecode));
    Ok(hash)
}

fn output_error(error: eyre::Report) {
    let error_type = if error.to_string().contains("uncommitted changes") {
        "git_dirty_state"
    } else if error.to_string().contains("not in a Git repository") {
        "no_git_repository"
    } else if error.to_string().contains("Compilation failed") {
        "compilation_failed"
    } else if error.to_string().contains("Docker") {
        "docker_error"
    } else if error.to_string().contains("Failed to fetch") {
        "network_error"
    } else {
        "unknown_error"
    };

    let output = Output::Error {
        error_type: error_type.to_string(),
        message: error.to_string(),
    };

    eprintln!("{}", serde_json::to_string(&output).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::parse_from(&["fluent-builder", "compile"]);
        assert!(matches!(cli.command, Commands::Compile { .. }));

        let cli = Cli::parse_from(&[
            "fluent-builder",
            "verify",
            "--address",
            "0x123",
            "--chain-id",
            "20993",
            "--rpc",
            "https://rpc.endpoint",
        ]);
        assert!(matches!(cli.command, Commands::Verify { .. }));
    }

    #[test]
    fn test_compile_settings() {
        let cli = Cli::parse_from(&[
            "fluent-builder",
            "compile",
            "--profile",
            "debug",
            "--features",
            "test feature2",
            "--no-default-features",
        ]);

        if let Commands::Compile {
            profile,
            features,
            no_default_features,
            ..
        } = cli.command {
            assert_eq!(profile, "debug");
            assert_eq!(features, vec!["test", "feature2"]);
            assert!(no_default_features);
        }
    }

    #[test]
    fn test_allow_dirty_flag() {
        let cli = Cli::parse_from(&["fluent-builder", "compile", "--allow-dirty"]);

        if let Commands::Compile { allow_dirty, .. } = cli.command {
            assert!(allow_dirty);
        }
    }

    #[test]
    fn test_no_docker_flag() {
        let cli = Cli::parse_from(&["fluent-builder", "compile", "--no-docker"]);

        if let Commands::Compile { no_docker, .. } = cli.command {
            assert!(no_docker);
        }
    }

    #[test]
    fn test_docker_clean_command() {
        let cli = Cli::parse_from(&["fluent-builder", "docker", "clean", "--keep", "3"]);

        if let Commands::Docker { command: DockerCommands::Clean { keep } } = cli.command {
            assert_eq!(keep, 3);
        }
    }
}