//! CLI for fluent-compiler library
//!
//! Compiles Rust smart contracts to WASM/rWASM bytecode for the Fluent blockchain.
//! By default saves all artifacts to disk. Use --json-only for CI/CD pipelines.
//!
//! Examples:
//! - Compile and save all artifacts: `fluent-compiler`
//! - Compile specific project: `fluent-compiler ./my-contract`
//! - Output only JSON to stdout: `fluent-compiler --json-only`
//! - Create source archive: `fluent-compiler --archive`

use clap::Parser;
use eyre::{Context, Result};
use fluent_compiler::{compile, CompileConfig};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::Level;

/// Compile Rust contracts to WASM/rWASM for Fluent
#[derive(Clone, Parser, Debug)]
#[command(name = "fluent-compiler")]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the project root directory
    #[arg(value_name = "PROJECT_DIR", default_value = ".")]
    project_root: PathBuf,

    /// Output directory for artifacts (default: ./out)
    #[arg(short, long, value_name = "DIR")]
    output_dir: Option<PathBuf>,

    /// Path to JSON configuration file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Output JSON to stdout only, don't save files
    #[arg(long)]
    json_only: bool,

    /// Create source archive (sources.tar.gz)
    #[arg(long)]
    archive: bool,

    /// Use compact JSON format (no pretty printing)
    #[arg(long)]
    compact: bool,

    /// Enable verbose logging (debug level)
    #[arg(short, long, conflicts_with = "quiet")]
    verbose: bool,

    /// Suppress all logging (only errors)
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,
}

/// Output structure for verification
#[derive(Debug, Serialize, Deserialize)]
struct CompilerOutput {
    /// Contract name
    pub contract_name: String,
    /// WASM bytecode as hex (without 0x prefix)
    pub wasm_bytecode_hex: String,
    /// rWASM bytecode as hex (without 0x prefix)
    pub rwasm_bytecode_hex: String,
    /// ABI JSON array
    pub abi: serde_json::Value,
    /// Build metadata for reproducibility
    pub build_metadata: serde_json::Value,
    /// Paths to saved artifacts (only if files were saved)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_artifacts: Option<SavedPaths>,
}

/// Paths where artifacts were saved
#[derive(Debug, Serialize, Deserialize)]
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

/// Error output structure
#[derive(Debug, Serialize, Deserialize)]
struct ErrorOutput {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// Simple success message for default mode
#[derive(Debug, Serialize, Deserialize)]
struct SuccessOutput {
    pub status: String,
    pub contract_name: String,
    pub output_dir: String,
    pub files_created: Vec<String>,
}

fn main() {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.quiet {
        Level::ERROR
    } else if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    // Run and handle result
    match run(args.clone()) {
        Ok(output) => {
            if args.json_only {
                // Output full JSON for pipelines
                output_json(&output, !args.compact);
            } else {
                // Output simple success message
                output_success(&output);
            }
        }
        Err(e) => {
            output_error(e, !args.compact);
            std::process::exit(1);
        }
    }
}

fn run(args: Args) -> Result<CompilerOutput> {
    // Load configuration
    let config = load_config(&args)?;

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
        let pretty_json = !args.compact;

        let options = fluent_compiler::ArtifactWriterOptions {
            output_dir: config.output_directory(),
            pretty_json,
            create_archive: args.archive,
            archive_format: fluent_compiler::archive::ArchiveFormat::TarGz,
            archive_respect_gitignore: true,
        };

        let saved = fluent_compiler::save_artifacts(result, &options, &config.artifacts)?;

        tracing::info!("Artifacts saved to: {}", saved.output_dir.display());

        let mut files = vec![
            saved
                .wasm_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            saved
                .rwasm_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ];

        if let Some(p) = &saved.abi_path {
            files.push(p.file_name().unwrap().to_string_lossy().to_string());
        }
        if let Some(p) = &saved.interface_path {
            files.push(p.file_name().unwrap().to_string_lossy().to_string());
        }
        if let Some(p) = &saved.metadata_path {
            files.push(p.file_name().unwrap().to_string_lossy().to_string());
        }
        if let Some(p) = &saved.archive_path {
            files.push(p.file_name().unwrap().to_string_lossy().to_string());
        }

        Some(SavedPaths {
            output_dir: saved.output_dir.display().to_string(),
            wasm: saved.wasm_path.display().to_string(),
            rwasm: saved.rwasm_path.display().to_string(),
            abi: saved
                .abi_path
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            interface: saved
                .interface_path
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            metadata: saved
                .metadata_path
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            archive: saved.archive_path.map(|p| p.display().to_string()),
        })
    } else {
        None
    };

    // Prepare output
    Ok(CompilerOutput {
        contract_name: result.contract_info.name.clone(),
        wasm_bytecode_hex: hex::encode(&result.outputs.wasm),
        rwasm_bytecode_hex: hex::encode(&result.outputs.rwasm),
        abi: serde_json::to_value(&result.artifacts.abi)?,
        build_metadata: serde_json::to_value(&result.artifacts.metadata.build_metadata)?,
        saved_artifacts: saved_paths,
    })
}

fn load_config(args: &Args) -> Result<CompileConfig> {
    let mut config = if let Some(config_path) = &args.config {
        // Load from file
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config: {}", config_path.display()))?;

        tracing::debug!("Loading config from: {}", config_path.display());
        serde_json::from_str(&content)?
    } else {
        // Default config
        CompileConfig::default()
    };

    // CLI args override config file values
    config.project_root = args.project_root.clone();

    if let Some(output_dir) = &args.output_dir {
        config.output_dir = output_dir.clone();
    }

    // Validate
    config.validate()?;

    Ok(config)
}

fn output_json(output: &CompilerOutput, pretty: bool) {
    let writer = std::io::stdout();
    let result = if pretty {
        serde_json::to_writer_pretty(writer, output)
    } else {
        serde_json::to_writer(writer, output)
    };

    if let Err(e) = result {
        eprintln!("Failed to write JSON output: {}", e);
        std::process::exit(1);
    }
    println!(); // Add newline
}

fn output_success(output: &CompilerOutput) {
    if let Some(ref paths) = output.saved_artifacts {
        println!("✅ Successfully compiled {}", output.contract_name);
        println!("📁 Output directory: {}", paths.output_dir);

        // List created files
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
            println!(
                "   - {}",
                archive.split('/').last().unwrap_or("sources.tar.gz")
            );
        }
    }
}

fn output_error(error: eyre::Report, pretty: bool) {
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

    let writer = std::io::stderr();
    let result = if pretty {
        serde_json::to_writer_pretty(writer, &error_output)
    } else {
        serde_json::to_writer(writer, &error_output)
    };

    if let Err(e) = result {
        eprintln!("Critical error: {}", e);
    }
    eprintln!(); // Add newline
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        // Default behavior
        let args = Args::parse_from(&["fluent-compiler"]);
        assert_eq!(args.project_root, PathBuf::from("."));
        assert!(!args.json_only);
        assert!(!args.archive);

        // With project path
        let args = Args::parse_from(&["fluent-compiler", "/path/to/project"]);
        assert_eq!(args.project_root, PathBuf::from("/path/to/project"));

        // JSON only mode
        let args = Args::parse_from(&["fluent-compiler", "--json-only"]);
        assert!(args.json_only);

        // With archive
        let args = Args::parse_from(&["fluent-compiler", "--archive"]);
        assert!(args.archive);
    }

    #[test]
    fn test_conflicting_args() {
        // Cannot use both verbose and quiet
        let result = Args::try_parse_from(&["fluent-compiler", "-v", "-q"]);
        assert!(result.is_err());
    }
}
