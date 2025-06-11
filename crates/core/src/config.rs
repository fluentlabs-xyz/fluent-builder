//! Configuration for WASM contract compilation

use eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for compiling a Rust smart contract
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompileConfig {
    /// Project root directory containing Cargo.toml
    pub project_root: PathBuf,

    /// Output directory for artifacts
    pub output_dir: PathBuf,

    /// Build profile: "debug", "release", or a custom profile name
    pub profile: String,

    /// Cargo features to enable during compilation
    pub features: Vec<String>,

    /// Whether to use --no-default-features
    pub no_default_features: bool,

    /// Whether to use --locked for reproducible builds
    pub locked: bool,

    /// Which artifacts to generate
    pub artifacts: ArtifactsConfig,

    /// Whether to use git source (requires clean public repo)
    pub use_git_source: bool,
}

/// Controls which artifacts are generated during compilation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactsConfig {
    /// Generate Solidity ABI (abi.json)
    pub generate_abi: bool,

    /// Generate Solidity interface (interface.sol)
    pub generate_interface: bool,

    /// Generate verification metadata (metadata.json)
    pub generate_metadata: bool,

    /// Pretty-print JSON files
    pub pretty_json: bool,
}

impl Default for CompileConfig {
    fn default() -> Self {
        Self {
            project_root: PathBuf::from("."),
            output_dir: PathBuf::from("out"),
            profile: "release".to_string(),
            features: vec![],
            no_default_features: true,
            locked: false,
            artifacts: ArtifactsConfig::default(),
            use_git_source: false,
        }
    }
}

impl Default for ArtifactsConfig {
    fn default() -> Self {
        Self {
            generate_abi: true,
            generate_interface: true,
            generate_metadata: true,
            pretty_json: true,
        }
    }
}

impl CompileConfig {
    /// Create a new config for the given project directory
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            ..Default::default()
        }
    }

    /// Get the absolute output directory path
    pub fn output_directory(&self) -> PathBuf {
        if self.output_dir.is_absolute() {
            self.output_dir.clone()
        } else {
            self.project_root.join(&self.output_dir)
        }
    }

    /// Get the target triple for WASM compilation
    pub fn target(&self) -> &str {
        "wasm32-unknown-unknown"
    }

    /// Validate that the configuration is valid
    pub fn validate(&self) -> Result<()> {
        if !self.project_root.exists() {
            return Err(eyre::eyre!(
                "Project root does not exist: {}",
                self.project_root.display()
            ));
        }

        let cargo_toml = self.project_root.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Err(eyre::eyre!(
                "No Cargo.toml found in project root: {}",
                self.project_root.display()
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        dir
    }

    #[test]
    fn test_default_config() {
        let config = CompileConfig::default();
        assert_eq!(config.profile, "release");
        assert_eq!(config.target(), "wasm32-unknown-unknown");
        assert!(config.no_default_features);
        assert!(config.artifacts.generate_metadata);
    }

    #[test]
    fn test_new_config() {
        let project = create_test_project();
        let config = CompileConfig::new(project.path());

        assert_eq!(config.project_root, project.path());
        assert_eq!(config.output_dir, PathBuf::from("out"));
        assert_eq!(config.profile, "release");
    }

    #[test]
    fn test_validation() {
        let project = create_test_project();
        let config = CompileConfig::new(project.path());
        assert!(config.validate().is_ok());

        let bad_config = CompileConfig::new("/nonexistent/path");
        assert!(bad_config.validate().is_err());
    }

    #[test]
    fn test_output_directory() {
        let config = CompileConfig::new("/project");
        assert_eq!(config.output_directory(), PathBuf::from("/project/out"));

        let mut config = CompileConfig::new("/project");
        config.output_dir = PathBuf::from("/absolute/out");
        assert_eq!(config.output_directory(), PathBuf::from("/absolute/out"));
    }
}
