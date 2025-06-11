//! CLI for fluent-compiler library
//!
//! Compiles and verifies Rust smart contracts for the Fluent blockchain.

use clap::{Parser, Subcommand};
use ethers::{
    providers::{Http, Middleware, Provider},
    types::Address,
};
use eyre::{Context, Result};
use fluent_compiler::{
    compile, create_verification_archive, save_artifacts, verify, ArchiveFormat, ArchiveOptions,
    CompileConfig, GitInfo, VerificationStatus,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tracing::Level;

/// Fluent smart contract compiler and verifier
#[derive(Parser, Debug)]
#[command(name = "fluent-compiler")]
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

/// Common compilation settings
#[derive(Parser, Debug, Clone)]
struct CompileSettings {
    /// Build profile
    #[arg(long, default_value = "release")]
    profile: String,

    /// Space-separated list of features
    #[arg(long, value_delimiter = ' ')]
    features: Vec<String>,

    /// Do not activate default features
    #[arg(long, default_value_t = true)]
    no_default_features: bool,

    /// Use Git source for verification (requires clean public repository)
    #[arg(long)]
    git_source: bool,

    /// Allow compilation with uncommitted changes (only for archive source)
    #[arg(long)]
    allow_dirty: bool,
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

        /// Create source archive
        #[arg(long)]
        archive: bool,

        /// Output JSON to stdout
        #[arg(long)]
        json: bool,

        #[command(flatten)]
        compile: CompileSettings,
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

        /// Output JSON
        #[arg(long)]
        json: bool,

        #[command(flatten)]
        compile: CompileSettings,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        git_info: Option<GitInfoJson>,
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
            archive,
            json,
            compile,
        } => run_compile(project_root, output_dir, compile, archive, json),
        Commands::Verify {
            project_root,
            address,
            chain_id,
            rpc,
            json,
            compile,
        } => {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create async runtime");
            runtime.block_on(run_verify(
                project_root,
                address,
                chain_id,
                rpc,
                compile,
                json,
            ))
        }
    };

    if let Err(e) = result {
        output_error(e);
        std::process::exit(1);
    }
}

fn run_compile(
    project_root: PathBuf,
    output_dir: PathBuf,
    settings: CompileSettings,
    archive: bool,
    json: bool,
) -> Result<()> {
    // Check Git status if using git source
    let git_info = fluent_compiler::detect_git_info(&project_root)?;
    
    // Validate git source requirements
    if settings.git_source {
        match &git_info {
            None => {
                return Err(eyre::eyre!(
                    "Cannot use --git-source: project is not in a Git repository.\n\
                     Initialize a Git repository or remove the --git-source flag."
                ));
            }
            Some(git) if git.is_dirty => {
                return Err(eyre::eyre!(
                    "Cannot use --git-source: repository has {} uncommitted changes.\n\
                     \n\
                     To fix this:\n\
                     1. Commit your changes: git add . && git commit -m \"Your message\"\n\
                     2. Or stash them: git stash\n\
                     3. Or remove --git-source flag to use archive source",
                    git.dirty_files_count
                ));
            }
            Some(git) => {
                // Additional check for public repository (optional)
                if !is_likely_public_repository(&git.remote_url) {
                    tracing::warn!(
                        "Repository '{}' may be private. Git source verification requires public access.",
                        git.remote_url
                    );
                }
            }
        }
    } else if let Some(git) = &git_info {
        // Archive mode - just warn if dirty
        if git.is_dirty && !settings.allow_dirty {
            tracing::warn!(
                "Repository has {} uncommitted changes. \
                 Consider committing for better reproducibility.",
                git.dirty_files_count
            );
        }
    }

    // Build configuration
    let mut config = CompileConfig::new(project_root);
    config.output_dir = output_dir;
    config.profile = settings.profile;
    config.features = settings.features;
    config.no_default_features = settings.no_default_features;
    config.use_git_source = settings.git_source;

    let result = compile(&config).context("Compilation failed")?;

    let rwasm_hash = format!("{:x}", Sha256::digest(&result.outputs.rwasm));

    if json {
        let output = Output::Success {
            data: SuccessData::Compile {
                contract_name: result.contract.name.clone(),
                rwasm_hash: format!("0x{}", rwasm_hash),
                wasm_size: result.outputs.wasm.len(),
                rwasm_size: result.outputs.rwasm.len(),
                has_abi: result
                    .artifacts
                    .as_ref()
                    .map(|a| !a.abi.is_empty())
                    .unwrap_or(false),
                output_dir: None,
                git_info: git_info.as_ref().map(GitInfoJson::from),
                source_type: if settings.git_source { "git" } else { "archive" }.to_string(),
            },
        };
        println!("{}", serde_json::to_string(&output)?);
    } else {
        // Print Git info if available
        if let Some(git) = &git_info {
            println!("📦 Git repository: {} @ {}", git.branch, git.commit_hash_short);
            if git.is_dirty {
                println!("⚠️  Warning: Repository has uncommitted changes");
            }
        }

        // Save artifacts if any were generated
        if let Some(artifacts) = &result.artifacts {
            let saved = save_artifacts(
                artifacts,
                &result.contract.name,
                &result.outputs.wasm,
                &result.outputs.rwasm,
                &config.output_directory(),
                &config.artifacts,
            )?;

            println!("✅ Successfully compiled {}", result.contract.name);
            
            // Show source type used
            if let Some(metadata) = &result.artifacts.as_ref().map(|a| &a.metadata) {
                match &metadata.source {
                    fluent_compiler::Source::Git { repository, commit, .. } => {
                        println!("📦 Source: Git repository");
                        println!("   Repository: {}", repository);
                        println!("   Commit: {}", &commit[..8]);
                    }
                    fluent_compiler::Source::Archive { .. } => {
                        println!("📦 Source: Archive (will be created)");
                    }
                }
            }
            
            println!("📁 Output directory: {}", saved.output_dir.display());
            println!("📄 Created files:");
            println!("   - lib.wasm ({} bytes)", result.outputs.wasm.len());
            println!("   - lib.rwasm ({} bytes)", result.outputs.rwasm.len());
            println!("   - rWASM hash: 0x{}", rwasm_hash);
            
            if saved.abi_path.is_some() {
                println!("   - abi.json");
            }
            if saved.interface_path.is_some() {
                println!("   - interface.sol");
            }
            if saved.metadata_path.is_some() {
                println!("   - metadata.json");
            }

            // Create archive if requested or if using archive source
            if archive || !settings.git_source {
                let archive_path = saved.output_dir.join("sources.tar.gz");
                let archive_options = ArchiveOptions {
                    format: ArchiveFormat::TarGz,
                    only_compilation_files: true,
                    compression_level: 6,
                    respect_gitignore: true,
                };

                let _archive_info = create_verification_archive(
                    &config.project_root,
                    &archive_path,
                    &archive_options,
                )?;
                println!("   - sources.tar.gz");
            }
        } else {
            println!(
                "✅ Successfully compiled {} (no artifacts generated)",
                result.contract.name
            );
            println!("   - WASM size: {} bytes", result.outputs.wasm.len());
            println!("   - rWASM size: {} bytes", result.outputs.rwasm.len());
            println!("   - rWASM hash: 0x{}", rwasm_hash);
        }
    }

    Ok(())
}

async fn run_verify(
    project_root: PathBuf,
    address: String,
    chain_id: u64,
    rpc: String,
    settings: CompileSettings,
    json: bool,
) -> Result<()> {
    // Get Git info for display
    let git_info = fluent_compiler::detect_git_info(&project_root)?;
    
    // Apply same validation as compile
    if settings.git_source {
        match &git_info {
            None => {
                return Err(eyre::eyre!(
                    "Cannot use --git-source: project is not in a Git repository"
                ));
            }
            Some(git) if git.is_dirty => {
                return Err(eyre::eyre!(
                    "Cannot use --git-source: repository has {} uncommitted changes",
                    git.dirty_files_count
                ));
            }
            _ => {}
        }
    }

    // Fetch deployed bytecode hash
    let deployed_hash = fetch_bytecode_hash(&address, &rpc, chain_id).await?;

    // Build compilation config
    let mut compile_config = CompileConfig::new(project_root.clone());
    compile_config.profile = settings.profile;
    compile_config.features = settings.features;
    compile_config.no_default_features = settings.no_default_features;
    compile_config.use_git_source = settings.git_source;

    // Run verification
    let verify_config = fluent_compiler::VerifyConfig {
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
                git_info: git_info.as_ref().map(GitInfoJson::from),
            },
        };
        println!("{}", serde_json::to_string(&output)?);
    } else {
        if verification_result.status.is_success() {
            println!("✅ Contract verified successfully!");
            println!("📝 Contract name: {}", verification_result.contract_name);
            println!("🔍 Bytecode hash matches: {}", deployed_hash);
            
            if let Some(git) = &git_info {
                println!("📦 Git: {} @ {}", git.branch, git.commit_hash_short);
            }
            
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

/// Check if repository URL is likely public
fn is_likely_public_repository(url: &str) -> bool {
    ["github.com", "gitlab.com", "bitbucket.org"]
        .iter()
        .any(|host| url.contains(host))
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
    let error_type = if error.to_string().contains("Git repository has") {
        "git_dirty_state"
    } else if error.to_string().contains("Cannot use --git-source") {
        "git_source_error"
    } else if error.to_string().contains("Compilation failed") {
        "compilation_failed"
    } else if error.to_string().contains("Failed to fetch") {
        "network_error"
    } else if error.to_string().contains("fluentbase-sdk") {
        "invalid_source"
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
        let cli = Cli::parse_from(&["fluent-compiler", "compile"]);
        assert!(matches!(cli.command, Commands::Compile { .. }));

        let cli = Cli::parse_from(&[
            "fluent-compiler",
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
            "fluent-compiler",
            "compile",
            "--profile",
            "debug",
            "--features",
            "test feature2",
            "--no-default-features",
            "--git-source",
        ]);

        if let Commands::Compile { compile, .. } = cli.command {
            assert_eq!(compile.profile, "debug");
            assert_eq!(compile.features, vec!["test", "feature2"]);
            assert!(compile.no_default_features);
            assert!(compile.git_source);
        }
    }

    #[test]
    fn test_allow_dirty_flag() {
        let cli = Cli::parse_from(&[
            "fluent-compiler",
            "compile",
            "--allow-dirty",
        ]);

        if let Commands::Compile { compile, .. } = cli.command {
            assert!(compile.allow_dirty);
            assert!(!compile.git_source);
        }
    }
}