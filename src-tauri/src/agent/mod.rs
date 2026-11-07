pub mod git;
pub mod mcp;
pub mod provider;
pub mod pty;
pub mod reflection;
pub mod runner;
pub mod search;
pub mod skills;
pub mod storage;
pub mod tasks;
pub mod tools;
pub mod types;

/// Creates a new `tokio::process::Command` configured with `CREATE_NO_WINDOW` (0x08000000)
/// on Windows to prevent console/terminal windows (cmd/PowerShell/conhost/Windows Terminal)
/// from popping up during tool execution in GUI applications.
pub fn create_hidden_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd
}
