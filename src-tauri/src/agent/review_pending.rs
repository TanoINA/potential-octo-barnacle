//! Durable, session-scoped review proposals. Staging is a trusted caller boundary.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWrite {
    pub id: String,
    pub session_id: String,
    pub run_id: String,
    pub skill_name: String,
    pub target_path: String,
    pub skill_root: String,
    pub original: Option<String>,
    pub proposed: Option<String>,
}

const MAX_BYTES: usize = 256 * 1024;
const MISSING: &str = "Pending proposal not found";
static WRITER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn pending_root() -> Result<PathBuf, String> {
    #[cfg(feature = "shared-profile")]
    {
        Ok(super::storage::config_dir().join("pending/skills"))
    }
    #[cfg(not(feature = "shared-profile"))]
    {
        #[cfg(feature = "diagnostics")]
        if let Some(root) = super::native_smoke::profile() {
            return Ok(root.join("pending/skills"));
        }
        dirs::home_dir()
            .map(|home| home.join(".aether-agent/pending/skills"))
            .ok_or_else(|| "Home directory unavailable".into())
    }
}

pub async fn stage(root: &Path, mut write: PendingWrite) -> Result<String, String> {
    let _guard = WRITER.lock().await;
    let root = checked_path(root)?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    checked_path(&root)?;
    if write.id.is_empty() {
        write.id = uuid::Uuid::new_v4().to_string();
    }
    validate_id(&write.id)?;
    let skill = checked_path(Path::new(&write.skill_root))?;
    let target = checked_path(Path::new(&write.target_path))?;
    if target == skill || !target.starts_with(&skill) {
        return Err("Target outside skill root".into());
    }
    if write.session_id.is_empty()
        || write.skill_name.is_empty()
        || write.original.as_ref().is_some_and(|v| v.len() > MAX_BYTES)
        || write.proposed.as_ref().is_some_and(|v| v.len() > MAX_BYTES)
    {
        return Err("Invalid or oversized proposal".into());
    }
    write.skill_root = skill.to_string_lossy().into_owned();
    write.target_path = target.to_string_lossy().into_owned();
    let path = record_path(&root, &write.id)?;
    if path.try_exists().map_err(|e| e.to_string())? {
        return Err("Pending ID already exists".into());
    }
    // The public JSON record is never authority for a new destination. Keep a
    // separate immutable staging snapshot and require byte-for-byte agreement.
    // This is not a security boundary against an OS user who can rewrite both.
    let authority = root.join(".authority");
    checked_path(&authority)?;
    std::fs::create_dir_all(&authority).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&write).map_err(|e| e.to_string())?;
    let seal = record_path(&authority, &write.id)?;
    if seal.try_exists().map_err(|e| e.to_string())? {
        return Err("Pending ID already exists".into());
    }
    atomic_write(&seal, &bytes)?;
    atomic_write(&path, &bytes)?;
    Ok(write.id)
}

pub async fn list_pending(root: &Path, session_id: &str) -> Result<Vec<Value>, String> {
    let _guard = WRITER.lock().await;
    let root = checked_path(root)?;
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut items = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|v| v.to_str()) else {
            continue;
        };
        if validate_id(id).is_err() {
            continue;
        }
        // Corrupt or unsafe individual records do not hide unrelated proposals.
        if let Ok(write) = load(&root, id, session_id) {
            items.push(view(&write));
        }
    }
    items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Ok(items)
}

pub async fn pending_diff(root: &Path, id: &str, session_id: &str) -> Result<Value, String> {
    let _guard = WRITER.lock().await;
    Ok(view(&load(&checked_path(root)?, id, session_id)?))
}

fn view(write: &PendingWrite) -> Value {
    let diff = similar::TextDiff::from_lines(
        write.original.as_deref().unwrap_or(""),
        write.proposed.as_deref().unwrap_or(""),
    )
    .unified_diff()
    .header("before", "after")
    .to_string();
    json!({"id":write.id,"name":write.skill_name,"skill_name":write.skill_name,"file_path":write.target_path,"run_id":write.run_id,"session_id":write.session_id,"before":write.original,"after":write.proposed,"diff":diff})
}

fn validate_id(id: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id) {
        Ok(())
    } else {
        Err(MISSING.into())
    }
}

fn record_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    validate_id(id)?;
    checked_path(&root.join(format!("{id}.json")))
}

fn load(root: &Path, id: &str, session: &str) -> Result<PendingWrite, String> {
    let path = record_path(root, id).map_err(|_| MISSING.to_string())?;
    let bytes = bounded_read(&path, MAX_BYTES * 16).map_err(|_| MISSING.to_string())?;
    let write: PendingWrite = serde_json::from_slice(&bytes).map_err(|_| MISSING.to_string())?;
    if write.id != id || write.session_id != session {
        return Err(MISSING.into());
    }
    Ok(write)
}

fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    checked_path(path)?;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err("Not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("File too large".into());
    }
    Ok(bytes)
}

fn checked_path(path: &Path) -> Result<PathBuf, String> {
    use std::path::Component;
    if path.as_os_str().is_empty() || path.to_string_lossy().chars().any(char::is_control) {
        return Err("Invalid path".into());
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err("Path traversal forbidden".into());
        }
        if let Component::Normal(name) = component {
            let name = name.to_string_lossy();
            let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
            let reserved = [
                "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
                "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
                "lpt9",
            ];
            if name.contains(':')
                || name.contains('\\')
                || name.ends_with('.')
                || name.ends_with(' ')
                || reserved.contains(&stem.as_str())
            {
                return Err("Unsafe path component".into());
            }
        }
        current.push(component);
        // A canonical Windows profile uses a verbatim drive prefix. The prefix
        // alone is not a filesystem path; inspect it once the root is appended.
        #[cfg(feature = "shared-profile")]
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err("Symlink forbidden".into());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err("Reparse point forbidden".into());
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_write_checked(path, bytes, || Ok(()))
}

fn atomic_write_checked(
    path: &Path,
    bytes: &[u8],
    before_commit: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    use std::io::Write;
    checked_path(path)?;
    let parent = path.parent().ok_or("Missing parent")?;
    let temporary = parent.join(format!(".pending-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        checked_path(path)?;
        before_commit()?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn sync_dir(_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    std::fs::File::open(_path)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn approve_pending(
    root: &Path,
    id: &str,
    session_id: &str,
) -> Result<super::types::SelfImprovementChange, String> {
    use super::types::{SelfImprovementAction, SelfImprovementChange};
    let _mutation = super::skills::SKILL_MUTATION_LOCK.lock().await;
    let _guard = WRITER.lock().await;
    let root = checked_path(root)?;
    let write = load(&root, id, session_id)?;
    let sealed = bounded_read(&record_path(&root.join(".authority"), id)?, MAX_BYTES * 16)?;
    if serde_json::to_vec(&write).map_err(|e| e.to_string())? != sealed {
        return Err("Pending staging snapshot mismatch".into());
    }
    let skill = checked_path(Path::new(&write.skill_root))?;
    ensure_unlocked(&skill)?;
    let target = checked_path(Path::new(&write.target_path))?;
    if target == skill || !target.starts_with(&skill) {
        return Err("Target outside skill root".into());
    }
    if write.proposed.as_ref().is_some_and(|v| v.len() > MAX_BYTES) {
        return Err("Proposal too large".into());
    }
    let action = if write.original == write.proposed {
        SelfImprovementAction::NoAction
    } else if write.original.is_none() {
        SelfImprovementAction::SkillCreated
    } else {
        SelfImprovementAction::SkillUpdated
    };
    checked_commit(
        &target,
        write.proposed.as_deref(),
        write.original.as_deref(),
        &skill,
        || {},
    )?;
    remove_record(&root, id)?;
    Ok(SelfImprovementChange {
        action,
        skill_name: write.skill_name,
        file_path: Some(write.target_path),
        skill_path: Some(write.skill_root),
        pending_id: Some(id.into()),
        summary: Some("Approved review skill change".into()),
    })
}

// Keep the last lock/CAS check after staging and immediately before rename/removal.
// All in-process skill commits take the common mutation lock before WRITER.
// External OS edits do not participate, so late lock/CAS checks remain necessary.
// The per-call hook makes staging races deterministic in tests without globals.
fn checked_commit(
    target: &Path,
    proposed: Option<&str>,
    original: Option<&str>,
    skill: &Path,
    before_commit: impl FnOnce(),
) -> Result<(), String> {
    ensure_unlocked(skill)?;
    ensure_current(target, original)?;
    if proposed == original {
        before_commit();
        ensure_unlocked(skill)?;
        return ensure_current(target, original);
    }
    if let Some(proposed) = proposed {
        let parent = target.parent().ok_or("Missing target parent")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        checked_path(target)?;
        atomic_write_checked(target, proposed.as_bytes(), || {
            before_commit();
            ensure_unlocked(skill)?;
            ensure_current(target, original)
        })
    } else {
        checked_path(target)?;
        before_commit();
        ensure_unlocked(skill)?;
        ensure_current(target, original)?;
        std::fs::remove_file(target).map_err(|e| e.to_string())?;
        sync_dir(target.parent().ok_or("Missing target parent")?)
    }
}

fn ensure_current(target: &Path, original: Option<&str>) -> Result<(), String> {
    let current = match bounded_read(target, MAX_BYTES) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            if std::fs::symlink_metadata(target)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                None
            } else {
                return Err(e);
            }
        }
    };
    if current.as_deref() != original.map(str::as_bytes) {
        return Err("Skill changed since staging (CAS conflict)".into());
    }
    Ok(())
}

pub async fn reject_pending(root: &Path, id: &str, session_id: &str) -> Result<(), String> {
    let _guard = WRITER.lock().await;
    let root = checked_path(root)?;
    load(&root, id, session_id)?;
    remove_record(&root, id)
}

fn remove_record(root: &Path, id: &str) -> Result<(), String> {
    std::fs::remove_file(record_path(root, id)?).map_err(|e| e.to_string())?;
    sync_dir(root)?;
    let authority = root.join(".authority");
    match std::fs::remove_file(record_path(&authority, id)?) {
        Ok(()) => sync_dir(&authority),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn ensure_unlocked(skill: &Path) -> Result<(), String> {
    checked_path(skill)?;
    match std::fs::symlink_metadata(skill.join(".locked")) {
        Ok(_) => return Err("Skill is locked by .locked marker".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let main = checked_path(&skill.join("SKILL.md"))?;
    let bytes = match bounded_read(&main, MAX_BYTES) {
        Ok(bytes) => bytes,
        Err(e) => {
            if std::fs::symlink_metadata(&main)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                return Ok(());
            }
            return Err(e);
        }
    };
    let text = String::from_utf8(bytes)
        .map_err(|e| e.to_string())?
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n");
    let yaml = text
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---").map(|p| p.0))
        .ok_or("Invalid skill frontmatter")?;
    let metadata: serde_yaml::Value = serde_yaml::from_str(yaml).map_err(|e| e.to_string())?;
    if metadata["metadata"]["aether"]["locked"].as_bool() == Some(true) {
        return Err("Skill metadata.aether.locked is true".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn commit_rejects_intervening_locks_after_staging() {
        for metadata in [false, true] {
            for (original, proposed) in [
                (Some("before\r\n"), Some("after\r\n")),
                (None, Some("created")),
                (Some("before\r\n"), None),
                (Some("before\r\n"), Some("before\r\n")),
            ] {
                let (root, mut write) = fixture();
                let skill = PathBuf::from(&write.skill_root);
                if original.is_none() {
                    write.target_path = skill.join("new.md").to_str().unwrap().into();
                }
                write.original = original.map(str::to_owned);
                write.proposed = proposed.map(str::to_owned);
                let target = PathBuf::from(&write.target_path);
                let id = stage(&root, write).await.unwrap();
                let result = checked_commit(&target, proposed, original, &skill, || {
                    if metadata {
                        std::fs::write(
                            skill.join("SKILL.md"),
                            "---\nmetadata:\n  aether:\n    locked: true\n---\nbody",
                        )
                        .unwrap();
                    } else {
                        std::fs::write(skill.join(".locked"), "").unwrap();
                    }
                });
                assert!(result.is_err(), "intervening lock must prevent commit");
                assert_eq!(std::fs::read_to_string(&target).ok().as_deref(), original);
                assert!(ensure_unlocked(&skill).is_err(), "lock must remain intact");
                assert!(
                    pending_diff(&root, &id, "session").await.is_ok(),
                    "failed commit must retain proposal"
                );
                assert!(
                    !std::fs::read_dir(&skill).unwrap().any(|entry| {
                        entry
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".pending-")
                    }),
                    "failed commit must clean up its temporary file"
                );
            }
        }
    }

    #[test]
    fn commit_rejects_intervening_bytes_after_staging() {
        for (original, proposed) in [
            (Some("before\r\n"), Some("after\r\n")),
            (None, Some("created")),
            (Some("before\r\n"), None),
            (Some("before\r\n"), Some("before\r\n")),
        ] {
            let (_, write) = fixture();
            let skill = Path::new(&write.skill_root);
            let target = if original.is_none() {
                skill.join("new.md")
            } else {
                PathBuf::from(&write.target_path)
            };
            let result = checked_commit(&target, proposed, original, skill, || {
                if proposed.is_some() && original != proposed {
                    assert!(
                        std::fs::read_dir(skill).unwrap().any(|entry| {
                            entry
                                .unwrap()
                                .file_name()
                                .to_string_lossy()
                                .starts_with(".pending-")
                        }),
                        "hook must run after the temporary write has been staged"
                    );
                }
                std::fs::write(&target, "intervening").unwrap();
            });
            assert!(
                result.is_err(),
                "intervening bytes must cause a CAS conflict"
            );
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "intervening");
            assert!(
                !std::fs::read_dir(skill).unwrap().any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".pending-")
                }),
                "failed commit must clean up its temporary file"
            );
        }
    }

    #[tokio::test]
    async fn creation_removal_and_concurrent_approval_preserve_cas() {
        let (root, mut write) = fixture();
        let target = PathBuf::from(&write.skill_root).join("references/new.md");
        write.target_path = target.to_str().unwrap().into();
        write.original = None;
        write.proposed = Some("created".into());
        let id = stage(&root, write.clone()).await.unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "unrelated").unwrap();
        assert!(approve_pending(&root, &id, "session").await.is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "unrelated");
        let (_, mut second) = fixture();
        second.target_path = PathBuf::from(&second.skill_root)
            .join("new.md")
            .to_str()
            .unwrap()
            .into();
        second.original = None;
        second.proposed = Some("created".into());
        let id = stage(&root, second.clone()).await.unwrap();
        let (a, b) = tokio::join!(
            approve_pending(&root, &id, "session"),
            approve_pending(&root, &id, "session")
        );
        assert_ne!(a.is_ok(), b.is_ok());
        assert_eq!(
            a.or(b).unwrap().action,
            super::super::types::SelfImprovementAction::SkillCreated
        );
        second.original = Some("created".into());
        second.proposed = None;
        let id = stage(&root, second.clone()).await.unwrap();
        approve_pending(&root, &id, "session").await.unwrap();
        assert!(!Path::new(&second.target_path).exists());
    }

    #[tokio::test]
    async fn unsafe_paths_and_duplicate_ids_are_rejected() {
        let (root, write) = fixture();
        let id = stage(&root, write.clone()).await.unwrap();
        let mut duplicate = write.clone();
        duplicate.id = id;
        assert!(stage(&root, duplicate).await.is_err());
        for leaf in [
            "../escape",
            "nested/../../escape",
            "bad:stream",
            "NUL",
            "con.txt",
            "trailing.",
        ] {
            let mut invalid = write.clone();
            invalid.target_path = Path::new(&write.skill_root)
                .join(leaf)
                .to_str()
                .unwrap()
                .into();
            assert!(stage(&root, invalid).await.is_err(), "unsafe leaf: {leaf}");
        }
        let mut invalid = write.clone();
        invalid.id = "../../escape".into();
        assert!(stage(&root, invalid).await.is_err());
        let mut invalid = write.clone();
        invalid.target_path = root
            .parent()
            .unwrap()
            .join("escape")
            .to_str()
            .unwrap()
            .into();
        assert!(stage(&root, invalid).await.is_err());
        assert!(stage(&root.join("../escape"), write).await.is_err());
    }

    #[tokio::test]
    async fn approval_rejects_replaced_directory_links() {
        let (root, mut write) = fixture();
        let skill = PathBuf::from(&write.skill_root);
        let nested = skill.join("nested");
        let outside = root.parent().unwrap().join("outside");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(nested.join("notes.md"), "before\r\n").unwrap();
        std::fs::write(outside.join("notes.md"), "before\r\n").unwrap();
        write.target_path = nested.join("notes.md").to_str().unwrap().into();
        let id = stage(&root, write.clone()).await.unwrap();
        std::fs::rename(&nested, skill.join("saved")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &nested).unwrap();
        #[cfg(windows)]
        {
            let command = format!(
                "mklink /J \"{}\" \"{}\"",
                nested.display(),
                outside.display()
            );
            use std::os::windows::process::CommandExt;
            let output = std::process::Command::new("cmd")
                .args(["/D", "/C"])
                .raw_arg(&command)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(approve_pending(&root, &id, "session").await.is_err());
        assert!(stage(&root, write).await.is_err());
        assert_eq!(
            std::fs::read_to_string(outside.join("notes.md")).unwrap(),
            "before\r\n"
        );
        reject_pending(&root, &id, "session").await.unwrap();
    }

    #[tokio::test]
    async fn approval_rechecks_locks_and_staging_authority() {
        for metadata in [false, true] {
            let (root, write) = fixture();
            let skill = PathBuf::from(&write.skill_root);
            let target = PathBuf::from(&write.target_path);
            let id = stage(&root, write).await.unwrap();
            if metadata {
                std::fs::write(skill.join("SKILL.md"), "---\r\nname: manual\r\nmetadata:\r\n  aether:\r\n    locked: true\r\n---\r\nbody").unwrap();
            } else {
                std::fs::write(skill.join(".locked"), "").unwrap();
            }
            assert!(
                approve_pending(&root, &id, "session").await.is_err(),
                "locked skill must not change"
            );
            assert_eq!(std::fs::read_to_string(target).unwrap(), "before\r\n");
            reject_pending(&root, &id, "session").await.unwrap();
        }
        let (root, write) = fixture();
        let mut forged = write.clone();
        let id = stage(&root, write).await.unwrap();
        let outside = root.parent().unwrap().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("victim"), "before\r\n").unwrap();
        forged.id = id.clone();
        forged.target_path = outside.join("victim").to_str().unwrap().into();
        forged.skill_root = outside.to_str().unwrap().into();
        std::fs::write(
            root.join(format!("{id}.json")),
            serde_json::to_vec(&forged).unwrap(),
        )
        .unwrap();
        assert!(
            approve_pending(&root, &id, "session").await.is_err(),
            "edited record cannot widen staged authority"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("victim")).unwrap(),
            "before\r\n"
        );
    }

    #[tokio::test]
    async fn approval_is_raw_cas_and_rejection_is_session_scoped() {
        let (root, write) = fixture();
        let target = PathBuf::from(&write.target_path);
        let id = stage(&root, write).await.unwrap();
        assert_eq!(
            reject_pending(&root, &id, "foreign").await.unwrap_err(),
            reject_pending(&root, &uuid::Uuid::new_v4().to_string(), "foreign")
                .await
                .unwrap_err()
        );
        assert!(approve_pending(&root, &id, "foreign").await.is_err());
        std::fs::write(&target, "before\n").unwrap();
        assert!(approve_pending(&root, &id, "session").await.is_err());
        assert_eq!(list_pending(&root, "session").await.unwrap().len(), 1);
        std::fs::write(&target, "before\r\n").unwrap();
        let change = approve_pending(&root, &id, "session").await.unwrap();
        assert_eq!(
            change.action,
            super::super::types::SelfImprovementAction::SkillUpdated
        );
        assert_eq!(change.pending_id.as_deref(), Some(id.as_str()));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "after\r\n");
        assert!(list_pending(&root, "session").await.unwrap().is_empty());
        assert!(approve_pending(&root, &id, "session").await.is_err());
        let (_, mut write) = fixture();
        write.target_path = target.to_str().unwrap().into();
        write.skill_root = target.parent().unwrap().to_str().unwrap().into();
        write.original = Some("after\r\n".into());
        let id = stage(&root, write).await.unwrap();
        reject_pending(&root, &id, "session").await.unwrap();
        assert_eq!(std::fs::read_to_string(target).unwrap(), "after\r\n");
        assert!(list_pending(&root, "session").await.unwrap().is_empty());
    }

    use super::*;
    fn fixture() -> (PathBuf, PendingWrite) {
        let base =
            std::env::temp_dir().join(format!("aether-pending-test-{}", uuid::Uuid::new_v4()));
        let skill = base.join("skills/manual");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("notes.md"), "before\r\n").unwrap();
        let write = PendingWrite {
            id: String::new(),
            session_id: "session".into(),
            run_id: "run".into(),
            skill_name: "manual".into(),
            target_path: skill.join("notes.md").to_str().unwrap().into(),
            skill_root: skill.to_str().unwrap().into(),
            original: Some("before\r\n".into()),
            proposed: Some("after\r\n".into()),
        };
        (base.join("pending"), write)
    }

    #[tokio::test]
    async fn durable_stage_list_diff_are_session_scoped() {
        let (root, write) = fixture();
        let target = write.target_path.clone();
        let id = stage(&root, write).await.unwrap();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "before\r\n");
        let records = list_pending(&root, "session").await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["id"], id);
        assert_eq!(records[0]["name"], "manual");
        assert!(list_pending(&root, "foreign").await.unwrap().is_empty());
        let diff = pending_diff(&root, &id, "session").await.unwrap();
        assert_eq!(diff["before"], "before\r\n");
        assert_eq!(diff["after"], "after\r\n");
        assert!(diff["diff"].as_str().unwrap().contains("-before"));
        assert_eq!(
            pending_diff(&root, &id, "foreign").await.unwrap_err(),
            pending_diff(&root, &uuid::Uuid::new_v4().to_string(), "foreign")
                .await
                .unwrap_err()
        );
        let stored: PendingWrite =
            serde_json::from_slice(&std::fs::read(root.join(format!("{id}.json"))).unwrap())
                .unwrap();
        assert_eq!(stored.id, id);
        #[cfg(feature = "shared-profile")]
        assert_eq!(
            pending_root().unwrap(),
            super::super::storage::config_dir().join("pending/skills")
        );
        #[cfg(not(feature = "shared-profile"))]
        assert_eq!(
            pending_root().unwrap(),
            dirs::home_dir()
                .unwrap()
                .join(".aether-agent/pending/skills")
        );
    }
}
