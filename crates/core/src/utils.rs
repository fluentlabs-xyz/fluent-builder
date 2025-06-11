//! Utility functions for WASM compilation

/// Gets the Rust compiler version
pub fn get_rust_version() -> String {
    // Try runtime detection first
    if let Ok(output) = std::process::Command::new("rustc")
        .arg("--version")
        .output()
    {
        if let Ok(version) = String::from_utf8(output.stdout) {
            return version.trim().to_string();
        }
    }

    // Fallback to compile-time version
    option_env!("RUSTC_VERSION")
        .unwrap_or("rustc 1.75.0 (82e1608df 2023-12-21)")
        .to_string()
}

pub fn hash_bytes(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}
