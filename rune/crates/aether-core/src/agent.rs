//! The authoritative backend implementations live beside the existing desktop host.
//! Only the native updater (`release.rs`) is excluded from this renderer-neutral crate.
#[path = "../../../../src-tauri/src/agent/events.rs"]
pub mod events;
#[path = "../../../../src-tauri/src/agent/git.rs"]
pub mod git;
#[path = "../../../../src-tauri/src/agent/history_maintenance.rs"]
pub mod history_maintenance;
#[path = "../../../../src-tauri/src/agent/images.rs"]
pub mod images;
#[path = "../../../../src-tauri/src/agent/mcp.rs"]
pub mod mcp;
#[cfg(feature = "diagnostics")]
#[path = "../../../../src-tauri/src/agent/native_smoke.rs"]
pub mod native_smoke;
#[cfg(feature = "shared-profile")]
#[path = "../../../../src-tauri/src/agent/profile.rs"]
pub mod profile;
#[path = "../../../../src-tauri/src/agent/provider.rs"]
pub mod provider;
#[path = "../../../../src-tauri/src/agent/pty.rs"]
pub mod pty;
#[path = "../../../../src-tauri/src/agent/reflection.rs"]
pub mod reflection;
#[path = "../../../../src-tauri/src/agent/release_policy.rs"]
pub mod release_policy;
#[path = "../../../../src-tauri/src/agent/review.rs"]
pub mod review;
#[path = "../../../../src-tauri/src/agent/review_pending.rs"]
pub mod review_pending;
#[path = "../../../../src-tauri/src/agent/review_skills.rs"]
pub mod review_skills;
#[path = "../../../../src-tauri/src/agent/runner.rs"]
pub mod runner;
#[path = "../../../../src-tauri/src/agent/search.rs"]
pub mod search;
#[path = "../../../../src-tauri/src/agent/session_journal.rs"]
pub mod session_journal;
#[path = "../../../../src-tauri/src/agent/session_pages.rs"]
pub mod session_pages;
#[path = "../../../../src-tauri/src/agent/skills.rs"]
pub mod skills;
#[path = "../../../../src-tauri/src/agent/storage.rs"]
pub mod storage;
#[path = "../../../../src-tauri/src/agent/tasks.rs"]
pub mod tasks;
#[path = "../../../../src-tauri/src/agent/tools.rs"]
pub mod tools;
#[path = "../../../../src-tauri/src/agent/types.rs"]
pub mod types;

pub use events::{AgentHost, EventSink};
pub use types::*;

/// Creates a new `tokio::process::Command` configured with `CREATE_NO_WINDOW` (0x08000000)
/// on Windows to prevent console/terminal windows (cmd/PowerShell/conhost/Windows Terminal)
/// from popping up during tool execution in GUI applications.
pub fn create_hidden_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd
}
