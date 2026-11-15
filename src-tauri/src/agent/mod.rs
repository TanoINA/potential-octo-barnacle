#[cfg(feature = "shared-profile")]
pub mod profile;
pub mod events;
pub mod git;
pub mod images;
pub mod mcp;
#[cfg(feature = "diagnostics")]
pub mod native_smoke;
pub mod provider;
pub mod pty;
pub mod reflection;
pub mod review;
pub mod review_pending;
pub mod review_skills;
pub mod runner;
pub mod search;
pub mod session_pages;
pub mod skills;
pub mod storage;
pub mod tasks;
pub mod tools;
pub mod types;

/// Creates a new `tokio::process::Command` configured with `CREATE_NO_WINDOW` (0x08000000)
/// on Windows to prevent console/terminal windows (cmd/PowerShell/conhost/Windows Terminal)
/// from popping up during tool execution in GUI applications.
pub fn create_hidden_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd
}

pub mod session_journal;

pub mod history_maintenance;

pub mod release;
mod release_download;
pub mod release_policy;
