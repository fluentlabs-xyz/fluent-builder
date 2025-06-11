//! Contract detection and metadata management

use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use toml::Value;
use walkdir::WalkDir;

/// Information about a detected WASM contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmContract {
    /// Path to the contract directory (containing Cargo.toml)
    pub path: PathBuf,

    /// Contract name from Cargo.toml
    pub name: String,

    /// Contract version from Cargo.toml
    pub version: String,

    /// Fluentbase SDK version from dependencies
    pub sdk_version: Option<String>,
}

impl WasmContract {
    /// Returns the expected WASM filename after compilation
    pub fn wasm_filename(&self) -> String {
        format!("{}.wasm", self.name.replace('-', "_"))
    }

    /// Returns the path where compiled WASM will be located
    pub fn wasm_output_path(&self, target: &str, profile: &str) -> PathBuf {
        self.path
            .join("target")
            .join(target)
            .join(profile)
            .join(self.wasm_filename())
    }

    /// Gets the main source file for this contract
    pub fn main_source_file(&self) -> Result<PathBuf> {
        let cargo_dir = &self.path;
        let candidates = [cargo_dir.join("src/lib.rs"), cargo_dir.join("src/main.rs")];

        for candidate in &candidates {
            if candidate.exists() {
                return Ok(candidate.clone());
            }
        }

        Err(eyre::eyre!(
            "No main source file found in {}. Expected src/lib.rs or src/main.rs",
            cargo_dir.display()
        ))
    }

    /// Checks if the contract has a specific feature in Cargo.toml
    pub fn has_feature(&self, feature: &str) -> Result<bool> {
        let cargo_toml = self.read_cargo_toml()?;

        if let Some(features) = cargo_toml.get("features").and_then(|f| f.as_table()) {
            Ok(features.contains_key(feature))
        } else {
            Ok(false)
        }
    }

    /// Reads and parses the contract's Cargo.toml
    fn read_cargo_toml(&self) -> Result<toml::Value> {
        let cargo_toml_path = self.path.join("Cargo.toml");
        let content = std::fs::read_to_string(&cargo_toml_path)
            .with_context(|| format!("Failed to read {}", cargo_toml_path.display()))?;

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", cargo_toml_path.display()))
    }
}

/// Detects all WASM contracts in the specified paths
pub fn detect_contracts(paths: &[PathBuf]) -> Result<Vec<WasmContract>> {
    let mut contracts = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    for base_path in paths {
        if !base_path.exists() {
            continue;
        }

        // Walk through directory tree looking for Cargo.toml files
        for entry in WalkDir::new(base_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() != "Cargo.toml" {
                continue;
            }

            let contract_path = entry.path().parent().unwrap().to_path_buf();

            // Skip if we've already seen this path (due to symlinks)
            if !seen_paths.insert(contract_path.clone()) {
                continue;
            }

            // Try to parse as a WASM contract
            match parse_contract_metadata(entry.path()) {
                Ok(mut contract) => {
                    // Update path to be the directory containing Cargo.toml
                    contract.path = contract_path;
                    contracts.push(contract);
                }
                Err(_) => {
                    // Not a Fluent contract, skip silently
                    continue;
                }
            }
        }
    }

    // Sort contracts by name for deterministic order
    contracts.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(contracts)
}

/// Parses contract metadata from a Cargo.toml file
pub fn parse_contract_metadata(cargo_toml_path: &Path) -> Result<WasmContract> {
    let content = std::fs::read_to_string(cargo_toml_path)
        .with_context(|| format!("Failed to read {}", cargo_toml_path.display()))?;

    let cargo_toml: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", cargo_toml_path.display()))?;

    // Check for fluentbase-sdk dependency
    let deps = cargo_toml
        .get("dependencies")
        .and_then(|d| d.as_table())
        .ok_or_else(|| eyre::eyre!("No dependencies section found"))?;

    if !deps.contains_key("fluentbase-sdk") {
        return Err(eyre::eyre!(
            "Not a Fluent contract (no fluentbase-sdk dependency)"
        ));
    }

    // Extract package info
    let package = cargo_toml
        .get("package")
        .and_then(|p| p.as_table())
        .ok_or_else(|| eyre::eyre!("No package section found"))?;

    let name = package
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| eyre::eyre!("No package name found"))?
        .to_string();

    let version = package
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.1.0")
        .to_string();

    // Get project directory
    let project_dir = cargo_toml_path.parent().unwrap();

    // Try to get SDK version from Cargo.lock first (most reliable)
    let sdk_version = read_sdk_version_from_cargo_lock(project_dir)?.or_else(|| {
        // Fallback to extracting from Cargo.toml if Cargo.lock doesn't exist
        extract_dependency_version(deps.get("fluentbase-sdk"))
    });

    Ok(WasmContract {
        path: project_dir.to_path_buf(),
        name,
        version,
        sdk_version,
    })
}

/// Extract SDK version from Cargo.lock file
pub fn read_sdk_version_from_cargo_lock(project_root: &Path) -> Result<Option<String>> {
    let cargo_lock_path = project_root.join("Cargo.lock");

    if !cargo_lock_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&cargo_lock_path)
        .with_context(|| format!("Failed to read Cargo.lock at {}", cargo_lock_path.display()))?;

    let lock_file: Value =
        toml::from_str(&content).with_context(|| "Failed to parse Cargo.lock")?;

    if let Some(packages) = lock_file.get("package").and_then(|p| p.as_array()) {
        for package in packages {
            if package.get("name").and_then(Value::as_str) == Some("fluentbase-sdk") {
                let version = package
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let source = package
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if source.starts_with("git+") {
                    if let Some(commit_hash) = source.split('#').nth(1) {
                        return Ok(Some(format!("{}-{}", version, commit_hash)));
                    }
                }
                return Ok(Some(version.to_string()));
            }
        }
    }

    Ok(None)
}

/// Read rust toolchain version from rust-toolchain or rust-toolchain.toml
pub fn read_rust_toolchain(project_root: &Path) -> Result<Option<String>> {
    // Try rust-toolchain.toml first
    let toolchain_toml = project_root.join("rust-toolchain.toml");
    if toolchain_toml.exists() {
        let content = std::fs::read_to_string(&toolchain_toml)?;
        let toml: toml::Value = toml::from_str(&content)?;

        if let Some(channel) = toml
            .get("toolchain")
            .and_then(|t| t.get("channel"))
            .and_then(|c| c.as_str())
        {
            return Ok(Some(channel.to_string()));
        }
    }

    // Try rust-toolchain file
    let toolchain_file = project_root.join("rust-toolchain");
    if toolchain_file.exists() {
        let content = std::fs::read_to_string(&toolchain_file)?;
        return Ok(Some(content.trim().to_string()));
    }

    Ok(None)
}

/// Gets the main source file for a contract
pub fn get_main_source_file(contract: &WasmContract) -> Result<PathBuf> {
    contract.main_source_file()
}

/// Extracts version from a dependency value
fn extract_dependency_version(dep_value: Option<&toml::Value>) -> Option<String> {
    match dep_value? {
        toml::Value::String(version) => Some(version.clone()),
        toml::Value::Table(table) => {
            if let Some(toml::Value::String(version)) = table.get("version") {
                Some(version.clone())
            } else if table.contains_key("path") {
                Some("path".to_string())
            } else if table.contains_key("git") {
                Some("git".to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Validates that a contract can be compiled
pub fn validate_contract(contract: &WasmContract) -> Result<()> {
    // Check that contract directory exists
    if !contract.path.exists() {
        return Err(eyre::eyre!(
            "Contract directory does not exist: {}",
            contract.path.display()
        ));
    }

    // Check that Cargo.toml exists
    let cargo_toml = contract.path.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(eyre::eyre!(
            "Cargo.toml not found in contract directory: {}",
            contract.path.display()
        ));
    }

    // Check that source file exists
    let main_source = contract.main_source_file()?;
    if !main_source.exists() {
        return Err(eyre::eyre!(
            "Main source file not found: {}",
            main_source.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_contract(dir: &Path, name: &str, with_sdk: bool) -> Result<()> {
        let contract_dir = dir.join(name);
        fs::create_dir_all(&contract_dir)?;
        fs::create_dir_all(contract_dir.join("src"))?;

        // Create Cargo.toml
        let cargo_toml = if with_sdk {
            format!(
                r#"
[package]
name = "{name}"
version = "0.1.0"

[dependencies]
fluentbase-sdk = "0.1.0"
"#
            )
        } else {
            format!(
                r#"
[package]
name = "{name}"
version = "0.1.0"

[dependencies]
serde = "1.0"
"#
            )
        };

        fs::write(contract_dir.join("Cargo.toml"), cargo_toml)?;

        // Create lib.rs
        fs::write(
            contract_dir.join("src/lib.rs"),
            r#"
#[no_mangle]
pub extern "C" fn deploy() {}
"#,
        )?;

        Ok(())
    }

    #[test]
    fn test_detect_contracts() {
        let temp_dir = TempDir::new().unwrap();

        // Create test contracts
        create_test_contract(temp_dir.path(), "contract1", true).unwrap();
        create_test_contract(temp_dir.path(), "contract2", true).unwrap();
        create_test_contract(temp_dir.path(), "not-a-contract", false).unwrap();

        let contracts = detect_contracts(&[temp_dir.path().to_path_buf()]).unwrap();

        assert_eq!(contracts.len(), 2);
        assert_eq!(contracts[0].name, "contract1");
        assert_eq!(contracts[1].name, "contract2");
    }

    #[test]
    fn test_parse_contract_metadata() {
        let temp_dir = TempDir::new().unwrap();
        create_test_contract(temp_dir.path(), "test-contract", true).unwrap();

        let cargo_toml_path = temp_dir.path().join("test-contract/Cargo.toml");
        let contract = parse_contract_metadata(&cargo_toml_path).unwrap();

        assert_eq!(contract.name, "test-contract");
        assert_eq!(contract.version, "0.1.0");
        assert_eq!(contract.sdk_version, Some("0.1.0".to_string()));
    }

    #[test]
    fn test_wasm_filename() {
        let contract = WasmContract {
            path: PathBuf::from("/test"),
            name: "my-test-contract".to_string(),
            version: "0.1.0".to_string(),
            sdk_version: None,
        };

        assert_eq!(contract.wasm_filename(), "my_test_contract.wasm");
    }

    #[test]
    fn test_wasm_output_path() {
        let contract = WasmContract {
            path: PathBuf::from("/project/contract"),
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            sdk_version: None,
        };

        let output_path = contract.wasm_output_path("wasm32-unknown-unknown", "release");
        assert_eq!(
            output_path,
            PathBuf::from("/project/contract/target/wasm32-unknown-unknown/release/test.wasm")
        );
    }

    #[test]
    fn test_validate_contract() {
        let temp_dir = TempDir::new().unwrap();
        create_test_contract(temp_dir.path(), "valid-contract", true).unwrap();

        let contract = WasmContract {
            path: temp_dir.path().join("valid-contract"),
            name: "valid-contract".to_string(),
            version: "0.1.0".to_string(),
            sdk_version: Some("0.1.0".to_string()),
        };

        assert!(validate_contract(&contract).is_ok());

        // Test invalid contract
        let invalid_contract = WasmContract {
            path: PathBuf::from("/non/existent/path"),
            name: "invalid".to_string(),
            version: "0.1.0".to_string(),
            sdk_version: None,
        };

        assert!(validate_contract(&invalid_contract).is_err());
    }

    #[test]
    fn test_read_sdk_version_from_cargo_lock() {
        let temp_dir = TempDir::new().unwrap();

        // Create a sample Cargo.lock
        let cargo_lock_content = r#"
# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "fluentbase-sdk"
version = "0.5.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abcd1234"

[[package]]
name = "my-contract"
version = "0.1.0"
dependencies = [
    "fluentbase-sdk",
]
"#;

        fs::write(temp_dir.path().join("Cargo.lock"), cargo_lock_content).unwrap();

        let sdk_version = read_sdk_version_from_cargo_lock(temp_dir.path()).unwrap();
        assert_eq!(sdk_version, Some("0.5.0".to_string()));
    }

    #[test]
    fn test_read_sdk_version_from_cargo_lock_missing() {
        let temp_dir = TempDir::new().unwrap();

        // No Cargo.lock file
        let sdk_version = read_sdk_version_from_cargo_lock(temp_dir.path()).unwrap();
        assert_eq!(sdk_version, None);
    }

    #[test]
    fn test_read_sdk_version_from_cargo_lock_no_sdk() {
        let temp_dir = TempDir::new().unwrap();

        // Cargo.lock without fluentbase-sdk
        let cargo_lock_content = r#"
[[package]]
name = "some-other-package"
version = "1.0.0"
"#;

        fs::write(temp_dir.path().join("Cargo.lock"), cargo_lock_content).unwrap();

        let sdk_version = read_sdk_version_from_cargo_lock(temp_dir.path()).unwrap();
        assert_eq!(sdk_version, None);
    }

    #[test]
    fn test_read_rust_toolchain() {
        let temp_dir = TempDir::new().unwrap();

        // Test rust-toolchain.toml
        let toolchain_toml = r#"
[toolchain]
channel = "1.75.0"
"#;
        fs::write(temp_dir.path().join("rust-toolchain.toml"), toolchain_toml).unwrap();

        let version = read_rust_toolchain(temp_dir.path()).unwrap();
        assert_eq!(version, Some("1.75.0".to_string()));

        // Test rust-toolchain file
        fs::remove_file(temp_dir.path().join("rust-toolchain.toml")).unwrap();
        fs::write(temp_dir.path().join("rust-toolchain"), "1.76.0\n").unwrap();

        let version = read_rust_toolchain(temp_dir.path()).unwrap();
        assert_eq!(version, Some("1.76.0".to_string()));
    }
}
