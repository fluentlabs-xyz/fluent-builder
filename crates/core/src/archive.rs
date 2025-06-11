use eyre::{ensure, Result};
use flate2::{write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tar::Builder;
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

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
    /// Path to the project directory inside the archive (where Cargo.toml is located)
    pub project_path: String,
}

const CRITICAL_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
];

pub fn create_verification_archive(
    project_root: &Path,
    output_path: &Path,
    options: &ArchiveOptions,
) -> Result<ArchiveInfo> {
    ensure!(
        project_root.join("Cargo.toml").exists(),
        "Cargo.toml missing"
    );

    let gitignore = if options.respect_gitignore {
        ignore::gitignore::Gitignore::new(project_root.join(".gitignore")).0
    } else {
        ignore::gitignore::Gitignore::empty()
    };

    let mut files = Vec::new();

    // Collect critical files
    for &critical in CRITICAL_FILES {
        let path = project_root.join(critical);
        if path.exists() {
            files.push(path);
        }
    }

    // Collect source files
    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_entry(|e| {
            !e.path().components().any(|c| {
                matches!(
                    c.as_os_str().to_str(),
                    Some("target" | "out" | "node_modules")
                ) || c.as_os_str().to_string_lossy().starts_with('.')
            })
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "rs")
            && !gitignore.matched(path, false).is_ignore()
        {
            files.push(path.to_path_buf());
        }
    }

    ensure!(!files.is_empty(), "No source files found");

    // Create output directory
    fs::create_dir_all(output_path.parent().unwrap())?;

    // Determine the project path inside the archive
    // We use the parent directory name as the root in the archive
    let project_dir_name = project_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    // Create archive with project directory structure
    match options.format {
        ArchiveFormat::TarGz => {
            let tar_gz = fs::File::create(output_path)?;
            let encoder = GzEncoder::new(tar_gz, Compression::new(options.compression_level));
            let mut tar = Builder::new(encoder);

            for file in &files {
                let relative_path = file.strip_prefix(project_root).unwrap();
                // Add project directory name as prefix
                let archive_path = Path::new(project_dir_name).join(relative_path);
                tar.append_path_with_name(file, &archive_path)?;
            }

            let encoder = tar.into_inner()?;
            encoder.finish()?;
        }
        ArchiveFormat::Zip => {
            let zip_file = fs::File::create(output_path)?;
            let mut zip = ZipWriter::new(zip_file);

            let options = FileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .compression_level(Some(options.compression_level as i32));

            for file in &files {
                let relative_path = file.strip_prefix(project_root).unwrap();
                // Add project directory name as prefix
                let archive_path = Path::new(project_dir_name).join(relative_path);
                let archive_path_str = archive_path.to_string_lossy();

                zip.start_file(&archive_path_str.into_owned(), options)?;
                zip.write_all(&fs::read(file)?)?;
            }

            zip.finish()?;
        }
    }

    // Calculate hash and size
    let content = fs::read(output_path)?;
    let hash = format!("{:x}", Sha256::digest(&content));
    let size = content.len() as u64;

    Ok(ArchiveInfo {
        path: output_path.into(),
        hash,
        size,
        file_count: files.len(),
        project_path: project_dir_name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_project_path_extraction() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let nested_project = temp_dir.path().join("contracts/token-contract");
        fs::create_dir_all(&nested_project)?;

        fs::write(
            nested_project.join("Cargo.toml"),
            "[package]\nname = \"token\"",
        )?;
        fs::create_dir_all(nested_project.join("src"))?;
        fs::write(nested_project.join("src/lib.rs"), "// token")?;

        let output_path = temp_dir.path().join("token.tar.gz");
        let options = ArchiveOptions::default();

        let info = create_verification_archive(&nested_project, &output_path, &options)?;

        // Should use the immediate parent directory name
        assert_eq!(info.project_path, "token-contract");

        Ok(())
    }
}
