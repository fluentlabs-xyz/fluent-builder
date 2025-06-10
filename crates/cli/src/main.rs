//! CLI for fluent-compiler library
//!
//! Compiles and verifies Rust smart contracts for the Fluent blockchain.
//!
//! Examples:
//! - Compile: `fluent-compiler compile`
//! - Compile with archive: `fluent-compiler compile --archive`
//! - Verify by address: `fluent-compiler verify --address 0x123...`
//! - Verify by hash: `fluent-compiler verify --deployed-hash 0xabc...`

use clap::{Parser, Subcommand};
use eyre::{Context, Result};
use fluent_compiler::{
    compile, verify_contract, CompileConfig, VerifyConfigBuilder,
    VerificationStatus, VerificationMetadata,
    blockchain::{NetworkConfig, ethers::fetch_bytecode_hash},
};
use serde::{Serialize};
use std::path::PathBuf;
use tracing::Level;

/// Fluent smart contract compiler and verifier
#[derive(Parser, Debug)]
#[command(name = "fluent-compiler")]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging (debug level)
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress all logging (only errors)
    #[arg(short, long, global = true)]
    quiet: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile a Rust contract to WASM/rWASM
    Compile(CompileArgs),
    
    /// Verify a deployed contract against source code
    Verify(VerifyArgs),
}

/// Compile command arguments
#[derive(Parser, Debug)]
struct CompileArgs {
    /// Path to the project root directory
    #[arg(value_name = "PROJECT_DIR", default_value = ".")]
    project_root: PathBuf,

    /// Output directory for artifacts (default: ./out)
    #[arg(short, long, value_name = "DIR")]
    output_dir: Option<PathBuf>,

    /// Output JSON to stdout only, don't save files
    #[arg(long)]
    json_only: bool,

    /// Create source archive (sources.tar.gz)
    #[arg(long)]
    archive: bool,

    /// Use compact JSON format (no pretty printing)
    #[arg(long)]
    compact: bool,
}

/// Verify command arguments
#[derive(Parser, Debug)]
struct VerifyArgs {
    /// Path to the project root directory
    #[arg(value_name = "PROJECT_DIR", default_value = ".")]
    project_root: PathBuf,

    /// Contract address to verify (fetches bytecode from chain)
    #[arg(long, value_name = "ADDRESS", conflicts_with = "deployed_hash", group = "source")]
    address: Option<String>,

    /// Deployed bytecode hash (if already known)
    #[arg(long, value_name = "HASH", conflicts_with = "address", group = "source")]
    deployed_hash: Option<String>,

    /// Use local network (localhost:8545)
    #[arg(long, conflicts_with_all = &["dev", "rpc"], requires = "address")]
    local: bool,

    /// Use dev network
    #[arg(long, conflicts_with_all = &["local", "rpc"], requires = "address")]
    dev: bool,

    /// Custom RPC endpoint
    #[arg(long, value_name = "URL", requires_all = &["chain_id", "address"])]
    rpc: Option<String>,

    /// Chain ID (required with --rpc)
    #[arg(long, value_name = "ID", requires = "rpc")]
    chain_id: Option<u64>,

    /// Export ABI to file after successful verification
    #[arg(long, value_name = "FILE")]
    export_abi: Option<PathBuf>,

    /// Build profile override (debug/release)
    #[arg(long, value_name = "PROFILE")]
    profile: Option<String>,

    /// Features to enable (can be used multiple times)
    #[arg(long, value_name = "FEATURE")]
    features: Vec<String>,

    /// Disable default features
    #[arg(long)]
    no_default_features: bool,

    /// Output format (json or human)
    #[arg(long, value_name = "FORMAT", default_value = "human")]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum OutputFormat {
    Human,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "human" => Ok(OutputFormat::Human),
            "json" => Ok(OutputFormat::Json),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
}

/// Compilation output for JSON mode
#[derive(Debug, Serialize)]
struct CompileOutput {
    pub contract_name: String,
    pub wasm_bytecode_hex: String,
    pub rwasm_bytecode_hex: String,
    pub abi: serde_json::Value,
    pub build_metadata: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_artifacts: Option<SavedPaths>,
}

/// Saved artifact paths
#[derive(Debug, Serialize)]
struct SavedPaths {
    pub output_dir: String,
    pub wasm: String,
    pub rwasm: String,
    pub abi: String,
    pub interface: String,
    pub metadata: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
}

/// Verification output for JSON mode
#[derive(Debug, Serialize)]
struct VerifyOutput {
    pub verified: bool,
    pub contract_name: String,
    pub expected_hash: String,
    pub actual_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abi_exported_to: Option<String>,
}

/// Error output
#[derive(Debug, Serialize)]
struct ErrorOutput {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
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

    // Create runtime for async operations (only for verify command)
    let result = match &cli.command {
        Commands::Compile(args) => run_compile(args),
        Commands::Verify(args) => {
            let runtime = tokio::runtime::Runtime::new()
                .expect("Failed to create async runtime");
            runtime.block_on(run_verify(args))
        }
    };

    if let Err(e) = result {
        output_error(e);
        std::process::exit(1);
    }
}

fn run_compile(args: &CompileArgs) -> Result<()> {
    let mut config = CompileConfig::default();
    config.project_root = args.project_root.clone();
    
    if let Some(output_dir) = args.output_dir.clone() {
        config.output_dir = output_dir;
    }

    config.validate()?;

    tracing::info!("Compiling project at: {}", config.project_root.display());

    // Compile
    let output = compile(&config).context("Compilation failed")?;
    let result = &output.result;

    tracing::info!(
        "Successfully compiled {} in {:?}",
        result.contract_info.name,
        output.duration
    );

    // Save artifacts unless --json-only
    let saved_paths = if !args.json_only {
        let options = fluent_compiler::ArtifactWriterOptions {
            output_dir: config.output_directory(),
            pretty_json: !args.compact,
            create_archive: args.archive,
            archive_format: fluent_compiler::archive::ArchiveFormat::TarGz,
            archive_respect_gitignore: true,
        };

        let saved = fluent_compiler::save_artifacts(result, &options, &config.artifacts)?;
        tracing::info!("Artifacts saved to: {}", saved.output_dir.display());

        Some(SavedPaths {
            output_dir: saved.output_dir.display().to_string(),
            wasm: saved.wasm_path.display().to_string(),
            rwasm: saved.rwasm_path.display().to_string(),
            abi: saved.abi_path.map(|p| p.display().to_string()).unwrap_or_default(),
            interface: saved.interface_path.map(|p| p.display().to_string()).unwrap_or_default(),
            metadata: saved.metadata_path.map(|p| p.display().to_string()).unwrap_or_default(),
            archive: saved.archive_path.map(|p| p.display().to_string()),
        })
    } else {
        None
    };

    // Output result
    if args.json_only {
        let output = CompileOutput {
            contract_name: result.contract_info.name.clone(),
            wasm_bytecode_hex: hex::encode(&result.outputs.wasm),
            rwasm_bytecode_hex: hex::encode(&result.outputs.rwasm),
            abi: serde_json::to_value(&result.artifacts.abi)?,
            build_metadata: serde_json::to_value(&result.artifacts.metadata.build_metadata)?,
            saved_artifacts: saved_paths,
        };
        
        let json = if args.compact {
            serde_json::to_string(&output)?
        } else {
            serde_json::to_string_pretty(&output)?
        };
        println!("{}", json);
    } else if let Some(paths) = saved_paths {
        println!("✅ Successfully compiled {}", result.contract_info.name);
        println!("📁 Output directory: {}", paths.output_dir);
        println!("📄 Created files:");
        println!("   - lib.wasm");
        println!("   - lib.rwasm");
        if !paths.abi.is_empty() {
            println!("   - abi.json");
        }
        if !paths.interface.is_empty() {
            println!("   - interface.sol");
        }
        if !paths.metadata.is_empty() {
            println!("   - metadata.json");
        }
        if let Some(ref archive) = paths.archive {
            println!("   - {}", archive.split('/').last().unwrap_or("sources.tar.gz"));
        }
    }

    Ok(())
}

async fn run_verify(args: &VerifyArgs) -> Result<()> {
    // Validate that either address or deployed_hash is provided
    if args.address.is_none() && args.deployed_hash.is_none() {
        return Err(eyre::eyre!("Either --address or --deployed-hash must be provided"));
    }

    // Determine bytecode hash and metadata
    let (bytecode_hash, metadata) = if let Some(address) = &args.address {
        // Get network config
        let network = get_network_config(&args)?;
        
        tracing::info!("Fetching bytecode from {} network...", network.name);
        tracing::info!("Contract address: {}", address);
        
        // Fetch bytecode hash from blockchain
        let hash = fetch_bytecode_hash(&network, address).await
            .with_context(|| format!("Failed to fetch bytecode for address {}", address))?;
        
        tracing::info!("Fetched bytecode hash: {}", hash);
        
        let metadata = Some(VerificationMetadata {
            address: address.clone(),
            chain_id: network.chain_id,
            deployment_tx_hash: None,
            block_number: None,
        });
        
        (hash, metadata)
    } else if let Some(hash) = &args.deployed_hash {
        // Use provided hash directly
        (hash.clone(), None)
    } else {
        unreachable!("Already validated that one of address or deployed_hash is provided");
    };

    // Build verification config
    let mut builder = VerifyConfigBuilder::new()
        .project_root(args.project_root.clone())
        .deployed_bytecode_hash(bytecode_hash.clone());

    // Add metadata if available
    if let Some(meta) = metadata.clone() {
        builder = builder.with_metadata(meta.address, meta.chain_id);
    }

    // Add compile overrides if provided
    if args.profile.is_some() || !args.features.is_empty() || args.no_default_features {
        let overrides = fluent_compiler::verify::CompileConfigOverride {
            rustc_version: None,
            sdk_version: None,
            profile: args.profile.clone(),
            features: if args.features.is_empty() { None } else { Some(args.features.clone()) },
            no_default_features: Some(args.no_default_features),
            cargo_flags: None,
            rustflags: None,
        };
        builder = builder.with_compile_override(overrides);
    }

    let config = builder.build()?;

    tracing::info!("Verifying contract at: {}", config.project_root.display());
    tracing::info!("Expected hash: {}", bytecode_hash);

    // Run verification
    let result = verify_contract(config)?;
    
    let verified = result.status == VerificationStatus::Success;
    
    // Export ABI if requested and verification succeeded
    let abi_path = if verified && args.export_abi.is_some() {
        let path = args.export_abi.as_ref().unwrap();
        if let Some(compilation_result) = &result.compilation_result {
            let abi_json = serde_json::to_string_pretty(&compilation_result.artifacts.abi)?;
            std::fs::write(path, abi_json)
                .with_context(|| format!("Failed to write ABI to {}", path.display()))?;
            tracing::info!("ABI exported to: {}", path.display());
            Some(path.display().to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Output result
    match args.format {
        OutputFormat::Json => {
            let output = VerifyOutput {
                verified,
                contract_name: result.contract_name,
                expected_hash: result.details.expected_hash,
                actual_hash: result.details.actual_hash.unwrap_or_default(),
                contract_address: metadata.as_ref().map(|m| m.address.clone()),
                chain_id: metadata.as_ref().map(|m| m.chain_id),
                error_message: result.details.error_message,
                abi_exported_to: abi_path,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Human => {
            if verified {
                println!("✅ Contract verified successfully!");
                println!("📝 Contract name: {}", result.contract_name);
                println!("🔍 Bytecode hash matches: {}", result.details.expected_hash);
                
                if let Some(meta) = &metadata {
                    println!("\n📋 Contract details:");
                    println!("   Address: {}", meta.address);
                    println!("   Chain ID: {}", meta.chain_id);
                }
                
                if let Some(sdk_version) = &result.details.sdk_version {
                    println!("\n🛠️  Build details:");
                    println!("   SDK version: {}", sdk_version);
                    println!("   Compiler: {}", result.details.compiler_version.as_ref().unwrap_or(&"unknown".to_string()));
                    println!("   Profile: {}", result.details.build_profile.as_ref().unwrap_or(&"unknown".to_string()));
                }
                
                if let Some(path) = abi_path {
                    println!("\n📄 ABI exported to: {}", path);
                }
            } else {
                println!("❌ Verification failed!");
                println!("📝 Contract name: {}", result.contract_name);
                
                if let Some(error) = &result.details.error_message {
                    println!("⚠️  Error: {}", error);
                }
                
                println!("\n🔍 Hash comparison:");
                println!("   Expected: {}", result.details.expected_hash);
                if let Some(actual) = &result.details.actual_hash {
                    println!("   Actual:   {}", actual);
                }
                
                println!("\n💡 Possible reasons:");
                println!("   - Different compiler version or build settings");
                println!("   - Source code doesn't match deployed contract");
                println!("   - Wrong contract address or network");
            }
        }
    }

    if !verified {
        std::process::exit(1);
    }

    Ok(())
}

fn get_network_config(args: &VerifyArgs) -> Result<NetworkConfig> {
    if args.local {
        Ok(NetworkConfig::local())
    } else if args.dev {
        Ok(NetworkConfig::fluent_dev())
    } else if let (Some(rpc), Some(chain_id)) = (&args.rpc, args.chain_id) {
        Ok(NetworkConfig::custom("custom", rpc.clone(), chain_id))
    } else {
        // Default to local if no network specified
        Ok(NetworkConfig::local())
    }
}

fn output_error(error: eyre::Report) {
    let error_output = ErrorOutput {
        error: error.to_string(),
        details: error
            .chain()
            .skip(1)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ")
            .into(),
    };

    eprintln!("{}", serde_json::to_string_pretty(&error_output).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        // Compile command
        let cli = Cli::parse_from(&["fluent-compiler", "compile"]);
        assert!(matches!(cli.command, Commands::Compile(_)));

        // Verify command with address
        let cli = Cli::parse_from(&["fluent-compiler", "verify", "--address", "0x123"]);
        assert!(matches!(cli.command, Commands::Verify(_)));

        // Verify command with deployed hash
        let cli = Cli::parse_from(&["fluent-compiler", "verify", "--deployed-hash", "0xabc"]);
        assert!(matches!(cli.command, Commands::Verify(_)));
    }

    #[test]
    fn test_compile_args() {
        let cli = Cli::parse_from(&[
            "fluent-compiler",
            "compile",
            "./my-contract",
            "--output-dir", "./build",
            "--archive",
            "--json-only",
        ]);
        
        if let Commands::Compile(args) = cli.command {
            assert_eq!(args.project_root, PathBuf::from("./my-contract"));
            assert_eq!(args.output_dir, Some(PathBuf::from("./build")));
            assert!(args.archive);
            assert!(args.json_only);
        } else {
            panic!("Expected Compile command");
        }
    }

    #[test]
    fn test_verify_args() {
        let cli = Cli::parse_from(&[
            "fluent-compiler",
            "verify",
            "--address", "0x123",
            "--dev",
            "--export-abi", "abi.json",
            "--profile", "release",
            "--features", "prod",
            "--no-default-features",
        ]);
        
        if let Commands::Verify(args) = cli.command {
            assert_eq!(args.address, Some("0x123".to_string()));
            assert!(args.dev);
            assert_eq!(args.export_abi, Some(PathBuf::from("abi.json")));
            assert_eq!(args.profile, Some("release".to_string()));
            assert_eq!(args.features, vec!["prod"]);
            assert!(args.no_default_features);
        } else {
            panic!("Expected Verify command");
        }
    }

    #[test]
    fn test_output_format_parsing() {
        assert_eq!("human".parse::<OutputFormat>().unwrap(), OutputFormat::Human);
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert!("invalid".parse::<OutputFormat>().is_err());
    }
}