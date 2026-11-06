use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRepoStatus {
    #[serde(rename = "isRepo")]
    pub is_repo: bool,
    pub branch: String,
    #[serde(rename = "hasChanges")]
    pub has_changes: bool,
    pub summary: String,
}

/// Checks whether the workspace directory is a valid git repository and gets status.
pub async fn check_git_status(cwd: &str) -> Result<GitRepoStatus, String> {
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Ok(GitRepoStatus {
            is_repo: false,
            branch: String::new(),
            has_changes: false,
            summary: "No workspace selected".to_string(),
        });
    }

    // 1. Check if git repo
    let check = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .await;

    match check {
        Ok(out) if out.status.success() => {}
        _ => {
            return Ok(GitRepoStatus {
                is_repo: false,
                branch: String::new(),
                has_changes: false,
                summary: "Not a git repository".to_string(),
            });
        }
    }

    // 2. Get current branch
    let branch_out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to read git branch: {e}"))?;

    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();

    // 3. Check status
    let status_out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to check git status: {e}"))?;

    let status_str = String::from_utf8_lossy(&status_out.stdout);
    let has_changes = !status_str.trim().is_empty();

    Ok(GitRepoStatus {
        is_repo: true,
        branch: if branch.is_empty() {
            "HEAD".to_string()
        } else {
            branch
        },
        has_changes,
        summary: if has_changes {
            format!("{} modified entries", status_str.lines().count())
        } else {
            "Working tree clean".to_string()
        },
    })
}

/// Stages all changes and creates a commit with the specified message.
pub async fn git_commit(cwd: &str, message: &str) -> Result<String, String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err("Commit message cannot be empty".to_string());
    }
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Err("Invalid or missing workspace directory".to_string());
    }

    // git add -A
    let add_out = Command::new("git")
        .args(["add", "-A"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to execute git add: {e}"))?;

    if !add_out.status.success() {
        return Err(format!(
            "git add failed: {}",
            String::from_utf8_lossy(&add_out.stderr).trim()
        ));
    }

    // git commit -m <message>
    let commit_out = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to execute git commit: {e}"))?;

    if !commit_out.status.success() {
        let err = String::from_utf8_lossy(&commit_out.stderr);
        let out = String::from_utf8_lossy(&commit_out.stdout);
        return Err(format!("git commit failed: {} {}", err.trim(), out.trim()));
    }

    Ok(String::from_utf8_lossy(&commit_out.stdout)
        .trim()
        .to_string())
}

/// Pushes the current branch to origin.
pub async fn git_push(cwd: &str) -> Result<String, String> {
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Err("Invalid or missing workspace directory".to_string());
    }

    let push_out = Command::new("git")
        .args(["push"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to execute git push: {e}"))?;

    if !push_out.status.success() {
        let err = String::from_utf8_lossy(&push_out.stderr);
        return Err(format!("git push failed: {}", err.trim()));
    }

    let out = String::from_utf8_lossy(&push_out.stdout);
    let err = String::from_utf8_lossy(&push_out.stderr);
    let combined = format!("{} {}", out.trim(), err.trim()).trim().to_string();
    Ok(if combined.is_empty() {
        "Pushed successfully".to_string()
    } else {
        combined
    })
}
