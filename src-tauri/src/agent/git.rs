use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitFileChange {
    pub path: String,
    pub status: String,
    #[serde(rename = "oldPath", skip_serializing_if = "Option::is_none", default)]
    pub old_path: Option<String>,
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
#[cfg(test)]
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
            files.push(GitFileChange {
                path,
                status,
                old_path: None,
            });
        }
    }
    files
}

fn ensure_success(output: &std::process::Output, operation: &str) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    let stderr: String = String::from_utf8_lossy(&output.stderr)
        .chars()
        .take(4096)
        .collect();
    Err(format!(
        "git {operation} failed ({}): {}",
        output.status,
        stderr.trim()
    ))
}

fn parse_porcelain_z(raw: &[u8], prefix: &str) -> Result<Vec<GitFileChange>, String> {
    let mut records = raw.split(|byte| *byte == 0);
    let mut files = Vec::new();
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if record.len() < 4 || !record[..2].is_ascii() || record[2] != b' ' {
            return Err("Malformed git porcelain record".into());
        }
        let status = std::str::from_utf8(&record[..2])
            .map_err(|e| e.to_string())?
            .trim()
            .to_string();
        let path = std::str::from_utf8(&record[3..]).map_err(|_| "Git path is not UTF-8")?;
        let old_path = if record[..2].iter().any(|b| matches!(b, b'R' | b'C')) {
            let old = records
                .next()
                .filter(|old| !old.is_empty())
                .ok_or("Missing git rename source")?;
            Some(
                std::str::from_utf8(old)
                    .map_err(|_| "Git rename path is not UTF-8")?
                    .to_string(),
            )
        } else {
            None
        };
        if let Some(relative) = path.strip_prefix(prefix) {
            files.push(GitFileChange {
                path: relative.to_string(),
                status,
                old_path: old_path.and_then(|old| old.strip_prefix(prefix).map(str::to_string)),
            });
        }
    }
    Ok(files)
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
        .await
        .map_err(|e| format!("Could not execute git: {e}"))?;

    match check {
        out if out.status.success() => {}
        out => {
            if !String::from_utf8_lossy(&out.stderr).contains("not a git repository") {
                ensure_success(&out, "rev-parse")?;
            }
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

    ensure_success(&branch_out, "branch")?;
    let prefix_out = super::create_hidden_command("git")
        .args(["rev-parse", "--show-prefix"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Could not read repository prefix: {e}"))?;
    ensure_success(&prefix_out, "rev-parse prefix")?;
    let prefix = String::from_utf8(prefix_out.stdout).map_err(|_| "Git prefix is not UTF-8")?;
    let prefix = prefix.trim_end_matches(['\r', '\n']);
    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();

    // 3. Check status
    let status_out = super::create_hidden_command("git")
        .args([
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "-z",
            "--",
            ".",
        ])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to check git status: {e}"))?;

    ensure_success(&status_out, "status")?;
    let files = parse_porcelain_z(&status_out.stdout, prefix)?;
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

/// Stages workspace changes and commits them, rejecting pre-staged siblings.
pub async fn git_commit(cwd: &str, message: &str) -> Result<String, String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err("Commit message cannot be empty".to_string());
    }
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Err("Invalid or missing workspace directory".to_string());
    }

    // The status UI is workspace-scoped. Inspect the entire index before any
    // staging: a plain commit would also consume hidden, pre-staged siblings.
    let prefix_out = super::create_hidden_command("git")
        .args(["rev-parse", "--show-prefix"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Could not read repository prefix: {e}"))?;
    ensure_success(&prefix_out, "rev-parse prefix")?;
    let prefix = String::from_utf8(prefix_out.stdout).map_err(|_| "Git prefix is not UTF-8")?;
    let prefix = prefix.trim_end_matches(['\r', '\n']);
    if !prefix.is_empty() {
        let staged_out = super::create_hidden_command("git")
            // Disable rename folding so both sides of a cross-boundary rename
            // are checked. --no-relative overrides a user's diff.relative.
            .args([
                "diff",
                "--cached",
                "--name-only",
                "--no-renames",
                "--no-relative",
                "-z",
            ])
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| format!("Could not inspect staged paths: {e}"))?;
        ensure_success(&staged_out, "diff --cached")?;
        if staged_out
            .stdout
            .split(|byte| *byte == 0)
            .any(|path| !path.is_empty() && !path.starts_with(prefix.as_bytes()))
        {
            return Err("Cannot commit: staged changes exist outside the workspace. Commit or unstage them from the repository root first.".into());
        }
    }

    let add_out = super::create_hidden_command("git")
        .args(["--literal-pathspecs", "add", "-A", "--", "."])
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

    // Keep the commit itself scoped too, so a sibling staged by another Git
    // client after the preflight cannot enter this workspace's commit.
    let mut commit_command = super::create_hidden_command("git");
    commit_command.args(["--literal-pathspecs", "commit", "-m", msg]);
    if !prefix.is_empty() {
        commit_command.args(["--only", "--", "."]);
    }
    let commit_out = commit_command
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

fn validated_file_path(cwd: &str, requested: &str) -> Result<(std::path::PathBuf, String), String> {
    if requested.is_empty() || requested.starts_with(":(") {
        return Err("Expected one literal workspace file path".into());
    }
    let path = super::tools::resolve_workspace_path(cwd, requested)?;
    let relative = super::tools::workspace_relative_path(cwd, &path)?;
    if relative.is_empty() || path.is_dir() {
        return Err("Expected a file, not the workspace root or a directory".into());
    }
    Ok((path, relative))
}

/// Reverts changes made to a specific file (git checkout/restore, or removes untracked file).
pub async fn git_revert_file(cwd: &str, file_path: &str) -> Result<String, String> {
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Err("Invalid or missing workspace directory".to_string());
    }
    let (full_path, rel_str) = validated_file_path(cwd, file_path)?;

    // Check status of this file
    let status_out = super::create_hidden_command("git")
        .args([
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "-z",
            "--",
            &rel_str,
        ])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to inspect file status: {e}"))?;
    ensure_success(&status_out, "status")?;
    {
        let out = status_out;
        let text = String::from_utf8_lossy(&out.stdout);
        if text.trim().starts_with("??") {
            // Untracked file: safely remove it if it exists
            if full_path.is_file() {
                std::fs::remove_file(&full_path)
                    .map_err(|e| format!("Failed to remove untracked file: {e}"))?;
                return Ok(format!("Removed untracked file {}", rel_str));
            }
        }
    }

    // Attempt git checkout
    let checkout_out = super::create_hidden_command("git")
        .args(["--literal-pathspecs", "checkout", "--", &rel_str])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to revert file changes: {e}"))?;

    if !checkout_out.status.success() {
        // Fallback to git restore
        let restore_out = super::create_hidden_command("git")
            .args(["--literal-pathspecs", "restore", "--", &rel_str])
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| format!("Failed to restore file: {e}"))?;

        if !restore_out.status.success() {
            let err = String::from_utf8_lossy(&restore_out.stderr);
            return Err(format!("Could not revert {}: {}", rel_str, err.trim()));
        }
    }

    Ok(format!("Reverted changes in {}", rel_str))
}

/// Stages a specific file (git add).
pub async fn git_stage_file(cwd: &str, file_path: &str) -> Result<String, String> {
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Err("Invalid or missing workspace directory".to_string());
    }
    let (_, rel_str) = validated_file_path(cwd, file_path)?;

    let add_out = super::create_hidden_command("git")
        .args(["--literal-pathspecs", "add", "--", &rel_str])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to stage file: {e}"))?;

    if !add_out.status.success() {
        let err = String::from_utf8_lossy(&add_out.stderr);
        return Err(format!("Could not stage {}: {}", rel_str, err.trim()));
    }

    Ok(format!("Staged {}", rel_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> std::path::PathBuf {
        let root = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("aether-git-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("docs")).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "core.autocrlf", "false"],
            vec!["config", "user.name", "Regression"],
            vec!["config", "user.email", "regression@example.invalid"],
        ] {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap()
                .status
                .success());
        }
        for name in ["[ab].txt", "a.txt"] {
            std::fs::write(root.join("docs").join(name), "original\n").unwrap();
        }
        assert!(std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&root)
            .output()
            .unwrap()
            .status
            .success());
        assert!(std::process::Command::new("git")
            .args(["commit", "-qm", "fixture"])
            .current_dir(&root)
            .output()
            .unwrap()
            .status
            .success());
        root
    }

    fn git_output(root: &Path, args: &[&str]) -> Vec<u8> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        output.stdout
    }

    #[tokio::test]
    async fn regression_commit_scope_excludes_hidden_sibling_changes() {
        let root = repo();
        std::fs::write(root.join("outside.txt"), "original outside\n").unwrap();
        git_output(&root, &["add", "."]);
        git_output(&root, &["commit", "-qm", "outside fixture"]);
        std::fs::write(root.join("outside.txt"), "hidden modification\n").unwrap();
        std::fs::write(root.join("outside-new.txt"), "hidden addition\n").unwrap();
        std::fs::write(root.join("docs/a.txt"), "visible modification\n").unwrap();
        std::fs::write(root.join("docs/new.txt"), "visible addition\n").unwrap();
        std::fs::remove_file(root.join("docs/[ab].txt")).unwrap();
        let workspace = root.join("docs");
        let status = check_git_status(&workspace.to_string_lossy())
            .await
            .unwrap();
        assert_eq!(status.files.len(), 3);
        git_commit(&workspace.to_string_lossy(), "workspace only")
            .await
            .unwrap();
        let committed = git_output(
            &root,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                "-z",
                "HEAD",
            ],
        );
        assert_eq!(committed, b"docs/[ab].txt\0docs/a.txt\0docs/new.txt\0");
        assert_eq!(
            git_output(&root, &["show", "HEAD:outside.txt"]),
            b"original outside\n"
        );
        assert_eq!(
            git_output(&root, &["diff", "--cached", "--name-only", "-z"]),
            b""
        );
        assert!(
            !check_git_status(&workspace.to_string_lossy())
                .await
                .unwrap()
                .has_changes
        );
        assert_eq!(
            std::fs::read_to_string(root.join("outside.txt")).unwrap(),
            "hidden modification\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("outside-new.txt")).unwrap(),
            "hidden addition\n"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn regression_commit_scope_rejects_external_index_before_any_mutation() {
        for crossing_rename in [false, true] {
            let root = repo();
            if crossing_rename {
                git_output(&root, &["mv", "docs/a.txt", "outside.txt"]);
            } else {
                std::fs::write(root.join("outside.txt"), "hidden staged\n").unwrap();
                git_output(&root, &["add", "outside.txt"]);
                std::fs::write(root.join("outside.txt"), "hidden unstaged\n").unwrap();
            }
            std::fs::write(root.join("docs/[ab].txt"), "inside staged\n").unwrap();
            git_output(
                &root,
                &["--literal-pathspecs", "add", "--", "docs/[ab].txt"],
            );
            std::fs::write(root.join("docs/[ab].txt"), "inside unstaged\n").unwrap();
            let head = git_output(&root, &["rev-parse", "HEAD"]);
            let index = std::fs::read(root.join(".git/index")).unwrap();
            let worktree = git_output(&root, &["diff", "--binary"]);
            let result = git_commit(&root.join("docs").to_string_lossy(), "must reject").await;
            assert!(
                result
                    .as_ref()
                    .is_err_and(|error| error.contains("outside the workspace")),
                "{result:?}"
            );
            assert_eq!(git_output(&root, &["rev-parse", "HEAD"]), head);
            assert_eq!(
                std::fs::read(root.join(".git/index")).unwrap(),
                index,
                "rejection mutated the index before staging"
            );
            assert_eq!(git_output(&root, &["diff", "--binary"]), worktree);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/[ab].txt")).unwrap(),
                "inside unstaged\n"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn regression_commit_scope_root_keeps_repository_wide_behavior() {
        let root = repo();
        std::fs::write(root.join("docs/a.txt"), "inside\n").unwrap();
        std::fs::write(root.join("outside.txt"), "outside\n").unwrap();
        git_output(&root, &["add", "outside.txt"]);
        git_commit(&root.to_string_lossy(), "whole repository")
            .await
            .unwrap();
        assert_eq!(
            git_output(
                &root,
                &[
                    "diff-tree",
                    "--no-commit-id",
                    "--name-only",
                    "-r",
                    "-z",
                    "HEAD"
                ]
            ),
            b"docs/a.txt\0outside.txt\0"
        );
        assert!(
            !check_git_status(&root.to_string_lossy())
                .await
                .unwrap()
                .has_changes
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn literal_stage_and_revert_do_not_touch_glob_neighbor() {
        let root = repo();
        let cwd = root.to_string_lossy();
        std::fs::write(root.join("docs/[ab].txt"), "selected\n").unwrap();
        std::fs::write(root.join("docs/a.txt"), "neighbor\n").unwrap();
        git_stage_file(&cwd, "docs/[ab].txt").await.unwrap();
        let staged = std::process::Command::new("git")
            .args(["diff", "--cached", "--name-only", "-z"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(staged.stdout, b"docs/[ab].txt\0");
        std::fs::write(root.join("docs/[ab].txt"), "unstaged\n").unwrap();
        git_revert_file(&cwd, "docs/[ab].txt").await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("docs/[ab].txt")).unwrap(),
            "selected\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("docs/a.txt")).unwrap(),
            "neighbor\n"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn per_file_git_rejects_workspace_escape_and_directory() {
        let root = repo();
        let cwd = root.join("docs");
        for path in [
            "../docs/a.txt",
            ":(top)**",
            ".",
            "",
            "C:/outside.txt",
            "//invalid-host/share/file",
        ] {
            assert!(
                git_stage_file(&cwd.to_string_lossy(), path).await.is_err(),
                "accepted {path}"
            );
            assert!(
                git_revert_file(&cwd.to_string_lossy(), path).await.is_err(),
                "accepted {path}"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn corrupt_index_is_error_not_clean_tree() {
        let root = repo();
        std::fs::write(root.join(".git/index"), "corrupt index").unwrap();
        let result = check_git_status(&root.to_string_lossy()).await;
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.is_err(), "corrupt index returned {result:?}");
    }

    #[tokio::test]
    async fn status_preserves_rename_unicode_and_workspace_relative_names() {
        let root = repo();
        let new_name = "docs/雪 space.txt";
        let output = std::process::Command::new("git")
            .args(["mv", "docs/a.txt", new_name])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(output.status.success());
        std::fs::write(root.join("outside.txt"), "outside").unwrap();
        let result = check_git_status(&root.join("docs").to_string_lossy())
            .await
            .unwrap();
        assert_eq!(result.files.len(), 1, "{:?}", result.files);
        assert_eq!(result.files[0].path, "雪 space.txt");
        assert_eq!(result.files[0].status, "R");
        assert_eq!(result.files[0].old_path.as_deref(), Some("a.txt"));
        std::fs::remove_dir_all(root).unwrap();
    }

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
