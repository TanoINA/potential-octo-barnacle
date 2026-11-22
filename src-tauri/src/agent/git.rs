use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_GIT_OUTPUT_BYTES: usize = 1024 * 1024;

// Keep draining after the capture limit: a verbose hook must not deadlock on a
// full pipe. The deadline covers both process exit and EOF on inherited pipes.
async fn read_git_output(mut pipe: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut overflowed = false;
    let mut buffer = [0; 8192];
    loop {
        let count = pipe.read(&mut buffer).await?;
        if count == 0 {
            return if overflowed {
                Err(std::io::Error::other(
                    "Git output exceeded 1 MiB capture limit",
                ))
            } else {
                Ok(captured)
            };
        }
        let keep = count.min(MAX_GIT_OUTPUT_BYTES.saturating_sub(captured.len()));
        overflowed |= keep < count;
        captured.extend_from_slice(&buffer[..keep]);
    }
}

struct GitChild(Option<Child>);

impl Drop for GitChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // Also clean up when the caller's future is dropped. kill_on_drop
            // remains the fallback if the runtime is already shutting down.
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move { terminate_git_child(&mut child).await });
            }
        }
    }
}

async fn terminate_git_child(child: &mut Child) {
    if let Some(pid) = child.id() {
        #[cfg(windows)]
        let mut killer = {
            let mut command = super::create_hidden_command("taskkill");
            command.args(["/PID", &pid.to_string(), "/T", "/F"]);
            command
        };
        #[cfg(unix)]
        let mut killer = {
            let mut command = super::create_hidden_command("kill");
            command.args(["-KILL", "--", &format!("-{pid}")]);
            command
        };
        #[cfg(any(windows, unix))]
        {
            killer
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let _ = tokio::time::timeout(Duration::from_secs(1), killer.status()).await;
        }
    }
    // Child::kill waits as well: explicitly reap even if the tree killer failed
    // or the child exited between the deadline and cleanup.
    let _ = tokio::time::timeout(Duration::from_secs(1), child.kill()).await;
    let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
}

// Also applied to shell tools: Git launched inside PowerShell/sh inherits
// non-interactive authentication while retaining the tool's bounded deadline.
pub(crate) fn configure_git_environment(command: &mut Command) {
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GIT_EDITOR", "true");
    // Do not replace core.sshCommand, GIT_SSH_COMMAND or Plink configuration.
    // The subprocess deadline also bounds custom SSH credential helpers.
}

async fn run_git_command(
    command: &mut Command,
    operation: &str,
    deadline: Duration,
) -> Result<Output, String> {
    configure_git_environment(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let child = command
        .spawn()
        .map_err(|error| format!("Failed to execute git {operation}: {error}"))?;
    let mut owned = GitChild(Some(child));
    let child = owned.0.as_mut().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let result = tokio::time::timeout(deadline, async {
        let (status, stdout, stderr) = tokio::try_join!(
            child.wait(),
            read_git_output(stdout),
            read_git_output(stderr)
        )?;
        Ok::<_, std::io::Error>(Output {
            status,
            stdout,
            stderr,
        })
    })
    .await;
    match result {
        Ok(Ok(output)) => {
            owned.0.take();
            Ok(output)
        }
        failure => {
            terminate_git_child(owned.0.as_mut().unwrap()).await;
            owned.0.take();
            match failure {
                Err(_) => Err(format!("git {operation} timed out after {:.3}s; process terminated (authentication and hooks must be non-interactive)", deadline.as_secs_f64())),
                Ok(Err(error)) => Err(format!("Failed to execute git {operation}: {error}")),
                Ok(Ok(_)) => unreachable!(),
            }
        }
    }
}

async fn git_output(cwd: &str, args: &[&str], operation: &str) -> Result<Output, String> {
    run_git_command(
        super::create_hidden_command("git")
            .args(args)
            .current_dir(cwd),
        operation,
        GIT_TIMEOUT,
    )
    .await
}

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
    let check = git_output(cwd, &["rev-parse", "--is-inside-work-tree"], "command").await?;

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
    let branch_out = git_output(cwd, &["branch", "--show-current"], "command").await?;

    ensure_success(&branch_out, "branch")?;
    let prefix_out = git_output(cwd, &["rev-parse", "--show-prefix"], "command").await?;
    ensure_success(&prefix_out, "rev-parse prefix")?;
    let prefix = String::from_utf8(prefix_out.stdout).map_err(|_| "Git prefix is not UTF-8")?;
    let prefix = prefix.trim_end_matches(['\r', '\n']);
    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();

    // 3. Check status
    let status_out = git_output(
        cwd,
        &[
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "-z",
            "--",
            ".",
        ],
        "command",
    )
    .await?;

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
    let prefix_out = git_output(cwd, &["rev-parse", "--show-prefix"], "command").await?;
    ensure_success(&prefix_out, "rev-parse prefix")?;
    let prefix = String::from_utf8(prefix_out.stdout).map_err(|_| "Git prefix is not UTF-8")?;
    let prefix = prefix.trim_end_matches(['\r', '\n']);
    if !prefix.is_empty() {
        let staged_out = git_output(
            cwd,
            &[
                "diff",
                "--cached",
                "--name-only",
                "--no-renames",
                "--no-relative",
                "-z",
            ],
            "command",
        )
        .await?;
        ensure_success(&staged_out, "diff --cached")?;
        if staged_out
            .stdout
            .split(|byte| *byte == 0)
            .any(|path| !path.is_empty() && !path.starts_with(prefix.as_bytes()))
        {
            return Err("Cannot commit: staged changes exist outside the workspace. Commit or unstage them from the repository root first.".into());
        }
    }

    let add_out = git_output(
        cwd,
        &["--literal-pathspecs", "add", "-A", "--", "."],
        "command",
    )
    .await?;

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
    let commit_out =
        run_git_command(commit_command.current_dir(cwd), "commit", GIT_TIMEOUT).await?;

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

    let push_out = git_output(cwd, &["push"], "push").await?;

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

/// Fetches remotes with the same non-interactive, bounded runner as push.
pub async fn git_fetch(cwd: &str) -> Result<String, String> {
    if cwd.trim().is_empty() || !Path::new(cwd).is_dir() {
        return Err("Invalid or missing workspace directory".into());
    }
    let output = git_output(cwd, &["fetch"], "fetch").await?;
    ensure_success(&output, "fetch")?;
    Ok(format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
    .trim()
    .to_string())
}

fn validated_file_path(cwd: &str, requested: &str) -> Result<(std::path::PathBuf, String), String> {
    if requested.is_empty() || requested.starts_with(":(") {
        return Err("Expected one literal workspace file path".into());
    }
    if requested.contains('\0') || requested.starts_with(r"\\?\") || requested.starts_with(r"\\.\")
    {
        return Err("NT device/namespace paths are not allowed".into());
    }
    let raw = Path::new(requested);
    if raw
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Parent traversal is not allowed; use a workspace-contained path".into());
    }
    let root =
        std::fs::canonicalize(cwd).map_err(|error| format!("Workspace is unavailable: {error}"))?;
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    if let (Some(parent), Some(name)) = (candidate.parent(), candidate.file_name()) {
        if let Ok(resolved_parent) = std::fs::canonicalize(parent) {
            if resolved_parent.starts_with(&root) {
                let lexical = resolved_parent.join(name);
                if let Ok(meta) = std::fs::symlink_metadata(&lexical) {
                    if meta.file_type().is_symlink() {
                        if meta.is_dir() {
                            return Err(
                                "Expected a file, not the workspace root or a directory".into(),
                            );
                        }
                        let relative = super::tools::workspace_relative_path(cwd, &lexical)?;
                        if relative.is_empty() {
                            return Err(
                                "Expected a file, not the workspace root or a directory".into(),
                            );
                        }
                        return Ok((lexical, relative));
                    }
                }
            }
        }
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
    let status_out = git_output(
        cwd,
        &[
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "-z",
            "--",
            &rel_str,
        ],
        "command",
    )
    .await?;
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
    let checkout_out = git_output(
        cwd,
        &["--literal-pathspecs", "checkout", "--", &rel_str],
        "command",
    )
    .await?;

    if !checkout_out.status.success() {
        // Fallback to git restore
        let restore_out = git_output(
            cwd,
            &["--literal-pathspecs", "restore", "--", &rel_str],
            "command",
        )
        .await?;

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

    let add_out = git_output(
        cwd,
        &["--literal-pathspecs", "add", "--", &rel_str],
        "command",
    )
    .await?;

    if !add_out.status.success() {
        let err = String::from_utf8_lossy(&add_out.stderr);
        return Err(format!("Could not stage {}: {}", rel_str, err.trim()));
    }

    Ok(format!("Staged {}", rel_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_command(mode: &str) -> tokio::process::Command {
        let mut command = super::super::create_hidden_command(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "agent::git::tests::git_process_fixture",
            "--nocapture",
        ]);
        command.env("AETHER_GIT_FIXTURE", mode);
        command
    }

    #[test]
    #[ignore = "subprocess fixture invoked explicitly by Git runner regressions"]
    fn git_process_fixture() {
        match std::env::var("AETHER_GIT_FIXTURE").unwrap().as_str() {
            "hang" => std::thread::sleep(std::time::Duration::from_secs(10)),
            "environment" => {
                assert_eq!(std::env::var("GIT_TERMINAL_PROMPT").unwrap(), "0");
                assert_eq!(std::env::var("GCM_INTERACTIVE").unwrap(), "Never");
                assert_eq!(
                    std::env::var("GIT_SSH_COMMAND").unwrap(),
                    "custom-ssh -i identity"
                );
                use std::io::Read;
                assert_eq!(std::io::stdin().read(&mut [0]).unwrap(), 0);
            }
            "stdout-overflow" | "stderr-overflow" => {
                use std::io::Write;
                let bytes = vec![b'x'; MAX_GIT_OUTPUT_BYTES + 1];
                if std::env::var("AETHER_GIT_FIXTURE").unwrap() == "stdout-overflow" {
                    std::io::stdout().write_all(&bytes).unwrap();
                } else {
                    std::io::stderr().write_all(&bytes).unwrap();
                }
            }
            mode => panic!("unknown fixture mode {mode}"),
        }
    }

    #[tokio::test]
    async fn git_runner_disables_interactive_authentication() {
        let output = run_git_command(
            fixture_command("environment").env("GIT_SSH_COMMAND", "custom-ssh -i identity"),
            "fetch",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn regression_git_timeout_is_bounded() {
        let started = std::time::Instant::now();
        let result = run_git_command(
            &mut fixture_command("hang"),
            "push",
            std::time::Duration::from_millis(100),
        )
        .await;
        assert!(
            result
                .as_ref()
                .is_err_and(|error| error.contains("git push timed out")),
            "{result:?}"
        );
        // 100 ms deadline plus at most three 1s cleanup waits, with scheduler margin.
        // The fixture sleeps 10s, so an unbounded runner cannot satisfy this.
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[tokio::test]
    async fn regression_git_output_overflow_fails_closed() {
        for mode in ["stdout-overflow", "stderr-overflow"] {
            let result =
                run_git_command(&mut fixture_command(mode), "status", Duration::from_secs(5)).await;
            assert!(
                result
                    .as_ref()
                    .is_err_and(|error| error.contains("output exceeded")),
                "{mode} returned partial output instead of an error: {:?}",
                result
                    .as_ref()
                    .map(|output| (output.stdout.len(), output.stderr.len()))
            );
        }
    }

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
