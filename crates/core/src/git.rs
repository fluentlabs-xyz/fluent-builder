//! Git repository detection and information extraction

use eyre::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Git repository information
#[derive(Debug, Clone)]
pub struct GitInfo {
    /// Remote repository URL (e.g., https://github.com/user/repo.git)
    pub remote_url: String,
    /// Current commit hash (full 40-character SHA)
    pub commit_hash: String,
    /// Short commit hash (7 characters)
    pub commit_hash_short: String,
    /// Current branch name
    pub branch: String,
    /// Whether the repository has uncommitted changes
    pub is_dirty: bool,
    /// Number of uncommitted files
    pub dirty_files_count: usize,
}

/// Detect if a directory is part of a Git repository and extract info
pub fn detect_git_info(project_root: &Path) -> Result<Option<GitInfo>> {
    // Check if .git directory exists
    if !is_git_repository(project_root) {
        return Ok(None);
    }

    // Get commit hash
    let commit_hash = get_commit_hash(project_root)?;
    let commit_hash_short = commit_hash.chars().take(7).collect();

    // Get remote URL
    let remote_url = get_remote_url(project_root).unwrap_or_default();

    // Get current branch
    let branch = get_current_branch(project_root).unwrap_or_else(|_| "HEAD".to_string());

    // Check for uncommitted changes
    let (is_dirty, dirty_files_count) = check_dirty_state(project_root)?;

    Ok(Some(GitInfo {
        remote_url,
        commit_hash,
        commit_hash_short,
        branch,
        is_dirty,
        dirty_files_count,
    }))
}

/// Check if directory is a Git repository
fn is_git_repository(path: &Path) -> bool {
    let output = Command::new("git")
        .current_dir(path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();

    matches!(output, Ok(o) if o.status.success())
}

/// Get current commit hash
fn get_commit_hash(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(path)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("Failed to execute git rev-parse")?;

    if !output.status.success() {
        return Err(eyre::eyre!("Failed to get commit hash"));
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Get remote repository URL
fn get_remote_url(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(path)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .context("Failed to get remote URL")?;

    if !output.status.success() {
        return Err(eyre::eyre!("No remote origin found"));
    }

    let url = String::from_utf8(output.stdout)?.trim().to_string();

    // Normalize URL (remove credentials, convert SSH to HTTPS format)
    Ok(normalize_git_url(&url))
}

/// Get current branch name
fn get_current_branch(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to get current branch")?;

    if !output.status.success() {
        return Err(eyre::eyre!("Failed to get branch name"));
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Check if repository has uncommitted changes
fn check_dirty_state(path: &Path) -> Result<(bool, usize)> {
    // Check for any changes (staged or unstaged)
    let output = Command::new("git")
        .current_dir(path)
        .args(["status", "--porcelain"])
        .output()
        .context("Failed to check git status")?;

    if !output.status.success() {
        return Err(eyre::eyre!("Failed to get git status"));
    }

    let status = String::from_utf8(output.stdout)?;
    let dirty_files: Vec<&str> = status.lines().filter(|line| !line.is_empty()).collect();

    Ok((!dirty_files.is_empty(), dirty_files.len()))
}

/// Normalize Git URL to consistent format
fn normalize_git_url(url: &str) -> String {
    let url = url.trim();

    // Remove any embedded credentials
    let url = if let Some(idx) = url.find('@') {
        if url.starts_with("https://") || url.starts_with("http://") {
            format!("https://{}", &url[idx + 1..])
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    };

    // Convert SSH URLs to HTTPS format
    if url.starts_with("git@") {
        url.replace("git@", "https://")
            .replace(".com:", ".com/")
            .replace(".org:", ".org/")
    } else {
        url
    }
}

/// Calculate project path relative to Git root
pub fn get_project_path_in_repo(project_root: &Path) -> Result<String> {
    // Get git root directory
    let output = Command::new("git")
        .current_dir(project_root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to get git root")?;

    if !output.status.success() {
        return Err(eyre::eyre!("Failed to get git root directory"));
    }

    let git_root = PathBuf::from(String::from_utf8(output.stdout)?.trim());

    // Calculate relative path
    let relative_path = project_root
        .strip_prefix(&git_root)
        .context("Project is not inside git repository")?;

    Ok(relative_path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_git_url() {
        assert_eq!(
            normalize_git_url("git@github.com:user/repo.git"),
            "https://github.com/user/repo.git"
        );

        assert_eq!(
            normalize_git_url("https://user:pass@github.com/user/repo.git"),
            "https://github.com/user/repo.git"
        );

        assert_eq!(
            normalize_git_url("https://github.com/user/repo.git"),
            "https://github.com/user/repo.git"
        );
    }
}
