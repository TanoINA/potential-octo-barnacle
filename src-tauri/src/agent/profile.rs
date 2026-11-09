//! Startup-only profile routing. Configure before constructing a backend or worker.
//! No environment mutation, baseline fallback, or process-global reset is permitted.
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};
static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Pure selection/lexical validation; does not read or create profile files.
pub fn resolve_root(home: &Path, requested: Option<&Path>) -> Result<PathBuf, String> {
    let path = requested
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".aether-agent-rune"));
    if !home.is_absolute()
        || !path.is_absolute()
        || path.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err("Profile must be an absolute path without parent traversal".into());
    }
    if path.components().any(|c| match c {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            name.ends_with('.') || name.ends_with(' ') || name.contains(':')
        }
        _ => false,
    }) {
        return Err("Ambiguous profile component (trailing dot/space or stream) refused".into());
    }
    if path.components().any(|c| {
        c.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(".aether-agent")
    }) || home.join(".aether-agent").starts_with(&path)
    {
        return Err("Production .aether-agent cannot be a Rune write profile; use --import-profile to copy it".into());
    }
    Ok(path)
}
fn no_reparse(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                let mut linked = metadata.file_type().is_symlink();
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    linked |= metadata.file_attributes() & 0x400 != 0;
                }
                if linked {
                    return Err(format!(
                        "Symlink/reparse path refused: {}",
                        ancestor.display()
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
// Validate existing descendants as well: a plain root containing a junction at
// sessions/ or config.json must not redirect later storage outside the profile.
fn no_reparse_tree(path: &Path, depth: usize) -> Result<(), String> {
    no_reparse(path)?;
    if depth > 32 {
        return Err("Profile nesting exceeds safety limit".into());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            no_reparse_tree(&entry.map_err(|e| e.to_string())?.path(), depth + 1)?;
        }
    }
    Ok(())
}
fn directory(path: &Path) -> Result<PathBuf, String> {
    no_reparse(path)?;
    if !path.exists() {
        let parent = path.parent().ok_or("Profile needs a parent directory")?;
        directory(parent)?;
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    no_reparse(path)?;
    if !path.is_dir() {
        return Err("Profile must be a directory".into());
    }
    fs::canonicalize(path).map_err(|e| e.to_string())
}
/// One successful configuration per process. Call before any storage operation.
/// A second call (even the same directory) is an error, never a reset.
pub fn configure_storage_root(path: impl AsRef<Path>) -> Result<PathBuf, String> {
    if ROOT.get().is_some() {
        return Err("Storage root already configured".into());
    }
    let home =
        dirs::home_dir().ok_or("Home directory unavailable; profile cannot be selected safely")?;
    let selected = resolve_root(&home, Some(path.as_ref()))?;
    no_reparse(&selected)?;
    let selected = prospective_path(&selected)?;
    resolve_root(&home, Some(&selected))?;
    let canonical = directory(&selected)?;
    resolve_root(&home, Some(&canonical))?;
    no_reparse_tree(&canonical, 0)?;
    ROOT.set(canonical.clone())
        .map_err(|_| "Storage root already configured".to_string())?;
    Ok(canonical)
}
/// Production shared-profile callers fail closed on an unsafe default, never use
/// the desktop baseline. Unit fixtures retain their existing isolated test roots.
pub fn root() -> Option<PathBuf> {
    #[cfg(test)]
    {
        ROOT.get().cloned()
    }
    #[cfg(not(test))]
    {
        Some(
            ROOT.get_or_init(|| {
                let home =
                    dirs::home_dir().expect("Home directory required for isolated Rune profile");
                let selected = resolve_root(&home, None).expect("Invalid isolated Rune profile");
                let canonical =
                    directory(&selected).expect("Unsafe/unavailable isolated Rune profile");
                resolve_root(&home, Some(&canonical)).expect("Baseline write profile refused");
                no_reparse_tree(&canonical, 0).expect("Unsafe descendant in isolated Rune profile");
                canonical
            })
            .clone(),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!("aether-profile-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn resolver_is_pure_and_defaults_to_separate_rune_profile() {
        let home = fixture().join("not-created-home");
        assert_eq!(
            resolve_root(&home, None).unwrap(),
            home.join(".aether-agent-rune")
        );
        assert!(!home.exists(), "resolving must not create directories");
        for path in [
            home.join(".aether-agent"),
            home.join(".aether-agent/sessions"),
            home.join("bad/../.aether-agent"),
            PathBuf::from("relative"),
        ] {
            assert!(
                resolve_root(&home, Some(&path)).is_err(),
                "must reject {}",
                path.display()
            );
        }
    }
    #[test]
    fn configure_is_locked_in_a_fresh_child_and_routes_real_settings() {
        if std::env::var_os("AETHER_PROFILE_CHILD").is_some() {
            let path = fixture().join("rune");
            let unsafe_root = fixture();
            let outside = fixture();
            let linked = unsafe_root.join("sessions");
            #[cfg(windows)]
            {
                let status = std::process::Command::new("cmd")
                    .args(["/c", "mklink", "/J"])
                    .arg(&linked)
                    .arg(&outside)
                    .status()
                    .unwrap();
                assert!(status.success());
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, &linked).unwrap();
            assert!(
                configure_storage_root(&unsafe_root).is_err(),
                "nested reparse must not route storage into another profile"
            );
            let canonical = configure_storage_root(&path).unwrap();
            assert_eq!(canonical, std::fs::canonicalize(&path).unwrap());
            assert_eq!(super::super::storage::config_dir(), canonical);
            assert!(
                configure_storage_root(&path).is_err(),
                "even same root cannot reset configuration"
            );
            assert!(configure_storage_root(fixture()).is_err());
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let settings = super::super::storage::load_settings().await.unwrap();
                assert!(!settings.providers.is_empty());
                assert!(canonical.join("config.json").is_file());
            });
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "agent::profile::tests::configure_is_locked_in_a_fresh_child_and_routes_real_settings", "--nocapture"])
            .env("AETHER_PROFILE_CHILD", "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
/// Explicit opt-in copy of canonical settings/workspace/sessions and valid recovery
/// sidecars. Not a backup/mirror: derived projections, tasks and vectors are omitted.
/// The source is read-only. Validation precedes target creation; I/O failures retain
/// partial output for inspection and never roll back by deleting user files.
pub fn import_profile(source: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<usize, String> {
    use std::io::Write;
    let source = source.as_ref();
    if !source.is_absolute()
        || source
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err("Import source must be absolute without parent traversal".into());
    }
    no_reparse(source)?;
    let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
    if !source.is_dir() {
        return Err("Import source must be a directory".into());
    }
    let home = dirs::home_dir().ok_or("Home directory unavailable")?;
    let target = resolve_root(&home, Some(target.as_ref()))?;
    no_reparse(&target)?;
    let target = prospective_path(&target)?;
    resolve_root(&home, Some(&target))?;
    if target.starts_with(&source) || source.starts_with(&target) {
        return Err("Import source and target must not overlap".into());
    }
    if target.exists()
        && (!target.is_dir()
            || fs::read_dir(&target)
                .map_err(|e| e.to_string())?
                .next()
                .is_some())
    {
        return Err(
            "Import target must be absent or an empty directory; nothing overwritten".into(),
        );
    }
    let mut plan: Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> = Vec::new();
    for name in ["config.json", "workspace.json"] {
        let path = source.join(name);
        no_reparse(&path)?;
        if path.exists() {
            let (bytes, modified) = import_bytes(&path)?;
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if !value.is_object() {
                return Err(format!("{name} must be a JSON object"));
            }
            if name == "config.json" {
                serde_json::from_value::<super::types::AgentSettings>(value)
                    .map_err(|e| format!("Invalid settings: {e}"))?;
            } else if value.get("cwd").and_then(|v| v.as_str()).is_none() {
                return Err("workspace.json must contain a string cwd".into());
            }
            plan.push((PathBuf::from(name), bytes, modified));
        }
    }
    let sessions = source.join("sessions");
    no_reparse(&sessions)?;
    if sessions.exists() {
        for entry in fs::read_dir(&sessions).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            no_reparse(&path)?;
            if path.extension().is_some_and(|e| e == "checkpoint")
                && !path.with_extension("json").is_file()
            {
                return Err("Orphan checkpoint refused; preserve the source for recovery".into());
            }
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("Non-UTF8 session ID")?;
            safe_session_id(stem)?;
            if path.with_extension("deleted").exists() {
                return Err("Deleted-session tombstone refused".into());
            }
            let (bytes, modified) = import_bytes(&path)?;
            let data = super::storage::parse_session(
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
            )?;
            safe_session_id(&data.session.id)?;
            if data.session.id != stem {
                return Err("Session filename/ID mismatch".into());
            }
            plan.push((
                PathBuf::from("sessions").join(path.file_name().unwrap()),
                bytes.clone(),
                modified,
            ));
            let sidecar = path.with_extension("checkpoint");
            no_reparse(&sidecar)?;
            if sidecar.exists() {
                let (checkpoint, time) = import_bytes(&sidecar)?;
                validate_checkpoint(&path, stem, &bytes, &checkpoint)?;
                plan.push((
                    PathBuf::from("sessions").join(sidecar.file_name().unwrap()),
                    checkpoint,
                    time,
                ));
            }
            let total: usize = plan.iter().map(|(_, bytes, _)| bytes.len()).sum();
            if plan.len() > 10000 || total > 256 * 1024 * 1024 {
                return Err("Import exceeds 10000 files / 256 MiB; use a smaller snapshot".into());
            }
        }
    }
    if plan.is_empty() {
        return Err("Source contains no importable profile data".into());
    }
    let target = directory(&target)?;
    if fs::read_dir(&target)
        .map_err(|e| e.to_string())?
        .next()
        .is_some()
    {
        return Err("Import target became nonempty".into());
    }
    let count = plan.len();
    for (relative, bytes, modified) in plan {
        let destination = target.join(relative);
        directory(destination.parent().unwrap())?;
        no_reparse(&destination)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        // Checkpoints guard the canonical size + nanosecond mtime, not its path.
        file.set_times(fs::FileTimes::new().set_modified(modified))
            .map_err(|e| e.to_string())?;
    }
    Ok(count)
}
fn prospective_path(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|e| e.to_string());
    }
    let parent = path.parent().ok_or("Profile has no existing ancestor")?;
    Ok(prospective_path(parent)?.join(path.file_name().ok_or("Invalid profile path")?))
}
fn import_bytes(path: &Path) -> Result<(Vec<u8>, std::time::SystemTime), String> {
    no_reparse(path)?;
    let before = fs::metadata(path).map_err(|e| e.to_string())?;
    if !before.is_file() || before.len() > 64 * 1024 * 1024 {
        return Err("Import requires regular files at most 64 MiB each".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let after = fs::metadata(path).map_err(|e| e.to_string())?;
    let time = before.modified().map_err(|e| e.to_string())?;
    if before.len() != after.len() || time != after.modified().map_err(|e| e.to_string())? {
        return Err(
            "Source changed during import; close the source app and retry from a snapshot".into(),
        );
    }
    Ok((bytes, time))
}
fn safe_session_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Invalid session ID; path escapes refused".into());
    }
    Ok(())
}
// Same typed serialization/field order as session_journal's checksum contract.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportJournal {
    version: u32,
    session_id: String,
    run_id: String,
    base_revision: String,
    base_stamp: String,
    sequence: u64,
    messages: Vec<super::types::ChatMessage>,
    artifacts: Vec<super::types::Artifact>,
    request_hash: String,
    target_hash: Option<String>,
    checksum: String,
}
fn import_hash(bytes: &[u8]) -> String {
    let mut value = 0xcbf29ce484222325u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    format!("{value:016x}")
}
fn validate_checkpoint(
    path: &Path,
    id: &str,
    canonical: &[u8],
    bytes: &[u8],
) -> Result<(), String> {
    let mut journal: ImportJournal =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid checkpoint: {e}"))?;
    let checksum = std::mem::take(&mut journal.checksum);
    if journal.version != 1
        || journal.session_id != id
        || journal.run_id.is_empty()
        || journal.base_revision.is_empty()
        || checksum != import_hash(&serde_json::to_vec(&journal).map_err(|e| e.to_string())?)
    {
        return Err("Invalid checkpoint identity/version/checksum".into());
    }
    if journal.base_stamp != super::session_pages::fingerprint(path)?
        && journal.target_hash.as_ref() != Some(&import_hash(canonical))
    {
        return Err(
            "Checkpoint does not belong to the canonical session; conflict preserved in source"
                .into(),
        );
    }
    Ok(())
}
#[cfg(test)]
mod import_tests {
    use super::*;
    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!("aether-import-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn import_validates_before_copy_and_never_overwrites_or_copies_projection() {
        let parent = fixture();
        let source = parent.join(".aether-agent");
        let target = parent.join("rune");
        fs::create_dir_all(source.join("sessions")).unwrap();
        fs::write(
            source.join("config.json"),
            serde_json::to_vec(&super::super::storage::default_settings()).unwrap(),
        )
        .unwrap();
        fs::write(source.join("workspace.json"), b"{\"cwd\":\"\"}").unwrap();
        fs::write(
            source.join("sessions/sess_1.json"),
            br#"{"id":"sess_1","messages":[]}"#,
        )
        .unwrap();
        fs::write(
            source.join("session-pages.sqlite"),
            b"derived and path-specific",
        )
        .unwrap();
        assert_eq!(import_profile(&source, &target).unwrap(), 3);
        assert!(!target.join("session-pages.sqlite").exists());
        assert_eq!(
            fs::read(source.join("config.json")).unwrap(),
            fs::read(target.join("config.json")).unwrap()
        );
        assert!(import_profile(&source, &target).is_err());
        let invalid_target = parent.join("invalid");
        fs::write(
            source.join("sessions/sess_1.json"),
            br#"{"id":"../escape","messages":[]}"#,
        )
        .unwrap();
        assert!(import_profile(&source, &invalid_target).is_err());
        assert!(
            !invalid_target.exists(),
            "schema/path validation must happen before creating a target"
        );
        fs::write(source.join("sessions/sess_1.json"), b"invalid JSON").unwrap();
        assert!(import_profile(&source, parent.join("invalid-json")).is_err());
        assert!(import_profile(&source, source.join("nested")).is_err());
    }
}
#[cfg(test)]
mod safety_tests {
    use super::*;
    #[test]
    fn resolver_rejects_windows_alias_components_before_filesystem_creation() {
        let home = std::env::temp_dir().join("profile-home");
        for name in [
            ".aether-agent.",
            ".aether-agent ",
            "rune:stream",
            "rune.",
            "rune ",
        ] {
            assert!(
                resolve_root(&home, Some(&home.join(name))).is_err(),
                "alias must fail: {name}"
            );
        }
    }
    #[test]
    fn valid_checkpoint_is_copied_with_matching_canonical_stamp() {
        let parent =
            std::env::temp_dir().join(format!("aether-journal-import-{}", uuid::Uuid::new_v4()));
        let source = parent.join("source");
        let target = parent.join("target");
        fs::create_dir_all(source.join("sessions")).unwrap();
        let path = source.join("sessions/one.json");
        fs::write(&path, br#"{"id":"one","messages":[]}"#).unwrap();
        let mut journal = ImportJournal {
            version: 1,
            session_id: "one".into(),
            run_id: "run".into(),
            base_revision: "revision".into(),
            base_stamp: super::super::session_pages::fingerprint(&path).unwrap(),
            sequence: 1,
            messages: vec![],
            artifacts: vec![],
            request_hash: "hash".into(),
            target_hash: None,
            checksum: String::new(),
        };
        journal.checksum = import_hash(&serde_json::to_vec(&journal).unwrap());
        fs::write(
            path.with_extension("checkpoint"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        assert_eq!(import_profile(&source, &target).unwrap(), 2);
        assert_eq!(
            super::super::session_pages::fingerprint(&target.join("sessions/one.json")).unwrap(),
            journal.base_stamp
        );
        fs::write(path.with_extension("checkpoint"), b"{}").unwrap();
        assert!(import_profile(&source, parent.join("corrupt")).is_err());
        assert!(!parent.join("corrupt").exists());
    }
    #[test]
    fn reparse_source_is_refused() {
        let parent =
            std::env::temp_dir().join(format!("aether-link-import-{}", uuid::Uuid::new_v4()));
        let source = parent.join("source");
        let outside = parent.join("outside");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir(&outside).unwrap();
        let linked = source.join("sessions");
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&linked)
                .arg(&outside)
                .status()
                .unwrap();
            assert!(
                status.success(),
                "junction fixture must be created, not silently skipped"
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &linked).unwrap();
        assert!(import_profile(&source, parent.join("target")).is_err());
        assert!(no_reparse(&linked.join("child")).is_err());
    }
}
