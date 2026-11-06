use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitFileChange {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitRepoStatus {
    #[serde(rename = "isRepo")]
    pub is_repo: bool,
    pub branch: String,
    #[serde(rename = "hasChanges")]
    pub has_changes: bool,
    pub summary: String,
    pub files: Vec<GitFileChange>,
}

/// Parses standard `git status --porcelain` text output into structured file change items.
pub fn parse_porcelain_output(raw: &str) -> Vec<GitFileChange> {
    let mut files = Vec::new();
    for line in raw.lines() {
        if line.len() < 3 {
            continue;
        }
        let status = line[..2].trim().to_string();
        let raw_path = line[2..].trim();
        // Handle potential git quote encapsulation
        let path = raw_path.trim_matches('"').to_string();
        if !path.is_empty() {
            files.push(GitFileChange { path, status });
        }
    }
    files
}

/// Checks whether the workspace directory is a valid git repository and gets status.
pub async fn check_git_status(cwd: &str) -> Result<GitRepoStatus, String> {
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Ok(GitRepoStatus {
            is_repo: false,
            branch: String::new(),
            has_changes: false,
            summary: "No workspace selected".to_string(),
            files: Vec::new(),
        });
    }

    // 1. Check if git repo
    let check = super::create_hidden_command("git")
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
                files: Vec::new(),
            });
        }
    }

    // 2. Get current branch
    let branch_out = super::create_hidden_command("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to read git branch: {e}"))?;

    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();

    // 3. Check status
    let status_out = super::create_hidden_command("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to check git status: {e}"))?;

    let status_str = String::from_utf8_lossy(&status_out.stdout);
    let files = parse_porcelain_output(&status_str);
    let has_changes = !files.is_empty();

    Ok(GitRepoStatus {
        is_repo: true,
        branch: if branch.is_empty() {
            "HEAD".to_string()
        } else {
            branch
        },
        has_changes,
        summary: if has_changes {
            format!("{} modified entries", files.len())
        } else {
            "Working tree clean".to_string()
        },
        files,
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
    let add_out = super::create_hidden_command("git")
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
    let commit_out = super::create_hidden_command("git")
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

    let push_out = super::create_hidden_command("git")
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

/// Reverts changes made to a specific file (git checkout/restore, or removes untracked file).
pub async fn git_revert_file(cwd: &str, file_path: &str) -> Result<String, String> {
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Err("Invalid or missing workspace directory".to_string());
    }
    let full_path = Path::new(cwd).join(file_path);

    // Check status of this file
    let status_out = super::create_hidden_command("git")
        .args(["status", "--porcelain", file_path])
        .current_dir(cwd)
        .output()
        .await;

    if let Ok(out) = status_out {
        let text = String::from_utf8_lossy(&out.stdout);
        if text.trim().starts_with("??") {
            // Untracked file: safely remove it if it exists
            if full_path.is_file() {
                std::fs::remove_file(&full_path)
                    .map_err(|e| format!("Failed to remove untracked file: {e}"))?;
                return Ok(format!("Removed untracked file {}", file_path));
            }
        }
    }

    // Attempt git checkout
    let checkout_out = super::create_hidden_command("git")
        .args(["checkout", "--", file_path])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to revert file changes: {e}"))?;

    if !checkout_out.status.success() {
        // Fallback to git restore
        let restore_out = super::create_hidden_command("git")
            .args(["restore", file_path])
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| format!("Failed to restore file: {e}"))?;

        if !restore_out.status.success() {
            let err = String::from_utf8_lossy(&restore_out.stderr);
            return Err(format!("Could not revert {}: {}", file_path, err.trim()));
        }
    }

    Ok(format!("Reverted changes in {}", file_path))
}

/// Stages a specific file (git add).
pub async fn git_stage_file(cwd: &str, file_path: &str) -> Result<String, String> {
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Err("Invalid or missing workspace directory".to_string());
    }

    let add_out = super::create_hidden_command("git")
        .args(["add", "--", file_path])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to stage file: {e}"))?;

    if !add_out.status.success() {
        let err = String::from_utf8_lossy(&add_out.stderr);
        return Err(format!("Could not stage {}: {}", file_path, err.trim()));
    }

    Ok(format!("Staged {}", file_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_lines_correctly() {
        let raw = "\
 M src/main.rs
M  Cargo.toml
?? new_file.txt
 D deleted.txt
";
        let changes = parse_porcelain_output(raw);
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0].path, "src/main.rs");
        assert_eq!(changes[0].status, "M");
        assert_eq!(changes[1].path, "Cargo.toml");
        assert_eq!(changes[1].status, "M");
        assert_eq!(changes[2].path, "new_file.txt");
        assert_eq!(changes[2].status, "??");
        assert_eq!(changes[3].path, "deleted.txt");
        assert_eq!(changes[3].status, "D");
    }

    #[test]
    fn parses_quoted_porcelain_paths() {
        let raw = "?? \"path with spaces/file.txt\"\n";
        let changes = parse_porcelain_output(raw);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "path with spaces/file.txt");
    }

    #[tokio::test]
    async fn non_existent_directory_returns_not_a_repo() {
        let status = check_git_status("D:\\non_existent_folder_xyz_123")
            .await
            .unwrap();
        assert!(!status.is_repo);
        assert!(!status.has_changes);
        assert!(status.files.is_empty());
    }

    #[tokio::test]
    async fn empty_commit_message_is_rejected() {
        let err = git_commit("D:\\AetherAgent", "   ").await.unwrap_err();
        assert_eq!(err, "Commit message cannot be empty");
    }
}
