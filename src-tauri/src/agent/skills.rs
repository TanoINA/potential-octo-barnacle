use super::types::SkillInfo;
use ignore::WalkBuilder;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

pub fn default_skill_paths() -> Vec<String> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        // 1. Official standalone AetherAgent global skills directory
        roots.push(home.join(".aether-agent").join("skills"));
        // 2. Compatibility with Antigravity if present
        roots.push(home.join(".gemini").join("config").join("skills"));
    }
    roots
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect()
}

pub fn ensure_starter_skills(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);

    let debug_dir = dir.join("systematic-debugging");
    let _ = std::fs::create_dir_all(&debug_dir);
    let debug_file = debug_dir.join("SKILL.md");
    if !debug_file.exists() {
        let _ = std::fs::write(
            debug_file,
            "---\nname: systematic-debugging\ndescription: 4-phase root cause debugging: understand bugs thoroughly before fixing them.\n---\n# Systematic Debugging\n\n## Phase 1: Reproduce & Isolate\n- Observe exact symptoms, stack traces, and failure messages.\n- Use `run_command` to execute tests or reproduce issues.\n- Use `search_files` and `read_file` to locate the failure origin.\n\n## Phase 2: Root Cause Investigation\n- Formulate a testable hypothesis before editing code.\n- Verify variables, control flow, and data contracts.\n\n## Phase 3: Minimal Fix\n- Apply the most focused, targeted correction using `edit_file`.\n- Preserve existing coding conventions and contracts.\n\n## Phase 4: Verify & Regression Test\n- Run test suites using `run_command` to verify the fix works.\n- Verify that unrelated tests still pass.\n",
        );
    }

    let inspect_dir = dir.join("codebase-inspection");
    let _ = std::fs::create_dir_all(&inspect_dir);
    let inspect_file = inspect_dir.join("SKILL.md");
    if !inspect_file.exists() {
        let _ = std::fs::write(
            inspect_file,
            "---\nname: codebase-inspection\ndescription: Inspect project codebases, language distribution, architecture, and directory layout.\n---\n# Codebase Inspection\n\n- Use `list_directory` to explore directory hierarchy and layout.\n- Use `search_files` to locate configurations, entry points, or specific functions.\n- Use `read_file` to inspect dependency manifests (`package.json`, `Cargo.toml`, etc.).\n- Use `run_command` to run non-destructive inspection tools (e.g. `git status`, test runners).\n",
        );
    }

    let tdd_dir = dir.join("test-driven-development");
    let _ = std::fs::create_dir_all(&tdd_dir);
    let tdd_file = tdd_dir.join("SKILL.md");
    if !tdd_file.exists() {
        let _ = std::fs::write(
            tdd_file,
            "---\nname: test-driven-development\ndescription: Enforce test-driven development: verify tests before and after code changes.\n---\n# Test-Driven Development (TDD)\n\n1. **Baseline**: Run existing tests via `run_command` to establish a clean starting baseline.\n2. **Red**: Write a minimal failing test that specifies the expected behavior.\n3. **Green**: Implement minimal change using `write_file` or `edit_file` and verify test pass.\n4. **Refactor**: Clean up the code while keeping tests green.\n5. **Verify**: Run full unit and integration tests via `run_command` to ensure zero regressions.\n",
        );
    }
}

pub fn init_skills_environment() {
    if let Some(home) = dirs::home_dir() {
        ensure_starter_skills(&home.join(".aether-agent").join("skills"));
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

pub async fn load_skills(configured_paths: &[String]) -> Vec<SkillInfo> {
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
    let home = dirs::home_dir().ok_or("Cannot resolve user home directory")?;
    save_skill_markdown_at(
        &home.join(".aether-agent"),
        name,
        description,
        instructions,
        overwrite,
    )
    .await
}

async fn save_skill_markdown_at(
    config_root: &Path,
    name: &str,
    description: &str,
    instructions: &str,
    overwrite: bool,
) -> Result<SkillInfo, String> {
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

    super::storage::atomic_write(&file_path, markdown.as_bytes()).await?;

    Ok(SkillInfo {
        name: clean_name,
        description: description.trim().to_string(),
        path: file_path.to_string_lossy().to_string(),
        source: target_dir.to_string_lossy().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn default_skill_paths_do_not_include_machine_specific_directories() {
        for path in default_skill_paths() {
            let lowered = path.to_ascii_lowercase();
            assert!(
                !lowered.contains("hermes-agent"),
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
