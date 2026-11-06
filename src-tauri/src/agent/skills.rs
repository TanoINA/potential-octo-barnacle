use super::types::SkillInfo;
use ignore::WalkBuilder;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    if dir.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(dir);

    let debug_dir = dir.join("systematic-debugging");
    let _ = std::fs::create_dir_all(&debug_dir);
    let _ = std::fs::write(
        debug_dir.join("SKILL.md"),
        "---\nname: systematic-debugging\ndescription: 4-phase root cause debugging: understand bugs before fixing them.\n---\n# Systematic Debugging\n1. **Reproduce**: Identify reproduction steps and expected vs actual behavior.\n2. **Isolate**: Inspect logs, recent changes, and isolate the exact failing module.\n3. **Hypothesize & Test**: Form a testable hypothesis before editing code.\n4. **Fix & Verify**: Apply minimal fix, run tests, and verify no regressions.\n",
    );

    let inspect_dir = dir.join("codebase-inspection");
    let _ = std::fs::create_dir_all(&inspect_dir);
    let _ = std::fs::write(
        inspect_dir.join("SKILL.md"),
        "---\nname: codebase-inspection\ndescription: Inspect project codebases, language distribution, and directory layout.\n---\n# Codebase Inspection\nUse `list_directory` and `search_files` to understand the workspace structure before making modifications.\n",
    );

    let tdd_dir = dir.join("test-driven-development");
    let _ = std::fs::create_dir_all(&tdd_dir);
    let _ = std::fs::write(
        tdd_dir.join("SKILL.md"),
        "---\nname: test-driven-development\ndescription: Enforce test-driven development: verify tests before and after code changes.\n---\n# Test Driven Development\n1. Run existing test suites via `run_command` to ensure clean baseline.\n2. Formulate tests for new features or bug fixes.\n3. Implement minimal change and verify test pass.\n",
    );
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
            WalkBuilder::new(&root)
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
                .collect()
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
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| error.to_string())?;
    if metadata.len() > 256 * 1024 {
        return Err("SKILL.md exceeds the 256 KiB metadata/read limit".into());
    }
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| error.to_string())?;
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

pub async fn read_skill_content(skill: &SkillInfo) -> Result<String, String> {
    tokio::fs::read_to_string(&skill.path)
        .await
        .map_err(|e| format!("Failed to read skill at {}: {}", skill.path, e))
}

pub async fn save_skill_markdown(
    name: &str,
    description: &str,
    instructions: &str,
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

    let home = dirs::home_dir().ok_or("Cannot resolve user home directory")?;
    let target_dir = home.join(".aether-agent").join("skills").join(&clean_name);
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| format!("Failed to create skill directory: {e}"))?;

    let file_path = target_dir.join("SKILL.md");
    let frontmatter = serde_yaml::to_string(&serde_json::json!({
        "name": clean_name,
        "description": description.trim(),
    }))
    .map_err(|error| format!("Failed to encode skill metadata: {error}"))?;
    let markdown = format!(
        "---\n{}---\n# {}\n\n{}\n",
        frontmatter,
        clean_name,
        instructions.trim()
    );

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
        let unique = format!(
            "test-skill-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let res = save_skill_markdown(&unique, "Test description", "Test instructions").await;
        assert!(res.is_ok());
        let skill = res.unwrap();
        assert_eq!(skill.name, unique);
        assert!(std::path::Path::new(&skill.path).exists());
        let content = tokio::fs::read_to_string(&skill.path).await.unwrap();
        assert!(content.contains(&unique));
        assert!(content.contains("Test description"));
        assert!(content.contains("Test instructions"));
        // clean up
        let _ =
            tokio::fs::remove_dir_all(std::path::Path::new(&skill.path).parent().unwrap()).await;
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
        let temp = std::env::temp_dir().join(unique_name);
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
        // Windows-reserved device names must be refused before touching disk.
        for reserved in ["nul", "con", "prn", "aux", "com1", "lpt9"] {
            let error = save_skill_markdown(reserved, "d", "instructions")
                .await
                .expect_err("reserved device name must be rejected");
            assert!(error.contains("reserved"), "{error}");
        }
        // Empty or oversized slugs are rejected as well.
        assert!(save_skill_markdown("", "d", "instructions").await.is_err());
        assert!(save_skill_markdown("   ", "d", "instructions")
            .await
            .is_err());
        let long = "a".repeat(65);
        assert!(save_skill_markdown(&long, "d", "instructions")
            .await
            .is_err());
        // Oversized description / instruction bodies are rejected.
        assert!(
            save_skill_markdown("ok-name", &"d".repeat(1025), "instructions")
                .await
                .is_err()
        );
        assert!(save_skill_markdown("ok-name", "d", &"i".repeat(32769))
            .await
            .is_err());
        // A normal, normalized name still works.
        let info = save_skill_markdown("Test Skill!", "desc", "do the thing")
            .await
            .expect("valid skill name must be accepted");
        assert_eq!(info.name, "test-skill");
        let home = dirs::home_dir().expect("home dir");
        let _ =
            std::fs::remove_dir_all(home.join(".aether-agent").join("skills").join("test-skill"));
    }
}
