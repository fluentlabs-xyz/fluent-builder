//! CLI for fluent-compiler library
//!
//! Compiles and verifies Rust smart contracts for the Fluent blockchain.

use clap::{Parser, Subcommand};
use eyre::{Context, Result};
use fluent_compiler::{
    compile, CompileConfig, verify::{VerifyConfigBuilder, verify_contract},
    blockchain::{NetworkConfig, ethers::fetch_bytecode_hash},
};
use serde::Serialize;
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
    Error {
        error_type: String,
        message: String,
    },
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
        Commands::Compile { project_root, output_dir, archive, json, compile } => {
            run_compile(project_root, output_dir, compile, archive, json)
        }
        Commands::Verify { project_root, address, chain_id, rpc, json, compile } => {
            let runtime = tokio::runtime::Runtime::new()
                .expect("Failed to create async runtime");
            runtime.block_on(run_verify(project_root, address, chain_id, rpc, compile, json))
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
    // Use builder pattern for configuration
    let config = CompileConfig::builder()
        .project_root(project_root)
        .output_dir(output_dir)
        .profile(&settings.profile)
        .features(settings.features)
        .no_default_features(settings.no_default_features)
        .build()
        .context("Invalid compilation configuration")?;
    
    let output = compile(&config)
        .context("Compilation failed")?;
    
    let result = &output.result;
    
    if json {
        let output = Output::Success {
            data: SuccessData::Compile {
                contract_name: result.contract_info.name.clone(),
                rwasm_hash: result.rwasm_hash(),
                wasm_size: result.outputs.wasm.len(),
                rwasm_size: result.outputs.rwasm.len(),
                has_abi: !result.artifacts.abi.is_empty(),
                output_dir: None,
            },
        };
        println!("{}", serde_json::to_string(&output)?);
    } else {
        let options = fluent_compiler::ArtifactWriterOptions {
            project_root: Some(config.project_root.clone()),
            output_dir: config.output_directory(),
            pretty_json: true,
            create_archive: archive,
            archive_format: fluent_compiler::archive::ArchiveFormat::TarGz,
            archive_respect_gitignore: true,
        };

        let saved = fluent_compiler::save_artifacts(result, &options, &config.artifacts)?;
        
        println!("✅ Successfully compiled {}", result.contract_info.name);
        println!("📁 Output directory: {}", saved.output_dir.display());
        println!("📄 Created files:");
        println!("   - lib.wasm ({} bytes)", result.outputs.wasm.len());
        println!("   - lib.rwasm ({} bytes)", result.outputs.rwasm.len());
        if saved.abi_path.is_some() {
            println!("   - abi.json");
        }
        if saved.interface_path.is_some() {
            println!("   - interface.sol");
        }
        if saved.metadata_path.is_some() {
            println!("   - metadata.json");
        }
        if saved.archive_path.is_some() {
            println!("   - sources.tar.gz");
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
    // Fetch deployed bytecode hash
    let network = NetworkConfig::custom("custom", rpc, chain_id);
    let deployed_hash = fetch_bytecode_hash(&network, &address).await
        .context("Failed to fetch deployed bytecode")?;
    
    // Build verification config
    let verify_config = VerifyConfigBuilder::new()
        .project_root(project_root)
        .deployed_bytecode_hash(deployed_hash.clone())
        .with_compile_settings(&settings.profile, settings.features, settings.no_default_features)
        .with_metadata(address.clone(), chain_id)
        .build()?;
    
    // Run verification
    let verification_result = verify_contract(verify_config)
        .context("Verification failed")?;
    
    if json {
        let output = Output::Success {
            data: SuccessData::Verify {
                verified: verification_result.status.is_success(),
                contract_name: verification_result.contract_name.clone(),
                expected_hash: verification_result.details.expected_hash.clone(),
                actual_hash: verification_result.details.actual_hash.clone().unwrap_or_default(),
                abi: if verification_result.status.is_success() {
                    verification_result.compilation_result
                        .as_ref()
                        .filter(|r| !r.artifacts.abi.is_empty())
                        .map(|r| serde_json::to_value(&r.artifacts.abi).ok())
                        .flatten()
                } else {
                    None
                },
                compiler_version: verification_result.details.compiler_version.clone().unwrap_or_default(),
                sdk_version: verification_result.details.sdk_version.clone().unwrap_or_default(),
            },
        };
        println!("{}", serde_json::to_string(&output)?);
    } else {
        if verification_result.status.is_success() {
            println!("✅ Contract verified successfully!");
            println!("📝 Contract name: {}", verification_result.contract_name);
            println!("🔍 Bytecode hash matches: {}", verification_result.details.expected_hash);
            println!("\n📋 Contract details:");
            println!("   Address: {}", address);
            println!("   Chain ID: {}", chain_id);
            println!("\n🛠️  Build details:");
            if let Some(version) = &verification_result.details.compiler_version {
                println!("   Compiler: {}", version);
            }
            if let Some(sdk) = &verification_result.details.sdk_version {
                println!("   SDK version: {}", sdk);
            }
        } else {
            println!("❌ Verification failed!");
            println!("📝 Contract name: {}", verification_result.contract_name);
            if let Some(error) = &verification_result.details.error_message {
                println!("⚠️  Error: {}", error);
            }
            println!("\n🔍 Hash comparison:");
            println!("   Expected: {}", verification_result.details.expected_hash);
            if let Some(actual) = &verification_result.details.actual_hash {
                println!("   Actual:   {}", actual);
            }
        }
    }
    
    if !verification_result.status.is_success() {
        std::process::exit(1);
    }
    
    Ok(())
}

fn output_error(error: eyre::Report) {
    let error_type = if error.to_string().contains("Compilation failed") {
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
            "--address", "0x123",
            "--chain-id", "20993",
            "--rpc", "https://rpc.endpoint"
        ]);
        assert!(matches!(cli.command, Commands::Verify { .. }));
    }

    #[test]
    fn test_compile_settings() {
        let cli = Cli::parse_from(&[
            "fluent-compiler",
            "compile",
            "--profile", "debug",
            "--features", "test feature2",
            "--no-default-features"
        ]);
        
        if let Commands::Compile { compile, .. } = cli.command {
            assert_eq!(compile.profile, "debug");
            assert_eq!(compile.features, vec!["test", "feature2"]);
            assert!(compile.no_default_features);
        }
    }
}