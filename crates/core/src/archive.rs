//! Source code archiving utilities for contract verification

use crate::CompilationResult;
use eyre::{Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use tar::Builder;
use walkdir::WalkDir;

/// Archive format options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    /// Tar archive compressed with gzip (.tar.gz)
    TarGz,
    /// ZIP archive
    Zip,
}

/// Options for creating source archives
#[derive(Debug, Clone)]
pub struct ArchiveOptions {
    /// Archive format to use
    pub format: ArchiveFormat,
    /// Include only files that were used during compilation
    pub only_compilation_files: bool,
    /// Compression level (0-9, where 9 is maximum compression)
    pub compression_level: u32,
    /// Use .gitignore rules if present
    pub respect_gitignore: bool,
}

impl Default for ArchiveOptions {
    fn default() -> Self {
        Self {
            format: ArchiveFormat::TarGz,
            only_compilation_files: true,
            compression_level: 6,
            respect_gitignore: true,
        }
    }
}

/// Information about created archive
#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    /// Path to the created archive
    pub path: PathBuf,
    /// SHA256 hash of the archive
    pub hash: String,
    /// Size in bytes
    pub size: u64,
    /// Number of files included
    pub file_count: usize,
    /// Root path inside archive (for verifier to know where Cargo.toml is)
    pub cargo_toml_path: String,
}

/// Create a source archive for verification
pub fn create_verification_archive(
    project_root: &Path,
    output_path: &Path,
    options: &ArchiveOptions,
    compilation_result: Option<&CompilationResult>,
) -> Result<ArchiveInfo> {
    // Load gitignore rules if needed
    let gitignore = if options.respect_gitignore {
        load_gitignore_rules(project_root)?
    } else {
        None
    };

    // Determine which files to include
    let files_to_include = collect_files_to_archive(
        project_root,
        options,
        compilation_result,
        gitignore.as_ref(),
    )?;

    if files_to_include.is_empty() {
        return Err(eyre::eyre!("No source files found to archive"));
    }

    // Create the archive
    match options.format {
        ArchiveFormat::TarGz => create_tar_gz_archive(
            project_root,
            output_path,
            &files_to_include,
            options.compression_level,
        ),
        ArchiveFormat::Zip => create_zip_archive(
            project_root,
            output_path,
            &files_to_include,
            options.compression_level,
        ),
    }
}

/// Collect files that should be included in the archive
fn collect_files_to_archive(
    project_root: &Path,
    options: &ArchiveOptions,
    compilation_result: Option<&CompilationResult>,
    gitignore: Option<&ignore::gitignore::Gitignore>,
) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();

    if options.only_compilation_files && compilation_result.is_some() {
        // Include only files that were part of the compilation
        let result = compilation_result.unwrap();

        for source_path in result.artifacts.metadata.build_metadata.sources.keys() {
            if source_path != "_aggregate" {
                let full_path = project_root.join(source_path);
                if full_path.exists() && is_valid_source_file(&full_path) {
                    files.insert(full_path);
                }
            }
        }
    } else {
        // Walk through all source files
        for entry in WalkDir::new(project_root)
            .follow_links(false) // Don't follow symlinks for security
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip if gitignore says to ignore it
            if let Some(gi) = gitignore {
                if gi.matched(path, path.is_dir()).is_ignore() {
                    continue;
                }
            }

            // Only include valid source files
            if path.is_file() && is_valid_source_file(path) {
                files.insert(path.to_path_buf());
            }
        }
    }

    // Always ensure Cargo.toml is included
    let cargo_toml = project_root.join("Cargo.toml");
    if cargo_toml.exists() {
        files.insert(cargo_toml);
    } else {
        return Err(eyre::eyre!("Cargo.toml not found in project root"));
    }

    // Include Cargo.lock if it exists
    let cargo_lock = project_root.join("Cargo.lock");
    if cargo_lock.exists() {
        files.insert(cargo_lock);
    }

    Ok(files.into_iter().collect())
}

/// Check if a file should be included in the archive
fn is_valid_source_file(path: &Path) -> bool {
    // Check by extension
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext == "rs" {
            return true;
        }
    }

    // Check by filename
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(name, "Cargo.toml" | "Cargo.lock")
    } else {
        false
    }
}

/// Load gitignore rules from project
fn load_gitignore_rules(project_root: &Path) -> Result<Option<ignore::gitignore::Gitignore>> {
    let gitignore_path = project_root.join(".gitignore");

    if gitignore_path.exists() {
        let (gi, error) = ignore::gitignore::Gitignore::new(&gitignore_path);

        if let Some(err) = error {
            tracing::warn!("Error parsing .gitignore: {}", err);
        }

        Ok(Some(gi))
    } else {
        Ok(None)
    }
}

/// Create a tar.gz archive
fn create_tar_gz_archive(
    project_root: &Path,
    output_path: &Path,
    files: &[PathBuf],
    compression_level: u32,
) -> Result<ArchiveInfo> {
    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create output directory")?;
    }

    let tar_gz = File::create(output_path)
        .with_context(|| format!("Failed to create archive: {}", output_path.display()))?;

    let enc = GzEncoder::new(tar_gz, Compression::new(compression_level));
    let mut tar = Builder::new(enc);

    let cargo_toml_path = add_files_to_tar(&mut tar, project_root, files)?;

    let encoder = tar.into_inner().context("Failed to finalize tar archive")?;
    encoder.finish().context("Failed to finish compression")?;

    calculate_archive_info(output_path, files.len(), cargo_toml_path)
}

/// Create a ZIP archive
fn create_zip_archive(
    project_root: &Path,
    output_path: &Path,
    files: &[PathBuf],
    compression_level: u32,
) -> Result<ArchiveInfo> {
    use zip::write::FileOptions;
    use zip::{CompressionMethod, ZipWriter};

    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create output directory")?;
    }

    let file = File::create(output_path)
        .with_context(|| format!("Failed to create archive: {}", output_path.display()))?;

    let mut zip = ZipWriter::new(BufWriter::new(file));

    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(compression_level as i32));

    let mut cargo_toml_path = String::new();

    for file_path in files {
        let relative_path = file_path.strip_prefix(project_root).unwrap_or(file_path);

        let path_str = relative_path.to_string_lossy();

        // Track the first Cargo.toml we find
        if relative_path.file_name() == Some(std::ffi::OsStr::new("Cargo.toml"))
            && cargo_toml_path.is_empty()
        {
            cargo_toml_path = path_str.to_string();
        }

        zip.start_file(path_str.as_ref(), options)
            .with_context(|| format!("Failed to add file to zip: {}", path_str))?;

        let content = std::fs::read(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        zip.write_all(&content)
            .with_context(|| format!("Failed to write file to zip: {}", path_str))?;
    }

    zip.finish().context("Failed to finalize ZIP archive")?;

    calculate_archive_info(output_path, files.len(), cargo_toml_path)
}

/// Add files to tar archive
fn add_files_to_tar(
    tar: &mut Builder<GzEncoder<File>>,
    project_root: &Path,
    files: &[PathBuf],
) -> Result<String> {
    let mut cargo_toml_path = String::new();

    for file_path in files {
        let relative_path = file_path.strip_prefix(project_root).unwrap_or(file_path);

        // Track the first Cargo.toml path
        if relative_path.file_name() == Some(std::ffi::OsStr::new("Cargo.toml"))
            && cargo_toml_path.is_empty()
        {
            cargo_toml_path = relative_path.to_string_lossy().to_string();
        }

        tar.append_path_with_name(file_path, relative_path)
            .with_context(|| format!("Failed to add file to archive: {}", file_path.display()))?;
    }

    if cargo_toml_path.is_empty() {
        return Err(eyre::eyre!("No Cargo.toml found in archived files"));
    }

    Ok(cargo_toml_path)
}

/// Calculate archive info after creation
fn calculate_archive_info(
    path: &Path,
    file_count: usize,
    cargo_toml_path: String,
) -> Result<ArchiveInfo> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("Failed to get archive metadata: {}", path.display()))?;

    let hash = {
        let content = std::fs::read(path)
            .with_context(|| format!("Failed to read archive: {}", path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        format!("{:x}", hasher.finalize())
    };

    Ok(ArchiveInfo {
        path: path.to_path_buf(),
        hash,
        size: metadata.len(),
        file_count,
        cargo_toml_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project(dir: &Path) -> Result<()> {
        fs::create_dir_all(dir.join("src"))?;
        fs::create_dir_all(dir.join("target/debug"))?;

        // Source files
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"",
        )?;
        fs::write(
            dir.join("Cargo.lock"),
            "# This file is automatically @generated",
        )?;
        fs::write(dir.join("src/lib.rs"), "pub fn test() {}")?;
        fs::write(dir.join("src/main.rs"), "fn main() {}")?;

        // Files that should be ignored
        fs::write(dir.join("README.md"), "# Test Project")?;
        fs::write(dir.join(".gitignore"), "target/\n*.tmp")?;
        fs::write(dir.join("test.tmp"), "temporary file")?;
        fs::write(dir.join("target/debug/test"), "binary")?;

        Ok(())
    }

    #[test]
    fn test_create_archive_with_gitignore() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        create_test_project(&project_dir).unwrap();

        let archive_path = temp_dir.path().join("archive.tar.gz");
        let options = ArchiveOptions::default();

        let info =
            create_verification_archive(&project_dir, &archive_path, &options, None).unwrap();

        assert!(archive_path.exists());
        assert!(info.size > 0);
        assert!(!info.hash.is_empty());
        assert_eq!(info.cargo_toml_path, "Cargo.toml");

        // Should include only .rs files and Cargo files
        assert!(info.file_count <= 4); // Cargo.toml, Cargo.lock, lib.rs, main.rs
    }

    #[test]
    fn test_is_valid_source_file() {
        assert!(is_valid_source_file(Path::new("src/lib.rs")));
        assert!(is_valid_source_file(Path::new("src/main.rs")));
        assert!(is_valid_source_file(Path::new("Cargo.toml")));
        assert!(is_valid_source_file(Path::new("Cargo.lock")));

        assert!(!is_valid_source_file(Path::new("README.md")));
        assert!(!is_valid_source_file(Path::new("target/debug/binary")));
        assert!(!is_valid_source_file(Path::new(".gitignore")));
    }

    #[test]
    fn test_archive_without_gitignore() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        create_test_project(&project_dir).unwrap();

        // Remove .gitignore
        fs::remove_file(project_dir.join(".gitignore")).unwrap();

        let archive_path = temp_dir.path().join("archive.tar.gz");
        let mut options = ArchiveOptions::default();
        options.respect_gitignore = false;

        let info =
            create_verification_archive(&project_dir, &archive_path, &options, None).unwrap();

        assert!(info.file_count == 4); // Only source files
    }
}
