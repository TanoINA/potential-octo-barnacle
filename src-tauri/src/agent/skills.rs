use super::types::{ReviewConfig, SkillInfo};
use ignore::WalkBuilder;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    metadata: Option<serde_json::Value>,
}

// Unit tests must never discover or initialize the actual user's skill library.
#[cfg(not(feature = "shared-profile"))]
fn skills_home() -> Option<PathBuf> {
    #[cfg(feature = "diagnostics")]
    if let Some(root) = super::native_smoke::profile() {
        return Some(root.join("compatibility"));
    }
    #[cfg(test)]
    {
        Some(super::storage::config_dir().join("compatibility"))
    }
    #[cfg(not(test))]
    dirs::home_dir()
}

fn skills_config_root() -> Option<PathBuf> {
    #[cfg(feature = "shared-profile")]
    {
        Some(super::storage::config_dir())
    }
    #[cfg(not(feature = "shared-profile"))]
    {
        #[cfg(feature = "diagnostics")]
        if let Some(root) = super::native_smoke::profile() {
            return Some(root.clone());
        }
        #[cfg(test)]
        {
            Some(super::storage::config_dir())
        }
        #[cfg(not(test))]
        skills_home().map(|home| home.join(".aether-agent"))
    }
}

pub fn default_skill_paths() -> Vec<String> {
    // Rune never probes other applications' global libraries automatically.
    // External roots remain available through explicitly configured skill_paths.
    #[cfg(feature = "shared-profile")]
    let roots = vec![super::storage::config_dir().join("skills")];
    #[cfg(not(feature = "shared-profile"))]
    let roots = {
        let mut roots = Vec::new();
        if let Some(home) = skills_home() {
            // 1. Official standalone AetherAgent global skills directory
            if let Some(config) = skills_config_root() {
                roots.push(config.join("skills"));
            }
            // 2. Global user skills if present
            roots.push(home.join(".gemini").join("config").join("skills"));
        }
        roots
    };
    roots
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect()
}

fn write_starter_skill(dir: &Path, name: &str, content: &str) {
    let skill_dir = dir.join(name);
    let _ = std::fs::create_dir_all(&skill_dir);
    let skill_file = skill_dir.join("SKILL.md");
    if !skill_file.exists() {
        let _ = std::fs::write(skill_file, content);
    }
}

pub fn ensure_starter_skills(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);

    write_starter_skill(
        dir,
        "systematic-debugging",
        "---\nname: systematic-debugging\ndescription: '4-phase root cause debugging: understand bugs thoroughly before fixing them.'\n---\n# Systematic Debugging\n\n## Phase 1: Reproduce & Isolate\n- Observe exact symptoms, stack traces, and failure messages.\n- Use `run_command` to execute tests or reproduce issues.\n- Use `search_files` and `read_file` to locate the failure origin.\n\n## Phase 2: Root Cause Investigation\n- Formulate a testable hypothesis before editing code.\n- Verify variables, control flow, and data contracts.\n\n## Phase 3: Minimal Fix\n- Apply the most focused, targeted correction using `edit_file`.\n- Preserve existing coding conventions and contracts.\n\n## Phase 4: Verify & Regression Test\n- Run test suites using `run_command` to verify the fix works.\n- Verify that unrelated tests still pass.\n",
    );

    write_starter_skill(
        dir,
        "codebase-inspection",
        "---\nname: codebase-inspection\ndescription: 'Inspect project codebases, language distribution, architecture, and directory layout.'\n---\n# Codebase Inspection\n\n- Use `list_directory` to explore directory hierarchy and layout.\n- Use `search_files` to locate configurations, entry points, or specific functions.\n- Use `read_file` to inspect dependency manifests (`package.json`, `Cargo.toml`, etc.).\n- Use `run_command` to run non-destructive inspection tools (e.g. `git status`, test runners).\n",
    );

    write_starter_skill(
        dir,
        "test-driven-development",
        "---\nname: test-driven-development\ndescription: 'Enforce test-driven development: verify tests before and after code changes.'\n---\n# Test-Driven Development (TDD)\n\n1. **Baseline**: Run existing tests via `run_command` to establish a clean starting baseline.\n2. **Red**: Write a minimal failing test that specifies the expected behavior.\n3. **Green**: Implement minimal change using `write_file` or `edit_file` and verify test pass.\n4. **Refactor**: Clean up the code while keeping tests green.\n5. **Verify**: Run full unit and integration tests via `run_command` to ensure zero regressions.\n",
    );

    write_starter_skill(
        dir,
        "ponytail",
        include_str!("../../../.aether/skills/ponytail/SKILL.md"),
    );
    write_starter_skill(
        dir,
        "ponytail-audit",
        include_str!("../../../.aether/skills/ponytail-audit/SKILL.md"),
    );
    write_starter_skill(
        dir,
        "ponytail-debt",
        include_str!("../../../.aether/skills/ponytail-debt/SKILL.md"),
    );
    write_starter_skill(
        dir,
        "ponytail-gain",
        include_str!("../../../.aether/skills/ponytail-gain/SKILL.md"),
    );
    write_starter_skill(
        dir,
        "ponytail-help",
        include_str!("../../../.aether/skills/ponytail-help/SKILL.md"),
    );
    write_starter_skill(
        dir,
        "ponytail-review",
        include_str!("../../../.aether/skills/ponytail-review/SKILL.md"),
    );
}

pub fn init_skills_environment() {
    #[cfg(feature = "diagnostics")]
    if super::native_smoke::profile().is_some() {
        return;
    }
    #[cfg(test)]
    {
        // Concurrent test runtimes share this process-local library. Publish
        // starters only after initialization completes, never halfway through a write.
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            if let Some(config) = skills_config_root() {
                ensure_starter_skills(&config.join("skills"));
            }
        });
    }
    #[cfg(not(test))]
    if let Some(config) = skills_config_root() {
        ensure_starter_skills(&config.join("skills"));
    }
}

pub fn workspace_skill_paths(workspace_cwd: &str, configured: &[String]) -> Vec<String> {
    let mut roots = Vec::new();
    if !workspace_cwd.trim().is_empty() {
        roots.push(
            Path::new(workspace_cwd)
                .join(".aether/skills")
                .to_string_lossy()
                .to_string(),
        );
    }
    roots.extend_from_slice(configured);
    roots
}

/// Resolve review creation paths relative to the user configuration, never the workspace.
pub fn review_creation_root(config: &ReviewConfig) -> Result<PathBuf, String> {
    #[cfg(feature = "shared-profile")]
    let base = super::storage::config_dir();
    #[cfg(not(feature = "shared-profile"))]
    let base = dirs::home_dir()
        .ok_or("Home directory unavailable")?
        .join(".aether-agent");
    let Some(value) = config
        .create_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(base.join("skills"));
    };
    if value == "~" {
        return dirs::home_dir().ok_or_else(|| "Home directory unavailable".into());
    }
    if let Some(relative) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return dirs::home_dir()
            .map(|home| home.join(relative))
            .ok_or_else(|| "Home directory unavailable".into());
    }
    if value.starts_with('~') {
        return Err("Unsupported home-directory expansion".into());
    }
    let path = PathBuf::from(value);
    Ok(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

/// Workspace and explicitly configured roots win over review-created skills.
/// `load_skills` appends the default global roots after these paths.
pub fn effective_skill_paths(
    workspace: &str,
    configured: &[String],
    review_config: &ReviewConfig,
) -> Result<Vec<String>, String> {
    let mut paths = workspace_skill_paths(workspace, configured);
    paths.push(
        review_creation_root(review_config)?
            .to_string_lossy()
            .into_owned(),
    );
    Ok(paths)
}
pub async fn load_skills(configured_paths: &[String]) -> Vec<SkillInfo> {
    #[cfg(feature = "diagnostics")]
    if super::native_smoke::profile().is_some() {
        return Vec::new();
    }
    init_skills_environment();

    let mut roots: Vec<PathBuf> = configured_paths
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect();
    roots.extend(default_skill_paths().into_iter().map(PathBuf::from));

    let mut unique_roots = HashSet::new();
    roots.retain(|path| unique_roots.insert(path.to_string_lossy().to_lowercase()));

    let mut skills = Vec::new();
    let mut seen_names = HashSet::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        let files = if root.is_file() {
            vec![root.clone()]
        } else {
            let mut list: Vec<PathBuf> = WalkBuilder::new(&root)
                .hidden(false)
                .git_ignore(false)
                .max_depth(Some(6))
                .build()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
                .map(|entry| entry.into_path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
                })
                .take(1_000)
                .collect();
            list.sort();
            list
        };

        for path in files {
            match read_skill(&path, &root).await {
                Ok(skill) => {
                    let key = skill.name.to_lowercase();
                    if seen_names.insert(key) {
                        skills.push(skill);
                    }
                }
                Err(error) => {
                    eprintln!("Could not load skill {}: {error}", path.display());
                }
            }
        }
    }
    skills.sort_by_key(|skill| skill.name.to_lowercase());
    skills
}

async fn read_skill(path: &Path, source: &Path) -> Result<SkillInfo, String> {
    let content = read_bounded_skill(path).await?;
    let frontmatter = parse_frontmatter(&content)?;
    let name = frontmatter
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            path.parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .ok_or_else(|| "Skill has no name".to_string())?;
    let description = frontmatter
        .description
        .unwrap_or_default()
        .trim()
        .to_string();
    Ok(SkillInfo {
        name,
        description,
        path: path.to_string_lossy().to_string(),
        source: source.to_string_lossy().to_string(),
    })
}

fn parse_frontmatter(content: &str) -> Result<SkillFrontmatter, String> {
    let normalized = content
        .strip_prefix('\u{feff}')
        .unwrap_or(content)
        .replace("\r\n", "\n");
    let mut lines = normalized.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err("SKILL.md is missing YAML frontmatter".into());
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
        if yaml.len() > 64 * 1024 {
            return Err("Skill frontmatter exceeds 64 KiB".into());
        }
    }
    if !closed {
        return Err("SKILL.md frontmatter is not terminated".into());
    }
    serde_yaml::from_str(&yaml).map_err(|error| format!("Invalid skill YAML: {error}"))
}

pub fn render_skill_catalog(skills: &[SkillInfo]) -> String {
    if skills.is_empty() {
        return "\n## Skills\nNo SKILL.md catalogs were found in the configured skill roots."
            .into();
    }
    let mut output = String::from(
        "\n## Available skills\nThe following skills are discovered from trusted local SKILL.md files. Use a skill when its description matches the task; its path identifies the source catalog entry.\n",
    );
    for skill in skills {
        output.push_str(&format!(
            "- {}: {} ({})\n",
            skill.name,
            if skill.description.is_empty() {
                "No description"
            } else {
                &skill.description
            },
            skill.path
        ));
    }
    output
}

pub fn find_skill<'a>(skills: &'a [SkillInfo], name: &str) -> Option<&'a SkillInfo> {
    let target = name.trim().to_lowercase();
    skills.iter().find(|s| s.name.to_lowercase() == target)
}

pub fn normalize_skill_instructions(content: &str) -> String {
    content
        .replace("`terminal`", "`run_command`")
        .replace("`delegate_task`", "`spawn_subagent`")
        .replace("`web_search`", "`fetch_web_page`")
        .replace("`web_extract`", "`fetch_web_page`")
}

const MAX_SKILL_BYTES: u64 = 256 * 1024;

async fn read_bounded_skill(path: &Path) -> Result<String, String> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_SKILL_BYTES
    {
        return Err("Skill must be a regular file no larger than 256 KiB".into());
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| e.to_string())?;
    let opened = file.metadata().await.map_err(|e| e.to_string())?;
    if !opened.is_file() || opened.len() > MAX_SKILL_BYTES {
        return Err("Skill changed or exceeds 256 KiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_SKILL_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_SKILL_BYTES {
        return Err("Skill exceeds 256 KiB".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

pub async fn read_skill_content(skill: &SkillInfo) -> Result<String, String> {
    let source = Path::new(&skill.source);
    let root = if source.is_file() {
        source.parent().ok_or("Skill source has no parent")?
    } else {
        source
    };
    let path = std::fs::canonicalize(&skill.path).map_err(|e| e.to_string())?;
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    if !path.starts_with(&root) {
        return Err("Skill target escaped its discovered root".into());
    }
    let raw = read_bounded_skill(Path::new(&skill.path)).await?;
    let metadata = parse_frontmatter(&raw)?;
    let current_name = metadata
        .name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| path.parent()?.file_name()?.to_str().map(str::to_string));
    if current_name.as_deref() != Some(skill.name.as_str())
        || metadata.description.unwrap_or_default().trim() != skill.description.trim()
    {
        return Err("Skill metadata changed since discovery; rediscover before activation".into());
    }
    Ok(normalize_skill_instructions(&raw))
}

pub async fn save_skill_markdown(
    name: &str,
    description: &str,
    instructions: &str,
    overwrite: bool,
) -> Result<SkillInfo, String> {
    let config = skills_config_root().ok_or("Cannot resolve user home directory")?;
    save_skill_markdown_at(&config, name, description, instructions, overwrite).await
}

// All legacy save callers, including global sync, must respect review locks.
async fn ensure_skill_save_unlocked(dir: &Path) -> Result<(), String> {
    match tokio::fs::symlink_metadata(dir.join(".locked")).await {
        Ok(_) => return Err("Skill is locked by .locked marker".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not check skill lock: {error}")),
    }
    let path = dir.join("SKILL.md");
    match tokio::fs::symlink_metadata(&path).await {
        Ok(_) => {
            let metadata = parse_frontmatter(&read_bounded_skill(&path).await?)?.metadata;
            if metadata
                .as_ref()
                .and_then(|value| value.get("aether"))
                .and_then(|value| value.get("locked"))
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            {
                return Err("Skill metadata.aether.locked is true".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not inspect skill metadata: {error}")),
    }
    Ok(())
}
async fn save_skill_markdown_at(
    config_root: &Path,
    name: &str,
    description: &str,
    instructions: &str,
    overwrite: bool,
) -> Result<SkillInfo, String> {
    let _mutation = SKILL_MUTATION_LOCK.lock().await;
    let clean_name: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut clean_name = clean_name;
    while clean_name.contains("--") {
        // Collapse separator runs so the slug stays a single-hyphen slug.
        clean_name = clean_name.replace("--", "-");
    }
    let clean_name = clean_name.trim_matches('-').to_string();
    if clean_name.is_empty() {
        return Err("Skill name must contain valid alphanumeric characters".into());
    }
    // Reuse the reflection skill-name contract: lowercase ASCII slug, at most
    // 64 bytes, and never a Windows reserved device name (a "nul" or "con"
    // directory silently disappears or collides on Windows filesystems).
    super::reflection::validate_name(&clean_name)?;
    if description.trim().len() > super::reflection::MAX_DESCRIPTION_BYTES {
        return Err(format!(
            "Skill description exceeds {} bytes",
            super::reflection::MAX_DESCRIPTION_BYTES
        ));
    }
    if instructions.trim().is_empty() {
        return Err("Skill instructions must not be empty".into());
    }
    if instructions.trim().len() > super::reflection::MAX_BODY_BYTES {
        return Err(format!(
            "Skill instructions exceed {} bytes",
            super::reflection::MAX_BODY_BYTES
        ));
    }

    tokio::fs::create_dir_all(config_root)
        .await
        .map_err(|e| e.to_string())?;
    let root = config_root.to_string_lossy();
    let relative = format!("skills/{clean_name}/SKILL.md");
    let file_path = super::tools::resolve_workspace_path(&root, &relative)?;
    let target_dir = file_path
        .parent()
        .ok_or("Skill path has no parent")?
        .to_path_buf();
    ensure_skill_save_unlocked(&target_dir).await?;
    if file_path.exists() && !overwrite {
        return Err(format!(
            "Skill '{clean_name}' already exists at {}. To overwrite it, call learn_skill with overwrite: true.",
            file_path.display()
        ));
    }
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| format!("Failed to create skill directory: {e}"))?;

    let file_path = super::tools::resolve_workspace_path(&root, &relative)?;
    let frontmatter = serde_yaml::to_string(&serde_json::json!({
        "name": clean_name,
        "description": description.trim(),
    }))
    .map_err(|error| format!("Failed to encode skill metadata: {error}"))?;
    let trimmed_instructions = instructions.trim();
    let markdown = if trimmed_instructions.starts_with("# ") {
        format!("---\n{}---\n{}\n", frontmatter, trimmed_instructions)
    } else {
        format!(
            "---\n{}---\n# {}\n\n{}\n",
            frontmatter, clean_name, trimmed_instructions
        )
    };

    ensure_skill_save_unlocked(&target_dir).await?;
    super::storage::atomic_write(&file_path, markdown.as_bytes()).await?;

    Ok(SkillInfo {
        name: clean_name,
        description: description.trim().to_string(),
        path: file_path.to_string_lossy().to_string(),
        source: target_dir.to_string_lossy().to_string(),
    })
}

// Acquire before review-store or pending writers; stage/list never acquire this lock.
pub(crate) static SKILL_MUTATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_hermetic_default_skill_roots() {
        let config = super::super::storage::config_dir();
        let roots = default_skill_paths();
        #[cfg(feature = "shared-profile")]
        assert_eq!(roots.len(), 1);
        #[cfg(not(feature = "shared-profile"))]
        assert_eq!(roots.len(), 2);
        for root in &roots {
            assert!(
                Path::new(root).starts_with(&config),
                "test root escaped temporary config: {root}"
            );
        }
        assert_eq!(Path::new(&roots[0]), config.join("skills"));
        #[cfg(not(feature = "shared-profile"))]
        assert_eq!(
            Path::new(&roots[1]),
            config.join("compatibility/.gemini/config/skills")
        );
        init_skills_environment();
        for name in [
            "systematic-debugging",
            "codebase-inspection",
            "test-driven-development",
        ] {
            assert!(config.join("skills").join(name).join("SKILL.md").is_file());
        }
    }

    #[tokio::test]
    async fn regression_hermetic_starter_yaml_parses_and_loads() {
        let root = fixture_root();
        ensure_starter_skills(&root);
        for name in [
            "systematic-debugging",
            "codebase-inspection",
            "test-driven-development",
            "ponytail",
            "ponytail-audit",
            "ponytail-debt",
            "ponytail-gain",
            "ponytail-help",
            "ponytail-review",
        ] {
            let path = root.join(name).join("SKILL.md");
            let markdown = std::fs::read_to_string(&path).unwrap();
            let parsed = parse_frontmatter(&markdown)
                .unwrap_or_else(|error| panic!("starter {name}: {error}"));
            assert_eq!(parsed.name.as_deref(), Some(name));
            assert!(!parsed.description.unwrap().is_empty());
            let skill = read_skill(&path, &root).await.unwrap();
            assert_eq!(skill.name, name);
            assert!(!read_skill_content(&skill).await.unwrap().is_empty());
        }
        let catalog = load_skills(&[root.to_string_lossy().into_owned()]).await;
        for name in [
            "systematic-debugging",
            "codebase-inspection",
            "test-driven-development",
            "ponytail",
            "ponytail-audit",
            "ponytail-debt",
            "ponytail-gain",
            "ponytail-help",
            "ponytail-review",
        ] {
            let skill = find_skill(&catalog, name).expect("generated starter must be discoverable");
            assert_eq!(Path::new(&skill.source), root);
        }
        // Seeding must never repair or replace an existing user's starter.
        let edited = root.join("systematic-debugging/SKILL.md");
        let user_bytes = "Existing user-owned instructions, even without valid YAML.\n";
        std::fs::write(&edited, user_bytes).unwrap();
        ensure_starter_skills(&root);
        assert_eq!(std::fs::read_to_string(edited).unwrap(), user_bytes);
    }

    #[tokio::test]
    async fn shared_skill_mutation_lock_blocks_approval_direct_create_and_legacy_save() {
        use super::super::review_pending::{approve_pending, list_pending, stage, PendingWrite};
        use super::super::review_skills::ReviewSkillStore;
        use serde_json::json;
        use std::time::Duration;
        use tokio::time::timeout;

        let root = fixture_root();
        let pending = root.join("pending");
        let skill = root.join("manual");
        std::fs::create_dir_all(&skill).unwrap();
        let target = skill.join("notes.md");
        std::fs::write(&target, "before").unwrap();
        let proposal = PendingWrite {
            id: String::new(),
            session_id: "session".into(),
            run_id: "run".into(),
            skill_name: "manual".into(),
            target_path: target.to_string_lossy().into_owned(),
            skill_root: skill.to_string_lossy().into_owned(),
            original: Some("before".into()),
            proposed: Some("after".into()),
        };
        let id = stage(&pending, proposal.clone()).await.unwrap();
        let mut store = ReviewSkillStore::new(
            root.to_str().unwrap(),
            &[],
            root.join("created"),
            pending.clone(),
            false,
            "run",
            "session",
        )
        .await
        .unwrap();
        let guard = SKILL_MUTATION_LOCK.lock().await;
        // Stage/list must stay usable while their caller holds the common lock.
        timeout(Duration::from_secs(2), stage(&pending, proposal))
            .await
            .unwrap()
            .unwrap();
        let (started, mut ready) = tokio::sync::mpsc::unbounded_channel();
        let mut tasks = Vec::new();
        let approve_root = pending.clone();
        let signal = started.clone();
        tasks.push(tokio::spawn(async move {
            signal.send(()).unwrap();
            approve_pending(&approve_root, &id, "session")
                .await
                .map(|_| ())
        }));
        let signal = started.clone();
        tasks.push(tokio::spawn(async move {
            signal.send(()).unwrap();
            store.execute("skill_manage", &json!({"action":"create","name":"direct-recipe","description":"Recipe","content":"Steps"})).await.map(|_| ())
        }));
        let legacy_root = root.join("legacy");
        tasks.push(tokio::spawn(async move {
            started.send(()).unwrap();
            save_skill_markdown_at(&legacy_root, "legacy-recipe", "Recipe", "Steps", false)
                .await
                .map(|_| ())
        }));
        // Each task signals immediately before entering its mutation future.
        // This current-thread runtime polls it through to its first suspension.
        for _ in 0..tasks.len() {
            if timeout(Duration::from_secs(2), ready.recv())
                .await
                .ok()
                .flatten()
                .is_none()
            {
                for task in &tasks {
                    task.abort();
                }
                panic!("writer task did not start");
            }
        }
        let mut completed_early = false;
        for task in &mut tasks {
            completed_early |= timeout(Duration::from_millis(30), task).await.is_ok();
        }
        // Approval must wait for common BEFORE taking pending WRITER.
        let pending_available =
            timeout(Duration::from_secs(2), list_pending(&pending, "session")).await;
        if completed_early || pending_available.is_err() {
            for task in &tasks {
                task.abort();
            }
            panic!("writers must wait for common lock without holding pending WRITER");
        }
        assert_eq!(pending_available.unwrap().unwrap().len(), 2);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "before");
        assert!(!root.join("created").exists());
        assert!(!root.join("legacy").exists());
        drop(guard);
        let results = timeout(
            Duration::from_secs(5),
            futures_util::future::join_all(tasks.iter_mut()),
        )
        .await;
        if results.is_err() {
            for task in &tasks {
                task.abort();
            }
            panic!("writers did not resume after releasing common lock");
        }
        for result in results.unwrap() {
            result.unwrap().unwrap();
        }
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "after");
        assert!(root.join("created/direct-recipe/SKILL.md").is_file());
        assert!(root.join("legacy/skills/legacy-recipe/SKILL.md").is_file());
    }
    #[tokio::test]
    async fn custom_review_root_is_discoverable_and_activatable_after_configured_roots() {
        let root = fixture_root();
        let created = root.join("created");
        let workspace = root.join("workspace");
        let configured = root.join("configured");
        let name = format!("review-discovery-{}", uuid::Uuid::new_v4());
        for (base, body) in [
            (&created, "created recipe"),
            (&configured, "configured recipe"),
        ] {
            let dir = base.join(&name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: test discovery\n---\n{body}\n"),
            )
            .unwrap();
        }
        let config = super::super::types::ReviewConfig {
            create_dir: Some(created.to_string_lossy().into_owned()),
            ..Default::default()
        };
        assert_eq!(review_creation_root(&config).unwrap(), created);
        let paths = effective_skill_paths(
            workspace.to_str().unwrap(),
            &[configured.to_string_lossy().into_owned()],
            &config,
        )
        .unwrap();
        assert_eq!(Path::new(&paths[0]), workspace.join(".aether/skills"));
        assert_eq!(Path::new(&paths[1]), configured);
        assert_eq!(Path::new(&paths[2]), created);
        let catalog = load_skills(&paths).await;
        let skill = find_skill(&catalog, &name).unwrap();
        assert!(read_skill_content(skill)
            .await
            .unwrap()
            .contains("configured recipe"));
        let paths = effective_skill_paths("", &[], &config).unwrap();
        let catalog = load_skills(&paths).await;
        let skill =
            find_skill(&catalog, &name).expect("custom review creation root must be discoverable");
        assert!(read_skill_content(skill)
            .await
            .unwrap()
            .contains("created recipe"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_creation_root_preserves_home_relative_expansion() {
        let home = dirs::home_dir().unwrap();
        #[cfg(feature = "shared-profile")]
        let base = super::super::storage::config_dir();
        #[cfg(not(feature = "shared-profile"))]
        let base = home.join(".aether-agent");
        for (value, expected) in [
            (None, base.join("skills")),
            (Some("  "), base.join("skills")),
            (Some("custom-skills"), base.join("custom-skills")),
            (Some("~"), home.clone()),
            (Some(" ~/recipes "), home.join("recipes")),
            (Some("~\\recipes"), home.join("recipes")),
        ] {
            let config = super::super::types::ReviewConfig {
                create_dir: value.map(str::to_owned),
                ..Default::default()
            };
            assert_eq!(review_creation_root(&config).unwrap(), expected);
        }
        let config = super::super::types::ReviewConfig {
            create_dir: Some("~someone/recipes".into()),
            ..Default::default()
        };
        assert!(effective_skill_paths("", &[], &config)
            .unwrap_err()
            .contains("Unsupported"));
    }
    #[tokio::test]
    async fn save_skill_markdown_respects_marker_and_metadata_locks() {
        let root = fixture_root();
        // Both learn_skill and the legacy global sync writer use this helper.
        for metadata_lock in [false, true] {
            let name = if metadata_lock {
                "metadata-locked"
            } else {
                "marker-locked"
            };
            let dir = root.join("skills").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let original = format!("---\nname: {name}\ndescription: preserved\nmetadata:\n  aether:\n    locked: {metadata_lock}\n---\nOriginal instructions\n");
            std::fs::write(dir.join("SKILL.md"), &original).unwrap();
            if !metadata_lock {
                std::fs::write(dir.join(".locked"), "protected").unwrap();
            }
            let result = save_skill_markdown_at(
                &root,
                name,
                "replacement",
                "Replacement instructions",
                true,
            )
            .await;
            assert!(result.is_err(), "overwrite must not bypass a skill lock");
            assert!(result.unwrap_err().contains("locked"));
            assert_eq!(
                std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
                original
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    fn fixture_root() -> PathBuf {
        let root = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("aether-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn regression_skill_save_rejects_external_parent_link() {
        let root = fixture_root();
        let config = root.join("config");
        let outside = root.join("outside");
        tokio::fs::create_dir(&config).await.unwrap();
        tokio::fs::create_dir(&outside).await.unwrap();
        let link = config.join("skills");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link.to_string_lossy().replace('/', "\\"))
                .arg(outside.to_string_lossy().replace('/', "\\"))
                .status()
                .unwrap();
            assert!(
                status.success(),
                "Could not create isolated junction fixture"
            );
        }
        assert!(
            save_skill_markdown_at(&config, "safe-name", "description", "body", false)
                .await
                .is_err()
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        #[cfg(windows)]
        tokio::fs::remove_dir(&link).await.unwrap();
        #[cfg(unix)]
        tokio::fs::remove_file(&link).await.unwrap();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn regression_activation_rechecks_size_type_and_identity() {
        let root = fixture_root();
        let path = root.join("SKILL.md");
        tokio::fs::write(&path, "---\nname: safe-skill\ndescription: safe\n---\nbody")
            .await
            .unwrap();
        let skill = read_skill(&path, &root).await.unwrap();
        tokio::fs::write(&path, "a".repeat(256 * 1024 + 1))
            .await
            .unwrap();
        assert!(read_skill_content(&skill).await.is_err());
        tokio::fs::write(&path, "---\nname: swapped-skill\n---\nother")
            .await
            .unwrap();
        assert!(read_skill_content(&skill).await.is_err());
        tokio::fs::remove_file(&path).await.unwrap();
        tokio::fs::create_dir(&path).await.unwrap();
        assert!(read_skill_content(&skill).await.is_err());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn discovers_workspace_skills_before_global_namesakes() {
        let paths = workspace_skill_paths("project-root", &["custom-root".into()]);
        assert_eq!(
            paths[0],
            std::path::Path::new("project-root")
                .join(".aether/skills")
                .to_string_lossy()
        );
        assert_eq!(paths[1], "custom-root");
        assert_eq!(workspace_skill_paths("", &[]), Vec::<String>::new());
    }

    #[test]
    fn parses_yaml_frontmatter() {
        let parsed = parse_frontmatter(
            "---\nname: code-review\ndescription: Review code safely\n---\n# Body",
        )
        .expect("valid frontmatter");
        assert_eq!(parsed.name.as_deref(), Some("code-review"));
        assert_eq!(parsed.description.as_deref(), Some("Review code safely"));
    }

    #[test]
    fn rejects_missing_frontmatter() {
        assert!(parse_frontmatter("# Not a skill").is_err());
    }

    #[tokio::test]
    async fn test_save_skill_markdown() {
        let root = fixture_root();
        let unique = format!(
            "test-skill-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let res = save_skill_markdown_at(
            &root,
            &unique,
            "Test description",
            "Test instructions",
            false,
        )
        .await;
        assert!(res.is_ok());
        let skill = res.unwrap();
        assert_eq!(skill.name, unique);
        assert!(std::path::Path::new(&skill.path).exists());
        let content = tokio::fs::read_to_string(&skill.path).await.unwrap();
        assert!(content.contains(&unique));
        assert!(content.contains("Test description"));
        assert!(content.contains("Test instructions"));
        // clean up
        tokio::fs::remove_dir_all(&root).await.unwrap();
    }

    #[tokio::test]
    async fn save_skill_markdown_guards_against_accidental_overwrite() {
        let root = fixture_root();
        let unique = format!(
            "guard-skill-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let res =
            save_skill_markdown_at(&root, &unique, "Initial", "Initial instructions", false).await;
        assert!(res.is_ok());

        // Attempting to overwrite without overwrite: true must fail
        let duplicate =
            save_skill_markdown_at(&root, &unique, "Second", "Second instructions", false).await;
        assert!(duplicate.is_err());
        assert!(duplicate.unwrap_err().contains("already exists"));

        // Overwrite with overwrite: true must succeed
        let overwritten =
            save_skill_markdown_at(&root, &unique, "Second", "Updated instructions", true).await;
        assert!(overwritten.is_ok());
        let content = tokio::fs::read_to_string(&overwritten.unwrap().path)
            .await
            .unwrap();
        assert!(content.contains("Updated instructions"));

        tokio::fs::remove_dir_all(&root).await.unwrap();
    }

    #[test]
    fn seeds_starter_skills_in_fresh_directory() {
        let unique_name = format!(
            "aether_skills_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let temp = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(unique_name);
        ensure_starter_skills(&temp);
        assert!(temp.join("systematic-debugging").join("SKILL.md").exists());
        assert!(temp.join("codebase-inspection").join("SKILL.md").exists());
        assert!(temp
            .join("test-driven-development")
            .join("SKILL.md")
            .exists());
        assert!(temp.join("ponytail").join("SKILL.md").exists());
        assert!(temp.join("ponytail-audit").join("SKILL.md").exists());
        assert!(temp.join("ponytail-debt").join("SKILL.md").exists());
        assert!(temp.join("ponytail-gain").join("SKILL.md").exists());
        assert!(temp.join("ponytail-help").join("SKILL.md").exists());
        assert!(temp.join("ponytail-review").join("SKILL.md").exists());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn default_skill_paths_do_not_include_machine_specific_directories() {
        for path in default_skill_paths() {
            let lowered = path.to_ascii_lowercase();
            assert!(
                !lowered.contains("developer") && !lowered.contains("machine-specific"),
                "hardcoded developer path leaked into defaults: {path}"
            );
            assert!(
                !path.contains("D:\\") && !path.starts_with("D:/"),
                "hardcoded absolute developer path leaked into defaults: {path}"
            );
        }
    }

    #[tokio::test]
    async fn save_skill_markdown_rejects_unsafe_or_oversized_names() {
        let root = fixture_root();
        // Windows-reserved device names must be refused before touching disk.
        for reserved in ["nul", "con", "prn", "aux", "com1", "lpt9"] {
            let error = save_skill_markdown_at(&root, reserved, "d", "instructions", false)
                .await
                .expect_err("reserved device name must be rejected");
            assert!(error.contains("reserved"), "{error}");
        }
        // Empty or oversized slugs are rejected as well.
        assert!(
            save_skill_markdown_at(&root, "", "d", "instructions", false)
                .await
                .is_err()
        );
        assert!(
            save_skill_markdown_at(&root, "   ", "d", "instructions", false)
                .await
                .is_err()
        );
        let long = "a".repeat(65);
        assert!(
            save_skill_markdown_at(&root, &long, "d", "instructions", false)
                .await
                .is_err()
        );
        // Oversized description / instruction bodies are rejected.
        assert!(
            save_skill_markdown_at(&root, "ok-name", &"d".repeat(1025), "instructions", false)
                .await
                .is_err()
        );
        assert!(
            save_skill_markdown_at(&root, "ok-name", "d", &"i".repeat(32769), false)
                .await
                .is_err()
        );
        // A normal, normalized name still works.
        let info = save_skill_markdown_at(&root, "Test Skill!", "desc", "do the thing", false)
            .await
            .expect("valid skill name must be accepted");
        assert_eq!(info.name, "test-skill");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
