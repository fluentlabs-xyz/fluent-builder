//! Docker orchestration for reproducible builds

use eyre::{bail, eyre, Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Docker image name format for fluent-builder
fn image_name(sdk_version: &str, rust_version: &str) -> String {
    format!("fluent-builder-{}-rust-{}", sdk_version, rust_version)
}

/// Run the compilation inside Docker container
pub fn run_reproducible(
    project_root: &Path,
    rust_version: &str,
    sdk_version: &str,
    command_args: &[String],
) -> Result<()> {
    // Check if Docker is available
    check_docker_available()?;

    // TODO: use real version after we move fluent-builder to the fluentbase-sdk
    let sdk_version = "v0.1.0";

    // Canonicalize project path for proper mounting
    let canonicalized_project_root = project_root
        .canonicalize()
        .context("Failed to canonicalize project directory")?;

    // Create versioned image if needed
    create_image(sdk_version, rust_version)?;

    // Run compilation in container
    run_in_docker_container(
        &canonicalized_project_root,
        sdk_version,
        rust_version,
        command_args,
    )
}

/// Check if Docker daemon is running and accessible
fn check_docker_available() -> Result<()> {
    let status = Command::new("docker")
        .args(["info"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to execute docker command")?;

    if !status.success() {
        bail!(
            "Docker is not installed or not running. Please start Docker and try again.\n\
            To compile without Docker, use the --no-docker flag.\n\
            Install Docker: https://docs.docker.com/get-docker/"
        );
    }

    Ok(())
}

/// Check if Docker image exists locally
fn image_exists(name: &str) -> Result<bool> {
    let output = Command::new("docker")
        .args(["images", "-q", name])
        .output()
        .context("Failed to check Docker images")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to check if image exists: {}", stderr);
    }

    // If output is empty, image doesn't exist
    Ok(!output.stdout.is_empty())
}

/// Create Docker image with specific SDK and Rust versions
fn create_image(sdk_version: &str, rust_version: &str) -> Result<()> {
    let name = image_name(sdk_version, rust_version);

    if image_exists(&name)? {
        tracing::debug!("Using existing Docker image: {}", name);
        return Ok(());
    }

    println!(
        "Building Docker image for Rust {} with SDK {} (one-time setup)...",
        rust_version, sdk_version
    );

    // Determine base image name
    let base_image = format!("fluentlabs/fluent-builder:{}", sdk_version);

    // Check if base image exists (locally or in registry)
    if !base_image_available(&base_image)? {
        println!(
            "Base image {} not found, building from source...",
            base_image
        );
        build_base_image(sdk_version)?;
    }

    // Build versioned image with specific Rust toolchain
    build_versioned_image(&name, &base_image, rust_version)?;

    Ok(())
}

/// Check if base image is available locally or can be pulled from registry
fn base_image_available(image: &str) -> Result<bool> {
    // First check if it exists locally
    if image_exists(image)? {
        return Ok(true);
    }

    // Try to pull from registry
    tracing::debug!("Attempting to pull base image: {}", image);
    let status = Command::new("docker")
        .args(["pull", image])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to execute docker pull")?;

    Ok(status.success())
}

/// Build base fluent-builder image from source
fn build_base_image(sdk_version: &str) -> Result<()> {
    let image_name = format!("fluentlabs/fluent-builder:{}", sdk_version);

    // For now, build from latest Rust
    // TODO: In production, checkout specific SDK tag and build
    let dockerfile = r#"
FROM rust:latest AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y git && rm -rf /var/lib/apt/lists/*

# Clone and build fluent-builder
# TODO: Use specific SDK version tag
RUN git clone https://github.com/fluentlabs-xyz/fluent-builder /tmp/fluent-builder
WORKDIR /tmp/fluent-builder
RUN cargo build --release --manifest-path crates/cli/Cargo.toml

FROM rust:latest
COPY --from=builder /tmp/fluent-builder/target/release/fluent-builder /usr/local/bin/fluent-builder

# Verify installation
RUN fluent-builder --version
"#;

    build_docker_image(&image_name, dockerfile)
}

/// Build versioned image with specific Rust toolchain
fn build_versioned_image(target_image: &str, base_image: &str, rust_version: &str) -> Result<()> {
    // Format toolchain version for rustup
    let toolchain = format_toolchain_version(rust_version);

    let dockerfile = format!(
        r#"
FROM {base_image}

# Install specific Rust toolchain
RUN rustup toolchain install {toolchain}
RUN rustup default {toolchain}
RUN rustup target add wasm32-unknown-unknown --toolchain {toolchain}
RUN rustup component add rust-src --toolchain {toolchain}

# Set working directory
WORKDIR /workspace

# Mark as fluent-builder Docker image
ENV FLUENT_BUILDER_DOCKER=1
"#
    );

    build_docker_image(target_image, &dockerfile)
}

/// Format Rust version for rustup toolchain install
fn format_toolchain_version(rust_version: &str) -> String {
    if rust_version == "nightly" || rust_version.starts_with("nightly-") {
        // Nightly versions use the format as-is
        format!("{}-x86_64-unknown-linux-gnu", rust_version)
    } else {
        // Stable versions like "1.75.0"
        format!("{}-x86_64-unknown-linux-gnu", rust_version)
    }
}

/// Build Docker image from Dockerfile content
fn build_docker_image(image_name: &str, dockerfile_content: &str) -> Result<()> {
    let mut child = Command::new("docker")
        .args([
            "build",
            "--platform",
            "linux/amd64", // Force consistent platform
            "-t",
            image_name,
            "-f-",
            ".",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to start Docker build")?;

    // Write Dockerfile content to stdin
    child
        .stdin
        .as_mut()
        .ok_or_else(|| eyre!("Failed to get stdin for Docker process"))?
        .write_all(dockerfile_content.as_bytes())
        .context("Failed to write Dockerfile content")?;

    let status = child.wait().context("Docker build process failed")?;

    if !status.success() {
        bail!("Docker build failed for image: {}", image_name);
    }

    Ok(())
}

/// Run fluent-builder compilation inside Docker container
fn run_in_docker_container(
    project_root: &Path,
    sdk_version: &str,
    rust_version: &str,
    args: &[String],
) -> Result<()> {
    let image = image_name(sdk_version, rust_version);

    // Convert project path to string
    let project_path = project_root
        .to_str()
        .ok_or_else(|| eyre!("Project path contains invalid UTF-8"))?;

    // Build docker command
    let mut cmd = Command::new("docker");
    cmd.args([
        "run",
        "--rm",
        "--platform",
        "linux/amd64", // Force consistent platform for reproducible builds
        "--network",
        "host",
        "-v",
        &format!("{}:/workspace", project_path),
        "-v",
        "cargo-registry:/usr/local/cargo/registry",
        "-v",
        "cargo-git:/usr/local/cargo/git",
        "-w",
        "/workspace",
        &image,
        "fluent-builder",
    ]);

    // Add all CLI arguments
    cmd.args(args);

    // Add --no-docker to prevent recursion
    cmd.arg("--no-docker");

    tracing::debug!("Running Docker command: {:?}", cmd);

    // Execute and inherit stdio for real-time output
    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to execute Docker container")?;

    if !status.success() {
        bail!("Build failed inside Docker container");
    }

    Ok(())
}

/// Clean up old Docker images keeping only the most recent ones
pub fn cleanup_old_images(keep_recent: usize) -> Result<()> {
    let output = Command::new("docker")
        .args([
            "images",
            "--format",
            "{{.Repository}}:{{.Tag}}\t{{.CreatedAt}}",
            "--filter",
            "reference=fluent-builder-*",
        ])
        .output()
        .context("Failed to list Docker images")?;

    if !output.status.success() {
        bail!("Failed to list Docker images");
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut images: Vec<(&str, &str)> = output_str
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() == 2 && parts[0].starts_with("fluent-builder-") {
                Some((parts[0], parts[1]))
            } else {
                None
            }
        })
        .collect();

    if images.len() <= keep_recent {
        return Ok(());
    }

    // Sort by creation date (newest first)
    images.sort_by(|a, b| b.1.cmp(a.1));

    // Remove oldest images
    for (image, _) in images.into_iter().skip(keep_recent) {
        tracing::info!("Removing old Docker image: {}", image);

        let status = Command::new("docker")
            .args(["rmi", image])
            .status()
            .context("Failed to remove Docker image")?;

        if !status.success() {
            tracing::warn!("Failed to remove image: {}", image);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_name_generation() {
        assert_eq!(
            image_name("v0.1.0", "1.75.0"),
            "fluent-builder-v0.1.0-rust-1.75.0"
        );

        assert_eq!(
            image_name("v0.2.0-beta", "nightly-2024-01-01"),
            "fluent-builder-v0.2.0-beta-rust-nightly-2024-01-01"
        );
    }

    #[test]
    fn test_format_toolchain_version() {
        assert_eq!(
            format_toolchain_version("1.75.0"),
            "1.75.0-x86_64-unknown-linux-gnu"
        );

        assert_eq!(
            format_toolchain_version("nightly"),
            "nightly-x86_64-unknown-linux-gnu"
        );

        assert_eq!(
            format_toolchain_version("nightly-2024-01-01"),
            "nightly-2024-01-01-x86_64-unknown-linux-gnu"
        );
    }

    #[test]
    #[ignore] // Requires Docker to be running
    fn test_docker_available() {
        assert!(check_docker_available().is_ok());
    }
}
