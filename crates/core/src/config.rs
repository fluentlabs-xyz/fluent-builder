//! Configuration types for WASM compilation, rWASM conversion, and artifact generation

use eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main configuration that combines all compilation aspects
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompileConfig {
    /// Project root directory containing the Rust contract
    pub project_root: PathBuf,

    /// Output directory for all generated artifacts
    /// If relative, it's relative to project_root
    pub output_dir: PathBuf,

    /// WASM compilation settings
    pub wasm: WasmConfig,

    /// rWASM conversion settings
    pub rwasm: RwasmConfig,

    /// Artifact generation settings
    pub artifacts: ArtifactsConfig,
}

/// Configuration for WASM compilation phase
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WasmConfig {
    /// Target triple for WASM compilation (e.g., "wasm32-unknown-unknown")
    pub target: String,

    /// Build profile (debug, release, or custom)
    pub profile: BuildProfile,

    /// Features to enable during compilation
    pub features: Vec<String>,

    /// Whether to disable default features
    pub no_default_features: bool,

    /// Additional cargo build flags
    pub cargo_flags: Vec<String>,

    /// RUSTFLAGS environment variable value
    pub rustflags: Option<String>,

    /// Stack size for the WASM module in bytes
    pub stack_size: Option<usize>,

    /// Whether to use locked dependencies (--locked flag)
    /// Important for reproducible builds in verification
    pub locked: bool,
}

/// Configuration for rWASM conversion phase
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(Default)]
pub struct RwasmConfig {
    /// Entrypoint function name override
    /// If not specified, uses the default entrypoint detection
    pub entrypoint_name: Option<String>,

    /// Whether to use 32-bit addressing mode for stack operations
    /// Default is false (64-bit mode)
    pub use_32bit_stack: bool,
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

    /// Whether to include source file hashes in metadata
    /// Required for verification
    pub include_source_hashes: bool,

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
            wasm: WasmConfig::default(),
            rwasm: RwasmConfig::default(),
            artifacts: ArtifactsConfig::default(),
        }
    }
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            target: "wasm32-unknown-unknown".to_string(),
            profile: BuildProfile::Release,
            features: vec!["wasm".to_string()],
            no_default_features: true,
            cargo_flags: Vec::new(),
            rustflags: None,
            stack_size: None,
            locked: false,
        }
    }
}


impl Default for ArtifactsConfig {
    fn default() -> Self {
        Self {
            generate_interface: true,
            generate_abi: true,
            generate_metadata: true,
            include_source_hashes: true,
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

    /// Validates the entire configuration
    pub fn validate(&self) -> Result<()> {
        self.validate_project_root()?;
        self.wasm.validate()?;
        Ok(())
    }

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
}

impl WasmConfig {
    /// Validates WASM compilation settings
    pub fn validate(&self) -> Result<()> {
        // Validate target format
        if self.target.is_empty() {
            return Err(eyre::eyre!("Target cannot be empty"));
        }

        if !self.target.contains('-') {
            return Err(eyre::eyre!(
                "Invalid target format: '{}'. Expected format like 'wasm32-unknown-unknown'",
                self.target
            ));
        }

        // Validate stack size if specified
        if let Some(stack_size) = self.stack_size {
            if stack_size == 0 {
                return Err(eyre::eyre!("Stack size cannot be zero"));
            }
            if stack_size > 16 * 1024 * 1024 {
                // 16MB max
                return Err(eyre::eyre!(
                    "Stack size too large: {} bytes (max 16MB)",
                    stack_size
                ));
            }
        }

        Ok(())
    }

    /// Returns the profile name as used by cargo
    pub fn profile_name(&self) -> &str {
        match &self.profile {
            BuildProfile::Debug => "debug",
            BuildProfile::Release => "release",
            BuildProfile::Custom(name) => name,
        }
    }

    /// Builds the complete RUSTFLAGS value combining stack size and custom flags
    pub fn build_rustflags(&self) -> Option<String> {
        let mut flags = Vec::new();

        if let Some(stack_size) = self.stack_size {
            flags.push(format!("-C link-arg=-zstack-size={stack_size}"));
        }

        if let Some(custom_flags) = &self.rustflags {
            flags.push(custom_flags.clone());
        }

        if flags.is_empty() {
            None
        } else {
            Some(flags.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CompileConfig::default();
        assert_eq!(config.wasm.target, "wasm32-unknown-unknown");
        assert_eq!(config.wasm.profile, BuildProfile::Release);
        assert!(config.wasm.no_default_features);
        assert!(config.artifacts.generate_interface);
        assert!(config.artifacts.generate_abi);
        assert!(config.artifacts.generate_metadata);
    }
}
