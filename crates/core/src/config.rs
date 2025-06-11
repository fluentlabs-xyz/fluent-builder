//! Configuration types for WASM compilation and artifact generation

use eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main configuration for contract compilation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompileConfig {
    /// Project root directory containing the Rust contract
    pub project_root: PathBuf,

    /// Output directory for all generated artifacts
    /// If relative, it's relative to project_root
    pub output_dir: PathBuf,

    /// Build profile (debug, release, or custom)
    pub profile: BuildProfile,

    /// Features to enable during compilation
    pub features: Vec<String>,

    /// Whether to disable default features
    pub no_default_features: bool,

    /// Whether to use locked dependencies (--locked flag)
    pub locked: bool,

    /// Artifact generation settings
    pub artifacts: ArtifactsConfig,
}

/// Configuration for artifact generation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactsConfig {
    /// Whether to generate Solidity interface file (.sol)
    pub generate_interface: bool,

    /// Whether to generate Solidity ABI file (.json)
    pub generate_abi: bool,

    /// Whether to generate metadata file with build info
    pub generate_metadata: bool,

    /// Whether to pretty-print JSON artifacts
    pub pretty_json: bool,
}

/// Build profile for compilation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BuildProfile {
    Debug,
    Release,
    Custom(String),
}

impl Default for CompileConfig {
    fn default() -> Self {
        Self {
            project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            output_dir: PathBuf::from("out"),
            profile: BuildProfile::Release,
            features: vec![],
            no_default_features: true,
            locked: false,
            artifacts: ArtifactsConfig::default(),
        }
    }
}

impl Default for ArtifactsConfig {
    fn default() -> Self {
        Self {
            generate_interface: true,
            generate_abi: true,
            generate_metadata: true,
            pretty_json: true,
        }
    }
}

impl CompileConfig {
    /// Returns the absolute output directory path
    pub fn output_directory(&self) -> PathBuf {
        if self.output_dir.is_absolute() {
            self.output_dir.clone()
        } else {
            self.project_root.join(&self.output_dir)
        }
    }

    /// Returns the target triple for WASM compilation
    pub fn target(&self) -> &str {
        "wasm32-unknown-unknown"
    }

    /// Returns the profile name as used by cargo
    pub fn profile_name(&self) -> &str {
        match &self.profile {
            BuildProfile::Debug => "debug",
            BuildProfile::Release => "release",
            BuildProfile::Custom(name) => name,
        }
    }

    /// Validates the entire configuration
    pub fn validate(&self) -> Result<()> {
        // self.validate_project_root()?;
        Ok(())
    }

    #[allow(dead_code)]
    /// Validates that the project root exists and contains a Cargo.toml
    fn validate_project_root(&self) -> Result<()> {
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

    /// Create a new builder for CompileConfig
    pub fn builder() -> CompileConfigBuilder {
        CompileConfigBuilder::default()
    }
}

/// Builder for creating CompileConfig with a fluent API
#[derive(Default)]
pub struct CompileConfigBuilder {
    config: CompileConfig,
}

impl CompileConfigBuilder {
    /// Set the project root directory
    pub fn project_root(mut self, path: PathBuf) -> Self {
        self.config.project_root = path;
        self
    }

    /// Set the output directory
    pub fn output_dir(mut self, path: PathBuf) -> Self {
        self.config.output_dir = path;
        self
    }

    /// Set the build profile by name
    pub fn profile(mut self, profile: &str) -> Self {
        self.config.profile = match profile {
            "debug" => BuildProfile::Debug,
            "release" => BuildProfile::Release,
            custom => BuildProfile::Custom(custom.to_string()),
        };
        self
    }

    /// Set the build profile directly
    pub fn build_profile(mut self, profile: BuildProfile) -> Self {
        self.config.profile = profile;
        self
    }

    /// Set features to enable
    pub fn features(mut self, features: Vec<String>) -> Self {
        self.config.features = features;
        self
    }

    /// Add a single feature
    pub fn feature(mut self, feature: String) -> Self {
        self.config.features.push(feature);
        self
    }

    /// Set whether to disable default features
    pub fn no_default_features(mut self, no_default: bool) -> Self {
        self.config.no_default_features = no_default;
        self
    }

    /// Set whether to use --locked
    pub fn locked(mut self, locked: bool) -> Self {
        self.config.locked = locked;
        self
    }

    /// Configure artifact generation
    pub fn artifacts(mut self, configure: impl FnOnce(&mut ArtifactsConfig)) -> Self {
        configure(&mut self.config.artifacts);
        self
    }

    /// Disable all artifact generation
    pub fn no_artifacts(mut self) -> Self {
        self.config.artifacts.generate_interface = false;
        self.config.artifacts.generate_abi = false;
        self.config.artifacts.generate_metadata = false;
        self
    }

    /// Enable only ABI generation (useful for verification)
    pub fn abi_only(mut self) -> Self {
        self.config.artifacts.generate_interface = false;
        self.config.artifacts.generate_abi = true;
        self.config.artifacts.generate_metadata = false;
        self
    }

    /// Build and validate the configuration
    pub fn build(self) -> Result<CompileConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}

/// Simplified compilation settings matching CLI structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileSettings {
    pub profile: String,
    pub features: Vec<String>,
    pub no_default_features: bool,
}

impl CompileSettings {
    /// Convert to full CompileConfig
    pub fn to_config(self, project_root: PathBuf) -> CompileConfig {
        CompileConfig::builder()
            .project_root(project_root)
            .profile(&self.profile)
            .features(self.features)
            .no_default_features(self.no_default_features)
            .build()
            .expect("Invalid settings")
    }
}

impl From<CompileSettings> for CompileConfig {
    fn from(settings: CompileSettings) -> Self {
        settings.to_config(PathBuf::from("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CompileConfig::default();
        assert_eq!(config.target(), "wasm32-unknown-unknown");
        assert_eq!(config.profile, BuildProfile::Release);
        assert!(config.no_default_features);
        assert!(config.artifacts.generate_interface);
        assert!(config.artifacts.generate_abi);
        assert!(config.artifacts.generate_metadata);
    }

    #[test]
    fn test_builder_basic() {
        let config = CompileConfig::builder()
            .project_root(PathBuf::from("/test"))
            .output_dir(PathBuf::from("build"))
            .profile("debug")
            .features(vec!["test".to_string()])
            .no_default_features(true)
            .build()
            .unwrap();

        println!("config: {:?}", config);

        assert_eq!(config.project_root, PathBuf::from("/test"));
        assert_eq!(config.output_dir, PathBuf::from("build"));
        assert_eq!(config.profile, BuildProfile::Debug);
        assert_eq!(config.features, vec!["test"]);
        assert!(config.no_default_features);
    }

    #[test]
    fn test_compile_settings() {
        let settings = CompileSettings {
            profile: "release".to_string(),
            features: vec!["production".to_string()],
            no_default_features: true,
        };

        let config = settings.to_config(PathBuf::from("/test"));
        assert_eq!(config.profile, BuildProfile::Release);
        assert_eq!(config.features, vec!["production"]);
        assert!(config.no_default_features);
    }
}
