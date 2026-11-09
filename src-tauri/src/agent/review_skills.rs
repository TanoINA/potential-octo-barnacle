//! Restricted review-time skill tools. A `.locked` marker or
//! `metadata.aether.locked: true` in SKILL.md prevents all mutations.

use super::review_pending::{self, PendingWrite};
use super::types::{SelfImprovementAction, SelfImprovementChange, SkillInfo};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_FILE_BYTES: usize = 256 * 1024;

pub struct ReviewSkillStore {
    skills: Vec<SkillInfo>,
    reads: HashMap<PathBuf, String>,
    changes: Vec<SelfImprovementChange>,
    create_root: PathBuf,
    pending_root: PathBuf,
    write_approval: bool,
    run_id: String,
    session_id: String,
}

impl ReviewSkillStore {
    pub async fn new(
        workspace: &str,
        configured_paths: &[String],
        create_root: PathBuf,
        pending_root: PathBuf,
        write_approval: bool,
        run_id: &str,
        session_id: &str,
    ) -> Result<Self, String> {
        let create_root = checked_path(&create_root)?;
        let mut paths = super::skills::workspace_skill_paths(workspace, configured_paths);
        paths.push(create_root.to_string_lossy().into_owned());
        let skills = super::skills::load_skills(&paths).await;
        Ok(Self {
            skills,
            reads: HashMap::new(),
            changes: Vec::new(),
            create_root,
            pending_root: checked_path(&pending_root)?,
            write_approval,
            run_id: run_id.into(),
            session_id: session_id.into(),
        })
    }

    pub fn changes(&self) -> Vec<SelfImprovementChange> {
        self.changes.clone()
    }

    /// Reload durable proposals so restarting cannot bypass the pending guard.
    /// Staged bytes never authorize a read or enter the discovered catalog.
    async fn stage_change(
        &mut self,
        name: &str,
        path: &Path,
        dir: &Path,
        original: Option<String>,
        proposed: Option<String>,
    ) -> Result<Value, String> {
        let pending = review_pending::list_pending(&self.pending_root, &self.session_id).await?;
        if pending.iter().any(|item| {
            item["file_path"]
                .as_str()
                .is_some_and(|target| Path::new(target) == path)
        }) {
            return Err("Resolve the existing pending proposal for this exact target first".into());
        }
        ensure_unlocked(dir)?;
        check_cas(path, original.as_deref())?;
        let id = review_pending::stage(
            &self.pending_root,
            PendingWrite {
                id: String::new(),
                session_id: self.session_id.clone(),
                run_id: self.run_id.clone(),
                skill_name: name.into(),
                target_path: path.to_string_lossy().into_owned(),
                skill_root: dir.to_string_lossy().into_owned(),
                original,
                proposed,
            },
        )
        .await?;
        self.reads.remove(path);
        self.changes.push(change(
            SelfImprovementAction::SkillStaged,
            name,
            path,
            dir,
            Some(id.clone()),
        ));
        Ok(json!({"status":"staged","pending_id":id,"file_path":path,"skill_path":dir}))
    }
    fn allowed_path(&self, value: &str) -> Result<PathBuf, String> {
        if value.chars().any(char::is_control) {
            return Err("Control characters in path".into());
        }
        let path = checked_path(Path::new(value))?;
        for skill in &self.skills {
            let dir = checked_path(
                Path::new(&skill.path)
                    .parent()
                    .ok_or("Missing skill directory")?,
            )?;
            if path.starts_with(dir) {
                return Ok(path);
            }
        }
        Err("Read/search restricted to discovered skill directories".into())
    }

    pub async fn execute(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "skills_list" => Ok(
                json!({"skills": self.skills.iter().map(|s| json!({"name":s.name,"description":s.description})).collect::<Vec<_>>()}),
            ),
            "skill_view" | "use_skill" => {
                let skill = self
                    .skills
                    .iter()
                    .find(|s| {
                        s.name
                            .eq_ignore_ascii_case(args["name"].as_str().unwrap_or(""))
                    })
                    .ok_or("Unknown skill")?;
                let dir = Path::new(&skill.path)
                    .parent()
                    .ok_or("Skill has no directory")?;
                let path = match args["file_path"].as_str() {
                    Some(p) => relative_target(dir, p)?,
                    None => PathBuf::from(&skill.path),
                };
                let content = raw_read(&path)?;
                self.reads.insert(path.clone(), content.clone());
                Ok(json!({"name":skill.name,"file_path":path,"content":content}))
            }
            "read_file" => {
                let path = self.allowed_path(required(args, "path")?)?;
                let content = raw_read(&path)?;
                self.reads.insert(path.clone(), content.clone());
                Ok(json!({"file_path":path,"content":content}))
            }
            "search_files" => {
                let path = self.allowed_path(required(args, "path")?)?;
                let pattern = required(args, "pattern")?;
                if pattern.len() > 1024 {
                    return Err("Search pattern too large".into());
                }
                let regex = regex::RegexBuilder::new(pattern)
                    .size_limit(1024 * 1024)
                    .build()
                    .map_err(|e| e.to_string())?;
                let mut files = Vec::new();
                collect_files(&path, 0, &mut files)?;
                let mut matches = Vec::new();
                for file in files {
                    let Ok(content) = raw_read(&file) else {
                        continue;
                    };
                    for (line, text) in content.lines().enumerate() {
                        if regex.is_match(text) {
                            matches.push(json!({"file_path":file,"line":line + 1,"text":text.chars().take(500).collect::<String>()}));
                        }
                        if matches.len() >= 100 {
                            break;
                        }
                    }
                    if matches.len() >= 100 {
                        break;
                    }
                }
                Ok(json!({"matches":matches}))
            }
            "skill_manage" => {
                let _mutation = super::skills::SKILL_MUTATION_LOCK.lock().await;
                let _writer = WRITER.lock().await;
                let skill_name = required(args, "name")?;
                if args["action"] == "create" {
                    validate_slug(skill_name)?;
                    if self
                        .skills
                        .iter()
                        .any(|s| s.name.eq_ignore_ascii_case(skill_name))
                    {
                        return Err("Skill already exists".into());
                    }
                    let description = required(args, "description")?;
                    if description.len() > 1024 || description.chars().any(char::is_control) {
                        return Err("Invalid skill description".into());
                    }
                    let content = required(args, "content")?;
                    if content.trim().is_empty() {
                        return Err("Empty skill instructions".into());
                    }
                    let proposed = if content.trim_start_matches('\u{feff}').starts_with("---") {
                        let yaml = frontmatter(content)?;
                        if yaml["name"].as_str() != Some(skill_name)
                            || yaml["description"].as_str() != Some(description)
                        {
                            return Err("Create metadata must match name and description".into());
                        }
                        content.to_string()
                    } else {
                        let yaml = serde_yaml::to_string(
                            &json!({"name":skill_name,"description":description}),
                        )
                        .map_err(|e| e.to_string())?;
                        format!("---\n{yaml}---\n{content}\n")
                    };
                    validate_content(&proposed)?;
                    let dir = relative_target(&self.create_root, skill_name)?;
                    if dir.exists() {
                        return Err("Skill directory already exists".into());
                    }
                    let path = checked_path(&dir.join("SKILL.md"))?;
                    if self.write_approval {
                        return self
                            .stage_change(skill_name, &path, &dir, None, Some(proposed))
                            .await;
                    }
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    atomic_synced_write(&path, proposed.as_bytes())?;
                    self.skills.push(SkillInfo {
                        name: skill_name.into(),
                        description: description.into(),
                        path: path.to_string_lossy().into_owned(),
                        source: self.create_root.to_string_lossy().into_owned(),
                    });
                    self.changes.push(change(
                        SelfImprovementAction::SkillCreated,
                        skill_name,
                        &path,
                        &dir,
                        None,
                    ));
                    return Ok(json!({"status":"created","file_path":path}));
                }
                let skill = self
                    .skills
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(skill_name))
                    .cloned()
                    .ok_or("Unknown skill")?;
                let dir = checked_path(
                    Path::new(&skill.path)
                        .parent()
                        .ok_or("Skill has no directory")?,
                )?;
                ensure_unlocked(&dir)?;
                let path = match args["file_path"].as_str() {
                    Some(p) => relative_target(&dir, p)?,
                    None => checked_path(Path::new(&skill.path))?,
                };
                let action = required(args, "action")?;
                let main = checked_path(Path::new(&skill.path))?;
                if path != main || matches!(action, "write_file" | "remove_file") {
                    validate_support_target(&dir, &path)?;
                }
                if !matches!(action, "patch" | "edit" | "write_file" | "remove_file") {
                    return Err("Unsupported skill mutation".into());
                }
                let exists = match std::fs::symlink_metadata(&path) {
                    Ok(_) => true,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                    Err(e) => return Err(e.to_string()),
                };
                let original = if exists {
                    let read = self
                        .reads
                        .get(&path)
                        .ok_or("Read the exact file before mutating it")?
                        .clone();
                    check_cas(&path, Some(&read))?;
                    Some(read)
                } else {
                    if action != "write_file" {
                        return Err("Mutation target does not exist".into());
                    }
                    None
                };
                if action == "remove_file" {
                    if self.write_approval {
                        return self
                            .stage_change(&skill.name, &path, &dir, original, None)
                            .await;
                    }
                    ensure_unlocked(&dir)?;
                    check_cas(&path, original.as_deref())?;
                    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
                    #[cfg(unix)]
                    std::fs::File::open(path.parent().ok_or("Missing parent")?)
                        .and_then(|f| f.sync_all())
                        .map_err(|e| e.to_string())?;
                } else {
                    let proposed = match action {
                        "patch" => {
                            let original = original
                                .as_deref()
                                .ok_or("Mutation target does not exist")?;
                            let old = required(args, "old_string")?;
                            let new = required(args, "new_string")?;
                            if old.is_empty() || original.matches(old).count() != 1 {
                                return Err("old_string must match exactly once".into());
                            }
                            original.replacen(old, new, 1)
                        }
                        "edit" | "write_file" => required(args, "content")?.to_string(),
                        _ => unreachable!(),
                    };
                    validate_content(&proposed)?;
                    if path == main {
                        validate_skill_update(
                            original.as_deref().ok_or("Missing skill")?,
                            &proposed,
                        )?;
                    }
                    if original.as_deref() == Some(proposed.as_str()) {
                        return Ok(json!({"status":"no_action"}));
                    }
                    if self.write_approval {
                        return self
                            .stage_change(&skill.name, &path, &dir, original, Some(proposed))
                            .await;
                    }
                    let parent = path.parent().ok_or("Missing parent")?;
                    checked_path(parent)?;
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    atomic_synced_write_checked(&path, proposed.as_bytes(), || {
                        ensure_unlocked(&dir)?;
                        check_cas(&path, original.as_deref())
                    })?;
                }
                self.reads.remove(&path);
                self.changes.push(change(
                    SelfImprovementAction::SkillUpdated,
                    &skill.name,
                    &path,
                    &dir,
                    None,
                ));
                Ok(
                    json!({"status":if action == "remove_file" { "removed" } else { "updated" },"file_path":path}),
                )
            }
            _ => Err("Unsupported review skill tool".into()),
        }
    }
}

static WRITER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn validate_slug(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || name.starts_with('-')
        || name.ends_with('-')
    {
        return Err("Skill name must be a lowercase ASCII slug of 1..64 bytes".into());
    }
    let reserved = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if reserved.contains(&name) {
        return Err("Windows reserved device name".into());
    }
    Ok(())
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .ok_or_else(|| format!("Missing string field {key}"))
}

fn validate_content(content: &str) -> Result<(), String> {
    if content.len() > MAX_FILE_BYTES
        || content
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("Oversized content or forbidden control characters".into());
    }
    Ok(())
}

fn frontmatter(content: &str) -> Result<serde_yaml::Value, String> {
    let normalized = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let yaml = normalized
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---").map(|p| p.0))
        .ok_or("Missing YAML frontmatter")?;
    if yaml.len() > 64 * 1024 {
        return Err("Frontmatter exceeds 64 KiB".into());
    }
    serde_yaml::from_str(yaml).map_err(|e| e.to_string())
}

fn validate_skill_update(old: &str, new: &str) -> Result<(), String> {
    let previous = frontmatter(old)?;
    let proposed = frontmatter(new)?;
    // Existing metadata is immutable here: body patches remain byte-exact and
    // full-file edits cannot silently drop user-authored YAML extensions.
    if previous != proposed || raw_frontmatter(old)? != raw_frontmatter(new)? {
        return Err("Existing YAML metadata must be preserved byte-for-byte".into());
    }
    Ok(())
}

fn ensure_unlocked(dir: &Path) -> Result<(), String> {
    checked_path(dir)?;
    if std::fs::symlink_metadata(dir.join(".locked")).is_ok() {
        return Err("Skill is locked by .locked marker".into());
    }
    let main = dir.join("SKILL.md");
    if main.exists() {
        let yaml = frontmatter(&raw_read(&main)?)?;
        if yaml["metadata"]["aether"]["locked"].as_bool() == Some(true) {
            return Err("Skill metadata.aether.locked is true".into());
        }
    }
    Ok(())
}

fn change(
    action: SelfImprovementAction,
    name: &str,
    file: &Path,
    dir: &Path,
    pending: Option<String>,
) -> SelfImprovementChange {
    SelfImprovementChange {
        action,
        skill_name: name.into(),
        file_path: Some(file.to_string_lossy().into_owned()),
        skill_path: Some(dir.to_string_lossy().into_owned()),
        pending_id: pending,
        summary: Some("Review skill change".into()),
    }
}

fn atomic_synced_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_synced_write_checked(path, bytes, || check_cas(path, None))
}

fn atomic_synced_write_checked(
    path: &Path,
    bytes: &[u8],
    before_rename: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    use std::io::Write;
    checked_path(path)?;
    let parent = path.parent().ok_or("Missing parent")?;
    let temporary = parent.join(format!(".review-{}.tmp", uuid::Uuid::new_v4()));
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
        // Recheck the exact target and lock after staging and syncing, not merely
        // before opening the temporary file. External writers need not use WRITER.
        before_rename()?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn checked_path(path: &Path) -> Result<PathBuf, String> {
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
            return Err("Path traversal is forbidden".into());
        }
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(m) => {
                if m.file_type().is_symlink() {
                    return Err("Symlinks are forbidden".into());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if m.file_attributes() & 0x400 != 0 {
                        return Err("Reparse points are forbidden".into());
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}

fn relative_target(root: &Path, value: &str) -> Result<PathBuf, String> {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_control() || c == ':' || c == '\\')
        || Path::new(value).is_absolute()
        || value
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with('.') || p.ends_with(' '))
    {
        return Err("Invalid relative skill file path".into());
    }
    for segment in value.split('/') {
        validate_path_segment(segment)?;
    }
    checked_path(&root.join(value))
}

fn raw_frontmatter(content: &str) -> Result<&str, String> {
    let mut offset = 0;
    let mut lines = content.split_inclusive('\n');
    let first = lines.next().ok_or("Missing YAML frontmatter")?;
    if first
        .trim_start_matches('\u{feff}')
        .trim_end_matches(['\r', '\n'])
        != "---"
    {
        return Err("Missing YAML frontmatter".into());
    }
    offset += first.len();
    for line in lines {
        offset += line.len();
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Ok(&content[..offset]);
        }
        if offset > 64 * 1024 {
            return Err("Frontmatter exceeds 64 KiB".into());
        }
    }
    Err("Missing YAML frontmatter terminator".into())
}

fn validate_support_target(dir: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(dir)
        .map_err(|_| "Support file escapes skill directory")?;
    let mut parts = relative.components();
    let first = parts
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .ok_or("Missing support directory")?;
    if !matches!(first, "references" | "templates" | "scripts") || parts.next().is_none() {
        return Err("Support files must be inside references/, templates/, or scripts/".into());
    }
    for part in relative.components() {
        validate_path_segment(part.as_os_str().to_str().ok_or("Non-UTF-8 support path")?)?;
    }
    checked_path(path)?;
    Ok(())
}

fn validate_path_segment(segment: &str) -> Result<(), String> {
    if segment.is_empty()
        || segment.ends_with(['.', ' '])
        || segment
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '\\' | '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return Err("Unsafe skill path component".into());
    }
    let base = segment.split('.').next().unwrap_or("").to_ascii_lowercase();
    if matches!(base.as_str(), "con" | "prn" | "aux" | "nul")
        || ((base.starts_with("com") || base.starts_with("lpt"))
            && base.len() == 4
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("Windows reserved device name".into());
    }
    Ok(())
}

fn check_cas(path: &Path, expected: Option<&str>) -> Result<(), String> {
    checked_path(path)?;
    match expected {
        Some(content) if raw_read(path)? == content => Ok(()),
        Some(_) => Err("Skill changed since fresh read (CAS conflict)".into()),
        None => match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Ok(_) => Err("Skill target appeared before commit (CAS conflict)".into()),
            Err(e) => Err(e.to_string()),
        },
    }
}

fn collect_files(path: &Path, depth: usize, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > 16 || files.len() >= 1000 {
        return Err("Skill search traversal limit exceeded".into());
    }
    let path = checked_path(path)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if metadata.is_file() {
        files.push(path);
    } else if metadata.is_dir() {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
            if entries.len() >= 1000 {
                return Err("Skill search directory limit exceeded".into());
            }
            entries.push(entry.map_err(|e| e.to_string())?.path());
        }
        entries.sort();
        for entry in entries {
            collect_files(&entry, depth + 1, files)?;
        }
    } else {
        return Err("Search target is not a regular file or directory".into());
    }
    Ok(())
}

fn raw_read(path: &Path) -> Result<String, String> {
    let path = checked_path(path)?;
    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES as u64 {
        return Err("Skill file must be regular and at most 256 KiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Skill file exceeds 256 KiB".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("aether-review-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".aether/skills/manual")).unwrap();
        std::fs::write(root.join(".aether/skills/manual/SKILL.md"), "---\r\nname: manual\r\ndescription: Manual recipe\r\nmetadata:\r\n  custom: preserved\r\n---\r\nUse `terminal` safely.\r\n").unwrap();
        root
    }

    #[tokio::test]
    async fn review_skills_concurrent_stores_stage_only_one_proposal_per_session_target() {
        let root = fixture();
        let pending = root.join("pending");
        let create_root = root.join("created");
        let mut stores = Vec::new();
        for run in ["first", "second"] {
            stores.push(
                ReviewSkillStore::new(
                    root.to_str().unwrap(),
                    &[],
                    create_root.clone(),
                    pending.clone(),
                    true,
                    run,
                    "session",
                )
                .await
                .unwrap(),
            );
        }
        let mut second = stores.pop().unwrap();
        let mut first = stores.pop().unwrap();
        let create = json!({"action":"create","name":"shared-recipe","description":"Recipe","content":"Steps"});
        let (a, b) = tokio::join!(
            first.execute("skill_manage", &create),
            second.execute("skill_manage", &create),
        );
        assert_ne!(a.is_ok(), b.is_ok());
        let error = if let Err(error) = a {
            error
        } else {
            b.unwrap_err()
        };
        assert!(error.contains("existing pending proposal"));
        assert_eq!(
            review_pending::list_pending(&pending, "session")
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(!create_root.exists());
    }
    #[tokio::test]
    async fn review_skills_edit_content_requires_fresh_read_and_preserves_raw_frontmatter() {
        for approval in [false, true] {
            let root = fixture();
            let main = root.join(".aether/skills/manual/SKILL.md");
            let pending = root.join("pending");
            let original = raw_read(&main).unwrap();
            let proposed = original.replace("`terminal`", "`run_command`");
            let mut store = ReviewSkillStore::new(
                root.to_str().unwrap(),
                &[],
                root.join("created"),
                pending.clone(),
                approval,
                "run",
                "session",
            )
            .await
            .unwrap();
            let edit = json!({"action":"edit","name":"manual","content":proposed});
            assert!(store
                .execute("skill_manage", &edit)
                .await
                .unwrap_err()
                .contains("Read the exact file"));
            store
                .execute("skill_view", &json!({"name":"manual"}))
                .await
                .unwrap();
            let invalid =
                json!({"action":"edit","name":"manual","content":proposed.replace("\r\n", "\n")});
            assert!(store
                .execute("skill_manage", &invalid)
                .await
                .unwrap_err()
                .contains("preserved byte-for-byte"));
            std::fs::write(&main, original.replace("safely", "externally")).unwrap();
            assert!(store
                .execute("skill_manage", &edit)
                .await
                .unwrap_err()
                .contains("CAS conflict"));
            std::fs::write(&main, &original).unwrap();
            let result = store.execute("skill_manage", &edit).await.unwrap();
            if approval {
                assert_eq!(result["status"], "staged");
                assert_eq!(raw_read(&main).unwrap(), original);
                review_pending::approve_pending(
                    &pending,
                    result["pending_id"].as_str().unwrap(),
                    "session",
                )
                .await
                .unwrap();
            } else {
                assert_eq!(result["status"], "updated");
            }
            assert_eq!(raw_read(&main).unwrap(), proposed);
            assert_eq!(
                raw_frontmatter(&proposed).unwrap(),
                raw_frontmatter(&original).unwrap()
            );
            assert!(store
                .execute("skill_manage", &edit)
                .await
                .unwrap_err()
                .contains("Read the exact file"));
            store
                .execute("skill_view", &json!({"name":"manual"}))
                .await
                .unwrap();
            assert_eq!(
                store.execute("skill_manage", &edit).await.unwrap()["status"],
                "no_action"
            );
        }
    }
    #[tokio::test]
    async fn review_skills_approval_stages_durable_changes_without_authorizing_unread_files() {
        use super::super::review_pending::{approve_pending, list_pending};
        let root = fixture();
        let pending = root.join("pending");
        let create_root = root.join("created");
        let main = root.join(".aether/skills/manual/SKILL.md");
        let original = raw_read(&main).unwrap();
        let mut store = ReviewSkillStore::new(
            root.to_str().unwrap(),
            &[],
            create_root.clone(),
            pending.clone(),
            true,
            "run",
            "session",
        )
        .await
        .unwrap();
        let create = json!({"action":"create","name":"new-recipe","description":"Recipe","content":"Follow tested steps."});
        let created = store.execute("skill_manage", &create).await.unwrap();
        assert_eq!(created["status"], "staged");
        assert!(
            !create_root.exists(),
            "staging must not create skill directories"
        );
        assert!(store.execute("skill_manage", &create).await.is_err());
        assert!(store
            .execute("skill_view", &json!({"name":"new-recipe"}))
            .await
            .is_err());
        let catalog = store.execute("skills_list", &json!({})).await.unwrap();
        assert!(!catalog["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "new-recipe"));
        let patch = json!({"action":"patch","name":"manual","old_string":"`terminal`","new_string":"`run_command`"});
        assert!(store
            .execute("skill_manage", &patch)
            .await
            .unwrap_err()
            .contains("Read the exact file"));
        store
            .execute("skill_view", &json!({"name":"manual"}))
            .await
            .unwrap();
        let patched = store.execute("skill_manage", &patch).await.unwrap();
        assert_eq!(patched["status"], "staged");
        assert_eq!(raw_read(&main).unwrap(), original);
        store
            .execute("skill_view", &json!({"name":"manual"}))
            .await
            .unwrap();
        assert!(
            store.execute("skill_manage", &patch).await.is_err(),
            "a fresh read must not authorize a second pending write"
        );
        let changes = store.changes();
        assert_eq!(changes.len(), 2);
        assert!(changes
            .iter()
            .all(|c| c.action == SelfImprovementAction::SkillStaged
                && c.pending_id.is_some()
                && c.file_path.is_some()
                && c.skill_path.is_some()));
        drop(store);
        let mut reloaded = ReviewSkillStore::new(
            root.to_str().unwrap(),
            &[],
            create_root.clone(),
            pending.clone(),
            true,
            "next-run",
            "session",
        )
        .await
        .unwrap();
        assert!(reloaded.execute("skill_manage", &create).await.is_err());
        reloaded
            .execute("skill_view", &json!({"name":"manual"}))
            .await
            .unwrap();
        assert!(reloaded.execute("skill_manage", &patch).await.is_err());
        let records = list_pending(&pending, "session").await.unwrap();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|r| r["run_id"] == "run" && r["session_id"] == "session"));
        assert!(list_pending(&pending, "foreign").await.unwrap().is_empty());
        let create_id = created["pending_id"].as_str().unwrap();
        let patch_id = patched["pending_id"].as_str().unwrap();
        approve_pending(&pending, create_id, "session")
            .await
            .unwrap();
        assert!(create_root.join("new-recipe/SKILL.md").is_file());
        std::fs::write(&main, original.replace("\r\n", "\n")).unwrap();
        assert!(approve_pending(&pending, patch_id, "session")
            .await
            .unwrap_err()
            .contains("CAS conflict"));
        std::fs::write(&main, &original).unwrap();
        approve_pending(&pending, patch_id, "session")
            .await
            .unwrap();
        assert_eq!(
            raw_read(&main).unwrap(),
            original.replace("`terminal`", "`run_command`")
        );
        // Resolution releases the target, but never grants a full-file read.
        assert!(reloaded.execute("skill_manage", &patch).await.is_err());
        reloaded
            .execute("skill_view", &json!({"name":"manual"}))
            .await
            .unwrap();
        let edited = reloaded.execute("skill_manage", &json!({"action":"edit","name":"manual","content":raw_read(&main).unwrap().replace("safely", "carefully")})).await.unwrap();
        assert_eq!(edited["status"], "staged");
        assert!(!raw_read(&main).unwrap().contains("carefully"));
        approve_pending(&pending, edited["pending_id"].as_str().unwrap(), "session")
            .await
            .unwrap();
        let support = root.join(".aether/skills/manual/references/notes.md");
        let write = json!({"action":"write_file","name":"manual","file_path":"references/notes.md","content":"alpha"});
        let staged = reloaded.execute("skill_manage", &write).await.unwrap();
        assert_eq!(staged["status"], "staged");
        assert!(!support.parent().unwrap().exists());
        approve_pending(&pending, staged["pending_id"].as_str().unwrap(), "session")
            .await
            .unwrap();
        let overwrite = json!({"action":"write_file","name":"manual","file_path":"references/notes.md","content":"beta"});
        assert!(reloaded.execute("skill_manage", &overwrite).await.is_err());
        reloaded
            .execute("read_file", &json!({"path":support}))
            .await
            .unwrap();
        let staged = reloaded.execute("skill_manage", &overwrite).await.unwrap();
        assert_eq!(raw_read(&support).unwrap(), "alpha");
        approve_pending(&pending, staged["pending_id"].as_str().unwrap(), "session")
            .await
            .unwrap();
        let remove =
            json!({"action":"remove_file","name":"manual","file_path":"references/notes.md"});
        assert!(reloaded
            .execute("skill_manage", &remove)
            .await
            .unwrap_err()
            .contains("Read the exact file"));
        reloaded
            .execute("search_files", &json!({"path":support,"pattern":"beta"}))
            .await
            .unwrap();
        assert!(reloaded.execute("skill_manage", &remove).await.is_err());
        reloaded
            .execute("read_file", &json!({"path":support}))
            .await
            .unwrap();
        let staged = reloaded.execute("skill_manage", &remove).await.unwrap();
        assert_eq!(staged["status"], "staged");
        assert_eq!(raw_read(&support).unwrap(), "beta");
        approve_pending(&pending, staged["pending_id"].as_str().unwrap(), "session")
            .await
            .unwrap();
        assert!(!support.exists());
        assert!(list_pending(&pending, "session").await.unwrap().is_empty());
    }
    #[tokio::test]
    async fn review_skills_support_files_restricted_reads_search_and_removal() {
        let root = fixture();
        let dir = root.join(".aether/skills/manual");
        let mut store = ReviewSkillStore::new(
            root.to_str().unwrap(),
            &[],
            root.join("created"),
            root.join("pending"),
            false,
            "run",
            "session",
        )
        .await
        .unwrap();
        store.execute("skill_manage", &json!({"action":"write_file","name":"manual","file_path":"references/notes.md","content":"alpha beta"})).await.unwrap();
        let write = json!({"action":"write_file","name":"manual","file_path":"references/notes.md","content":"gamma"});
        assert!(store.execute("skill_manage", &write).await.is_err());
        let target = dir.join("references/notes.md");
        let read = store
            .execute("read_file", &json!({"path":target}))
            .await
            .unwrap();
        assert_eq!(read["content"], "alpha beta");
        store.execute("skill_manage", &write).await.unwrap();
        let remove =
            json!({"action":"remove_file","name":"manual","file_path":"references/notes.md"});
        assert!(store.execute("skill_manage", &remove).await.is_err());
        let found = store
            .execute("search_files", &json!({"path":dir,"pattern":"gamma"}))
            .await
            .unwrap();
        assert_eq!(found["matches"].as_array().unwrap().len(), 1);
        // Search results are not an exact full-file read authorization.
        assert!(store.execute("skill_manage", &remove).await.is_err());
        store
            .execute("read_file", &json!({"path":target}))
            .await
            .unwrap();
        store.execute("skill_manage", &remove).await.unwrap();
        assert!(!target.exists());
        for file in [
            "../escape",
            "other.txt",
            "scripts/../escape",
            "scripts/con",
            "scripts/a:stream",
        ] {
            assert!(store
                .execute(
                    "skill_manage",
                    &json!({"action":"write_file","name":"manual","file_path":file,"content":"bad"})
                )
                .await
                .is_err());
        }
        assert!(store
            .execute("skill_manage", &json!({"action":"delete","name":"manual"}))
            .await
            .is_err());
        assert!(store
            .execute(
                "skill_manage",
                &json!({"action":"remove_file","name":"manual","file_path":"SKILL.md"})
            )
            .await
            .is_err());
        let outside = root.join("secret");
        std::fs::write(&outside, "secret").unwrap();
        assert!(store
            .execute("read_file", &json!({"path":outside}))
            .await
            .is_err());
        assert!(store
            .execute("search_files", &json!({"path":root,"pattern":"secret"}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn review_skills_create_writes_discoverable_skill_and_tracks_multiple_changes() {
        let root = fixture();
        let mut store = ReviewSkillStore::new(
            root.to_str().unwrap(),
            &[],
            root.join("created"),
            root.join("pending"),
            false,
            "run",
            "session",
        )
        .await
        .unwrap();
        let create = json!({"action":"create","name":"new-recipe","description":"A: safe recipe","content":"# Recipe\nFollow these steps."});
        store.execute("skill_manage", &create).await.unwrap();
        assert!(root.join("created/new-recipe/SKILL.md").is_file());
        assert!(store.execute("skill_manage", &create).await.is_err());
        store
            .execute("skill_view", &json!({"name":"new-recipe"}))
            .await
            .unwrap();
        store.execute("skill_manage", &json!({"action":"patch","name":"new-recipe","old_string":"these steps", "new_string":"the tested steps"})).await.unwrap();
        assert_eq!(store.changes().len(), 2);
        assert_eq!(
            store.changes()[0].action,
            SelfImprovementAction::SkillCreated
        );
        for name in ["../escape", "nul", "a:b", "bad\nname"] {
            assert!(store
                .execute(
                    "skill_manage",
                    &json!({"action":"create","name":name,"description":"D","content":"C"})
                )
                .await
                .is_err());
        }
    }

    #[tokio::test]
    async fn review_skills_patch_requires_fresh_read_and_preserves_raw_metadata() {
        let root = fixture();
        let path = root.join(".aether/skills/manual/SKILL.md");
        let mut store = ReviewSkillStore::new(
            root.to_str().unwrap(),
            &[],
            root.join("created"),
            root.join("pending"),
            false,
            "run",
            "session",
        )
        .await
        .unwrap();
        let patch = json!({"action":"patch","name":"manual","old_string":"`terminal`","new_string":"`run_command`"});
        assert!(store.execute("skill_manage", &patch).await.is_err());
        store
            .execute("skill_view", &json!({"name":"manual"}))
            .await
            .unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        store.execute("skill_manage", &patch).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original.replace("`terminal`", "`run_command`")
        );
        assert_eq!(store.changes().len(), 1);
        assert!(store.execute("skill_manage", &patch).await.is_err());
        store
            .execute("skill_view", &json!({"name":"manual"}))
            .await
            .unwrap();
        std::fs::write(&path, original).unwrap();
        assert!(store.execute("skill_manage", &json!({"action":"patch","name":"manual","old_string":"safely","new_string":"carefully"})).await.is_err());
    }

    #[test]
    fn review_skills_atomic_commit_rechecks_cas_and_cleans_staging() {
        let root = fixture();
        let path = root.join(".aether/skills/manual/SKILL.md");
        let result = atomic_synced_write_checked(&path, b"replacement", || {
            std::fs::write(&path, "external change").unwrap();
            check_cas(&path, Some("original"))
        });
        assert!(result.unwrap_err().contains("CAS conflict"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external change");
        assert!(
            !std::fs::read_dir(path.parent().unwrap()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".review-"))
        );
        assert!(atomic_synced_write(&path, b"must not clobber").is_err());
    }

    #[test]
    fn review_skills_atomic_commit_rechecks_late_lock() {
        let root = fixture();
        let dir = root.join(".aether/skills/manual");
        let path = dir.join("SKILL.md");
        let original = raw_read(&path).unwrap();
        let result = atomic_synced_write_checked(&path, b"replacement", || {
            std::fs::write(dir.join(".locked"), "late lock").unwrap();
            ensure_unlocked(&dir)?;
            check_cas(&path, Some(&original))
        });
        assert!(result.unwrap_err().contains("locked"));
        assert_eq!(raw_read(&path).unwrap(), original);
    }

    #[tokio::test]
    async fn review_skills_locks_block_support_mutations() {
        let root = fixture();
        let dir = root.join(".aether/skills/manual");
        let main = dir.join("SKILL.md");
        let mut store = ReviewSkillStore::new(
            root.to_str().unwrap(),
            &[],
            root.join("created"),
            root.join("pending"),
            false,
            "run",
            "session",
        )
        .await
        .unwrap();
        std::fs::write(dir.join(".locked"), "locked").unwrap();
        let write = json!({"action":"write_file","name":"manual","file_path":"scripts/task.sh","content":"echo safe"});
        assert!(store
            .execute("skill_manage", &write)
            .await
            .unwrap_err()
            .contains("locked"));
        std::fs::remove_file(dir.join(".locked")).unwrap();
        let original = std::fs::read_to_string(&main).unwrap();
        std::fs::write(
            &main,
            original.replace("  custom: preserved", "  aether:\r\n    locked: true"),
        )
        .unwrap();
        assert!(store
            .execute("skill_manage", &write)
            .await
            .unwrap_err()
            .contains("locked"));
        assert!(!dir.join("scripts/task.sh").exists());
    }

    #[test]
    fn review_skills_search_is_bounded_and_support_paths_are_portable() {
        let root = fixture();
        let dir = root.join(".aether/skills/manual");
        for path in [
            "scripts/NUL.txt",
            "scripts/COM1.sh",
            "references/x.",
            "templates/a\\b",
            "scripts/a?b",
            "references/../escape",
        ] {
            assert!(relative_target(&dir, path).is_err(), "{path}");
        }
        let mut files = Vec::new();
        collect_files(&dir, 0, &mut files).unwrap();
        assert_eq!(files, vec![dir.join("SKILL.md")]);
        assert!(collect_files(&dir, 17, &mut Vec::new()).is_err());
        assert!(collect_files(&dir, 0, &mut vec![PathBuf::new(); 1000]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn review_skills_symlink_support_and_search_escape_is_rejected() {
        let root = fixture();
        let dir = root.join(".aether/skills/manual");
        std::os::unix::fs::symlink(&root, dir.join("references")).unwrap();
        assert!(relative_target(&dir, "references/secret").is_err());
        assert!(collect_files(&dir, 0, &mut Vec::new()).is_err());
    }

    #[test]
    fn review_skills_metadata_must_remain_byte_exact() {
        let old =
            "---\r\nname: manual\r\ndescription: Manual\r\n# keep this comment\r\n---\r\nBody";
        assert!(validate_skill_update(old, &old.replace("# keep this comment\r\n", "")).is_err());
        assert!(validate_skill_update(old, &old.replace("\r\n", "\n")).is_err());
        assert!(validate_skill_update(old, &old.replace("Body", "New body")).is_ok());
    }

    #[tokio::test]
    async fn review_skills_views_exact_raw_discovered_content() {
        let root = fixture();
        let mut store = ReviewSkillStore::new(
            root.to_str().unwrap(),
            &[],
            root.join(".aether-agent/skills"),
            root.join("pending"),
            false,
            "run",
            "session",
        )
        .await
        .unwrap();
        let catalog = store.execute("skills_list", &json!({})).await.unwrap();
        assert!(catalog["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "manual"));
        let viewed = store
            .execute("skill_view", &json!({"name":"manual"}))
            .await
            .unwrap();
        assert_eq!(
            viewed["content"],
            std::fs::read_to_string(root.join(".aether/skills/manual/SKILL.md")).unwrap()
        );
        assert!(store.changes().is_empty());
    }
}
