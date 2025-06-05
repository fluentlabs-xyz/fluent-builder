//! Artifact writing utilities for saving compilation results to disk

use crate::{
    archive::{create_verification_archive, ArchiveFormat, ArchiveInfo, ArchiveOptions},
    config::ArtifactsConfig,
    CompilationResult,
};
use eyre::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

/// Options for saving artifacts to disk
#[derive(Debug, Clone)]
pub struct ArtifactWriterOptions {
    /// The base output directory where contract folders will be created
    pub output_dir: PathBuf,
    /// Whether to pretty-print JSON files
    pub pretty_json: bool,
    /// Whether to create source archive
    pub create_archive: bool,
    /// Archive format to use
    pub archive_format: ArchiveFormat,
    /// Whether to respect .gitignore when creating archive
    pub archive_respect_gitignore: bool,
}

impl Default for ArtifactWriterOptions {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("out"),
            pretty_json: true,
            create_archive: false,
            archive_format: ArchiveFormat::TarGz,
            archive_respect_gitignore: true,
        }
    }
}

/// Information about saved artifacts
#[derive(Debug, Clone)]
pub struct SavedArtifacts {
    /// Path to the contract output directory
    pub output_dir: PathBuf,
    /// Path to WASM file
    pub wasm_path: PathBuf,
    /// Path to rWASM file
    pub rwasm_path: PathBuf,
    /// Path to ABI file (if saved)
    pub abi_path: Option<PathBuf>,
    /// Path to interface file (if saved)
    pub interface_path: Option<PathBuf>,
    /// Path to metadata file (if saved)
    pub metadata_path: Option<PathBuf>,
    /// Path to source archive (if created)
    pub archive_path: Option<PathBuf>,
    /// Archive info (if created)
    pub archive_info: Option<ArchiveInfo>,
}

/// Saves all generated artifacts for a contract to disk
///
/// Creates a directory structure like:
/// ```ignore
/// out/
///   ContractName.wasm/
///     lib.wasm
///     lib.rwasm
///     abi.json
///     interface.sol
///     metadata.json
///     sources.tar.gz
/// ```
pub fn save_artifacts(
    result: &CompilationResult,
    options: &ArtifactWriterOptions,
    artifacts_config: &ArtifactsConfig,
) -> Result<SavedArtifacts> {
    // Create contract-specific directory with .wasm suffix
    let contract_dir_name = format!("{}.wasm", result.contract_info.name);
    let contract_output_dir = options.output_dir.join(&contract_dir_name);

    std::fs::create_dir_all(&contract_output_dir).with_context(|| {
        format!(
            "Failed to create contract output directory: {}",
            contract_output_dir.display()
        )
    })?;

    // Initialize saved paths
    let mut saved = SavedArtifacts {
        output_dir: contract_output_dir.clone(),
        wasm_path: contract_output_dir.join("lib.wasm"),
        rwasm_path: contract_output_dir.join("lib.rwasm"),
        abi_path: None,
        interface_path: None,
        metadata_path: None,
        archive_path: None,
        archive_info: None,
    };

    // Save WASM as lib.wasm
    std::fs::write(&saved.wasm_path, &result.outputs.wasm)
        .with_context(|| format!("Failed to write WASM: {}", saved.wasm_path.display()))?;
    info!("Saved WASM to: {}", saved.wasm_path.display());

    // Save rWASM as lib.rwasm
    std::fs::write(&saved.rwasm_path, &result.outputs.rwasm)
        .with_context(|| format!("Failed to write rWASM: {}", saved.rwasm_path.display()))?;
    info!("Saved rWASM to: {}", saved.rwasm_path.display());

    // Save ABI
    if artifacts_config.generate_abi && !result.artifacts.abi.is_empty() {
        let abi_path = contract_output_dir.join("abi.json");
        let abi_json = if options.pretty_json || artifacts_config.pretty_json {
            serde_json::to_string_pretty(&result.artifacts.abi)?
        } else {
            serde_json::to_string(&result.artifacts.abi)?
        };

        std::fs::write(&abi_path, abi_json)
            .with_context(|| format!("Failed to write ABI: {}", abi_path.display()))?;
        info!("Saved ABI to: {}", abi_path.display());
        saved.abi_path = Some(abi_path);
    }

    // Save Solidity interface
    if artifacts_config.generate_interface && !result.artifacts.interface.is_empty() {
        let interface_path = contract_output_dir.join("interface.sol");
        std::fs::write(&interface_path, &result.artifacts.interface)
            .with_context(|| format!("Failed to write interface: {}", interface_path.display()))?;
        info!("Saved interface to: {}", interface_path.display());
        saved.interface_path = Some(interface_path);
    }

    // Save metadata
    if artifacts_config.generate_metadata {
        let metadata_path = contract_output_dir.join("metadata.json");
        let metadata_json = if options.pretty_json || artifacts_config.pretty_json {
            serde_json::to_string_pretty(&result.artifacts.metadata)?
        } else {
            serde_json::to_string(&result.artifacts.metadata)?
        };

        std::fs::write(&metadata_path, metadata_json)
            .with_context(|| format!("Failed to write metadata: {}", metadata_path.display()))?;
        info!("Saved metadata to: {}", metadata_path.display());
        saved.metadata_path = Some(metadata_path);
    }

    // Create source archive if requested
    if options.create_archive {
        let archive_extension = match options.archive_format {
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::Zip => "zip",
        };
        let archive_path = contract_output_dir.join(format!("sources.{}", archive_extension));

        let archive_options = ArchiveOptions {
            format: options.archive_format,
            only_compilation_files: true,
            compression_level: 6,
            respect_gitignore: options.archive_respect_gitignore,
        };

        // Get project root from the compilation result
        let project_root = result
            .build_metadata
            .settings
            .target
            .split('-')
            .next()
            .map(|_| PathBuf::from("."))
            .unwrap_or_else(|| PathBuf::from("."));

        let archive_info = create_verification_archive(
            &project_root,
            &archive_path,
            &archive_options,
            Some(result),
        )
        .context("Failed to create source archive")?;

        info!("Created source archive: {}", archive_path.display());

        saved.archive_path = Some(archive_path.clone());
        saved.archive_info = Some(ArchiveInfo {
            path: archive_path,
            hash: archive_info.hash,
            size: archive_info.size,
            file_count: archive_info.file_count,
            cargo_toml_path: archive_info.cargo_toml_path,
        });
    }

    // Log summary
    info!(
        "✅ All artifacts saved to: {}",
        contract_output_dir.display()
    );

    Ok(saved)
}

/// Helper to get the contract output directory path
pub fn get_contract_output_dir(base_dir: &Path, contract_name: &str) -> PathBuf {
    base_dir.join(format!("{}.wasm", contract_name))
}

/// Helper to save just the metadata file
pub fn save_metadata(
    metadata: &crate::artifacts::Metadata,
    output_path: &Path,
    pretty: bool,
) -> Result<()> {
    let json = if pretty {
        serde_json::to_string_pretty(metadata)?
    } else {
        serde_json::to_string(metadata)?
    };

    std::fs::write(output_path, json)
        .with_context(|| format!("Failed to write metadata: {}", output_path.display()))?;

    Ok(())
}

/// Helper to save just the ABI file
pub fn save_abi(abi: &crate::artifacts::Abi, output_path: &Path, pretty: bool) -> Result<()> {
    let json = if pretty {
        serde_json::to_string_pretty(abi)?
    } else {
        serde_json::to_string(abi)?
    };

    std::fs::write(output_path, json)
        .with_context(|| format!("Failed to write ABI: {}", output_path.display()))?;

    Ok(())
}
