//! Docker orchestration for reproducible builds

use eyre::{bail, eyre, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Run the compilation inside Docker
pub fn run_reproducible(
    project_root: &Path,
    rust_version: &str,
    sdk_version: &str,
    command_args: &[String],
) -> Result<()> {
    // Ensure Docker is available
    check_docker_available()?;

    // TODO: use real version after we will move fluent-builder to the fluentbase-sdk
    let sdk_version = "v0.1.0";

    // Create versioned image if needed
    create_image(sdk_version, rust_version)?;

    // Run compilation in container
    run_in_docker_container(project_root, sdk_version, rust_version, command_args)
}

/// Check if Docker is available
fn check_docker_available() -> Result<()> {
    let output = Command::new("docker")
        .arg("info")
        .output()
        .map_err(|e| eyre!("Failed to execute Docker command: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Cannot connect to the Docker daemon") {
            bail!(
                "Docker daemon is not running. Please start Docker and try again.\n\
                 To compile without Docker, use the --no-docker flag."
            );
        }
        bail!("Docker check failed: {}", stderr);
    }

    Ok(())
}

/// Generate image name for specific SDK and Rust versions
fn image_name(sdk_version: &str, rust_version: &str) -> String {
    // Sanitize versions for Docker tag
    let sdk_tag = sdk_version.replace([':', '/', '\\'], "-");
    let rust_tag = rust_version.replace([':', '/', '\\'], "-");
    format!("fluent-builder-{}-rust-{}", sdk_tag, rust_tag)
}

/// Check if image exists locally
fn image_exists(name: &str) -> Result<bool> {
    let output = Command::new("docker")
        .arg("images")
        .arg(name)
        .output()
        .map_err(|e| eyre!("Failed to check Docker images: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to check if image exists: {}", stderr);
    }

    // Check if we have more than just the header line
    Ok(output.stdout.iter().filter(|&&c| c == b'\n').count() > 1)
}

/// Create Docker image if it doesn't exist
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

/// Check if base image is available (locally or can be pulled)
fn base_image_available(image: &str) -> Result<bool> {
    // First check if it exists locally
    if image_exists(image)? {
        return Ok(true);
    }

    // Try to pull from registry
    tracing::debug!("Attempting to pull base image: {}", image);
    let output = Command::new("docker")
        .args(["pull", image])
        .output()
        .map_err(|e| eyre!("Failed to execute docker pull: {}", e))?;

    Ok(output.status.success())
}

/// Build base image from source
fn build_base_image(sdk_version: &str) -> Result<()> {
    let image_name = format!("fluentlabs/fluent-builder:{}", sdk_version);

    // For now, build from latest Rust
    // TODO: In production, checkout specific SDK tag and build
    let dockerfile = r#"
ARG BUILD_PLATFORM=linux/amd64
FROM --platform=${BUILD_PLATFORM} rust:latest AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y git && rm -rf /var/lib/apt/lists/*

# Clone and build fluent-builder
# TODO: Use specific SDK version tag
RUN git clone https://github.com/fluentlabs-xyz/fluent-builder /tmp/fluent-builder
WORKDIR /tmp/fluent-builder
RUN cargo build --release --manifest-path crates/cli/Cargo.toml

FROM --platform=${BUILD_PLATFORM} rust:latest
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
ARG BUILD_PLATFORM=linux/amd64
FROM --platform=${{BUILD_PLATFORM}} {base_image}

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
        .args(["build", "-t", image_name, "-f-", "."])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| eyre!("Failed to start Docker build: {}", e))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| eyre!("Failed to get stdin for Docker process"))?
        .write_all(dockerfile_content.as_bytes())?;

    let status = child.wait()?;
    if !status.success() {
        bail!("Docker build failed for image: {}", image_name);
    }

    Ok(())
}

/// Run compilation in Docker container
fn run_in_docker_container(
    project_root: &Path,
    sdk_version: &str,
    rust_version: &str,
    args: &[String],
) -> Result<()> {
    let image = image_name(sdk_version, rust_version);
    let project_path = project_root
        .to_str()
        .ok_or_else(|| eyre!("Project path contains invalid UTF-8"))?;

    let mut cmd = Command::new("docker");
    cmd.args([
        "run",
        "--rm",
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

    tracing::debug!("Running Docker command: {:?}", cmd);

    let status = cmd.status()?;
    if !status.success() {
        bail!("Build failed inside Docker container");
    }

    Ok(())
}

/// Clean up old Docker images
pub fn cleanup_old_images(keep_recent: usize) -> Result<()> {
    let output = Command::new("docker")
        .args(["images", "--format", "{{.Repository}}:{{.Tag}}"])
        .output()?;

    if !output.status.success() {
        bail!("Failed to list Docker images");
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut images: Vec<&str> = output_str
        .lines()
        .filter(|line| line.starts_with("fluent-builder-") && line.contains("-rust-"))
        .collect();

    if images.len() <= keep_recent {
        return Ok(());
    }

    // Sort by creation time would be better, but this is simpler
    images.sort_unstable();

    // Remove oldest images
    for image in images.clone().into_iter().take(images.len() - keep_recent) {
        tracing::info!("Removing old image: {}", image);
        Command::new("docker").args(["rmi", image]).status()?;
    }

    Ok(())
}
