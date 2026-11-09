#![cfg(feature = "shared-profile")]

use aether_core::agent::{profile, review_pending, skills, storage, types::ReviewConfig};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const CHILD: &str = "AETHER_SKILL_PROFILE_CHILD";
const CASE: &str = "AETHER_SKILL_PROFILE_CASE";
const BROKEN: &str =
    "---\nname: broken-legacy\ndescription: broken YAML: must not be loaded\n---\nlegacy bytes\n";

fn legacy_roots(home: &Path) -> [PathBuf; 4] {
    [
        home.join(".aether-agent/skills"),
        home.join(".gemini/config/skills"),
        home.join(".hermes/skills"),
        home.join("hermes-agent/skills"),
    ]
}

fn run_child(case: &str) {
    if std::env::var_os(CHILD).is_some() {
        let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        // Windows resolves home through the OS, not USERPROFILE. Guard every
        // automatic selector before exercising any filesystem operation.
        let requested = PathBuf::from(std::env::var_os("AETHER_SKILL_TEST_ROOT").unwrap());
        let root = profile::configure_storage_root(&requested).unwrap();
        assert_eq!(storage::config_dir(), root);
        assert!(
            profile::configure_storage_root(&requested).is_err(),
            "configuration must be startup-only"
        );
        if matches!(case, "catalog" | "writes") {
            let defaults: Vec<PathBuf> = skills::default_skill_paths()
                .iter()
                .map(PathBuf::from)
                .collect();
            assert_eq!(
                defaults,
                vec![root.join("skills")],
                "refuse to exercise an automatic home-library leak"
            );
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            match std::env::var(CASE).unwrap().as_str() {
                "defaults" => {
                    assert!(!root.join("skills").exists());
                    let paths: Vec<PathBuf> = skills::default_skill_paths()
                        .iter()
                        .map(PathBuf::from)
                        .collect();
                    assert_eq!(
                        paths,
                        vec![root.join("skills")],
                        "Rune must not discover global compatibility libraries"
                    );
                    assert!(
                        !root.join("skills").exists(),
                        "default path selection must not initialize the library"
                    );
                }
                "catalog" => {
                    let paths =
                        skills::effective_skill_paths("", &[], &ReviewConfig::default()).unwrap();
                    assert!(
                        paths.iter().all(|path| Path::new(path).starts_with(&root)
                            || path == &home.join(".gemini/config/skills").to_string_lossy()),
                        "refuse automatic global reads"
                    );
                    let catalog = skills::load_skills(&paths).await;
                    assert!(!catalog.is_empty());
                    for skill in &catalog {
                        assert!(
                            Path::new(&skill.path).starts_with(root.join("skills")),
                            "automatic global read: {}",
                            skill.path
                        );
                    }
                    assert!(skills::find_skill(&catalog, "legacy-sentinel").is_none());
                    let mut settings = storage::default_settings();
                    settings.skill_paths = vec![home
                        .join(".gemini/config/skills")
                        .to_string_lossy()
                        .into_owned()];
                    let paths = skills::effective_skill_paths(
                        "",
                        &settings.skill_paths,
                        &ReviewConfig::default(),
                    )
                    .unwrap();
                    assert!(
                        paths.iter().all(|path| Path::new(path).starts_with(&root)
                            || path == &home.join(".gemini/config/skills").to_string_lossy()),
                        "refuse automatic global reads"
                    );
                    let catalog = skills::load_skills(&paths).await;
                    let sentinel = skills::find_skill(&catalog, "legacy-sentinel")
                        .expect("explicit skill_paths remain opt-in");
                    assert!(Path::new(&sentinel.path).starts_with(&settings.skill_paths[0]));
                    assert!(skills::find_skill(&catalog, "broken-legacy").is_none());
                }
                "writes" => {
                    skills::init_skills_environment();
                    assert!(
                        root.join("skills/systematic-debugging/SKILL.md").is_file(),
                        "starters must be seeded only in the Rune profile"
                    );
                    let saved = skills::save_skill_markdown(
                        "rune-only",
                        "test",
                        "profile-only instructions",
                        false,
                    )
                    .await
                    .unwrap();
                    assert!(Path::new(&saved.path).starts_with(root.join("skills")));
                    assert!(root.join("skills/rune-only/SKILL.md").is_file());
                }
                "creation" => {
                    for value in [None, Some("  "), Some("custom-skills")] {
                        let config = ReviewConfig {
                            create_dir: value.map(str::to_owned),
                            ..Default::default()
                        };
                        let expected = root.join(if value == Some("custom-skills") {
                            "custom-skills"
                        } else {
                            "skills"
                        });
                        assert_eq!(skills::review_creation_root(&config).unwrap(), expected);
                    }
                    // Explicit destinations retain their preexisting absolute/tilde semantics.
                    for (value, expected) in [
                        (
                            "~/recipes".to_string(),
                            dirs::home_dir().unwrap().join("recipes"),
                        ),
                        (
                            home.join("explicit").to_string_lossy().into_owned(),
                            home.join("explicit"),
                        ),
                    ] {
                        let config = ReviewConfig {
                            create_dir: Some(value),
                            ..Default::default()
                        };
                        assert_eq!(skills::review_creation_root(&config).unwrap(), expected);
                    }
                    let skill = skills::review_creation_root(&ReviewConfig::default())
                        .unwrap()
                        .join("created");
                    fs::create_dir_all(&skill).unwrap();
                    fs::write(
                        skill.join("SKILL.md"),
                        "---\nname: created\ndescription: profile\n---\ncreated in Rune\n",
                    )
                    .unwrap();
                    assert!(root.join("skills/created/SKILL.md").is_file());
                }
                "pending" => {
                    let pending = review_pending::pending_root().unwrap();
                    assert_eq!(pending, root.join("pending/skills"));
                    let skill = root.join("skills/pending-skill");
                    let target = skill.join("SKILL.md");
                    let proposed =
                        "---\nname: pending-skill\ndescription: profile\n---\nprofile proposal\n";
                    let id = review_pending::stage(
                        &pending,
                        review_pending::PendingWrite {
                            id: String::new(),
                            session_id: "session".into(),
                            run_id: "run".into(),
                            skill_name: "pending-skill".into(),
                            target_path: target.to_string_lossy().into_owned(),
                            skill_root: skill.to_string_lossy().into_owned(),
                            original: None,
                            proposed: Some(proposed.into()),
                        },
                    )
                    .await
                    .unwrap();
                    assert!(root.join(format!("pending/skills/{id}.json")).is_file());
                    assert!(root
                        .join(format!("pending/skills/.authority/{id}.json"))
                        .is_file());
                    assert_eq!(
                        review_pending::list_pending(&pending, "session")
                            .await
                            .unwrap()
                            .len(),
                        1
                    );
                    review_pending::approve_pending(&pending, &id, "session")
                        .await
                        .unwrap();
                    assert_eq!(fs::read_to_string(target).unwrap(), proposed);
                    assert!(review_pending::list_pending(&pending, "session")
                        .await
                        .unwrap()
                        .is_empty());
                }
                other => panic!("unknown case {other}"),
            }
        });
        for legacy in legacy_roots(&home) {
            assert_eq!(
                fs::read_to_string(legacy.join("broken-legacy/SKILL.md")).unwrap(),
                BROKEN
            );
            assert_eq!(
                fs::read_dir(legacy).unwrap().count(),
                2,
                "global skill library must remain untouched"
            );
        }
        assert!(!home.join(".aether-agent/pending").exists());
        return;
    }
    let base = std::env::temp_dir().join(format!("aether-skill-boundary-{}", uuid::Uuid::new_v4()));
    let home = base.join("fake-home");
    for legacy in legacy_roots(&home) {
        fs::create_dir_all(legacy.join("broken-legacy")).unwrap();
        fs::write(legacy.join("broken-legacy/SKILL.md"), BROKEN).unwrap();
        fs::create_dir_all(legacy.join("legacy-sentinel")).unwrap();
        fs::write(
            legacy.join("legacy-sentinel/SKILL.md"),
            "---\nname: legacy-sentinel\ndescription: explicit only\n---\nlegacy sentinel\n",
        )
        .unwrap();
    }
    let home = fs::canonicalize(&home).unwrap();
    let name = format!("shared_profile_{case}");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture"])
        .env(CHILD, "1")
        .env(CASE, case)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("AETHER_SKILL_TEST_ROOT", base.join("rune"))
        .output()
        .unwrap();
    if case == "catalog" && output.status.success() {
        assert_eq!(
            String::from_utf8_lossy(&output.stderr)
                .matches("Could not load skill")
                .count(),
            1,
            "only the explicitly configured malformed YAML may be read"
        );
    }
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn shared_profile_defaults() {
    run_child("defaults");
}
#[test]
fn shared_profile_catalog() {
    run_child("catalog");
}
#[test]
fn shared_profile_writes() {
    run_child("writes");
}
#[test]
fn shared_profile_creation() {
    run_child("creation");
}
#[test]
fn shared_profile_pending() {
    run_child("pending");
}
