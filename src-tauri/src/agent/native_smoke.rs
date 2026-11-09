//! Opt-in engineer harness. Never enabled by an environment variable or in default builds.
//! Storage/restart checks are native, not UI checks; the CDP script separately tests IPC.
use super::{
    storage,
    types::{CheckpointDelta, SessionData},
};
use base64::Engine;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Instant,
};

static PROFILE: OnceLock<PathBuf> = OnceLock::new();
const ID: &str = "diagnostic-smoke";
const COUNT: usize = 1000;
const IMAGE_BYTES: usize = 2 * 1024 * 1024;
pub fn profile() -> Option<&'static PathBuf> {
    PROFILE.get()
}

fn checked_profile(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Diagnostic profile must be absolute".into());
    }
    let root = path.canonicalize().map_err(|e| e.to_string())?;
    let temp = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    // Restrict to dedicated temp children, even when a forged sentinel is supplied.
    if root == temp || !root.starts_with(&temp) {
        return Err("Diagnostic profile must be a dedicated temp child".into());
    }
    if let Some(home) = dirs::home_dir() {
        let production = home.join(".aether-agent");
        let production = production.canonicalize().unwrap_or(production);
        if root.starts_with(&production) || production.starts_with(&root) {
            return Err("Production profile is forbidden".into());
        }
    }
    let sentinel: Value = serde_json::from_slice(
        &std::fs::read(root.join("aether-smoke.json"))
            .map_err(|_| "Missing aether-smoke.json sentinel")?,
    )
    .map_err(|e| e.to_string())?;
    if sentinel != json!({"purpose":"aether-native-smoke","version":1}) {
        return Err("Invalid diagnostic sentinel".into());
    }
    // Reject junctions/symlinks inside an existing profile before selecting it.
    fn check_tree(path: &Path) -> Result<(), String> {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let metadata = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("Reparse points forbidden in diagnostic profiles".into());
                }
            }
            if metadata.file_type().is_symlink() {
                return Err("Symlinks forbidden in diagnostic profiles".into());
            }
            if metadata.is_dir() {
                check_tree(&entry.path())?;
            }
        }
        Ok(())
    }
    check_tree(&root)?;
    Ok(root)
}

/// Must run before load_workspace_sync, skills initialization, or Tauri construction.
pub fn enter() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return Ok(());
    }
    if !args.iter().any(|s| {
        s.starts_with("--diagnostic") || s.starts_with("--smoke-test") || s == "--data-dir"
    }) {
        return Ok(());
    }
    let mut root = None;
    let mut smoke = false;
    let mut phase = "all".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--diagnostic-smoke" => smoke = true,
            "--diagnostic-profile" | "--data-dir" | "--smoke-test" => {
                let flag = &args[i];
                i += 1;
                if root.is_some() {
                    return Err("Specify exactly one profile path".into());
                }
                root = Some(PathBuf::from(args.get(i).ok_or("Missing profile path")?));
                if flag == "--smoke-test" {
                    smoke = true;
                }
            }
            "--smoke-test-phase" => {
                i += 1;
                phase = args.get(i).ok_or("Missing phase")?.clone();
            }
            _ => return Err("Unknown diagnostic argument".into()),
        }
        i += 1;
    }
    if !matches!(
        phase.as_str(),
        "seed" | "crash-seed" | "recover" | "verify" | "all"
    ) {
        return Err("Invalid smoke phase".into());
    }
    let root = checked_profile(&root.ok_or("Explicit diagnostic profile path required")?)?;
    PROFILE
        .set(root.clone())
        .map_err(|_| "Profile already initialized")?;
    if smoke {
        let result = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(run(&phase));
        let report = match &result {
            Ok(value) => value.clone(),
            Err(error) => json!({"ok":false,"phase":phase,"error":error}),
        };
        std::fs::write(
            root.join(format!("native-{phase}.json")),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        println!("{report}");
        if result.is_ok() && phase == "crash-seed" {
            // The parent waits for this marker then kills this process. No in-memory recovery.
            std::fs::write(root.join("crash-ready"), b"durable checkpoint")
                .map_err(|e| e.to_string())?;
            loop {
                std::thread::park();
            }
        }
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    Ok(())
}

fn fixture() -> Result<SessionData, String> {
    let mut messages: Vec<Value> = (0..COUNT).map(|i| json!({"id":format!("message-{i}"),"role":if i == COUNT-1 {"assistant"} else {"user"},"content":format!("Native smoke message {i}"),"timestamp":i as i64})).collect();
    // A valid PNG signature plus a 2 MiB opaque payload exercises persistence, not image decoding.
    let mut image = vec![42u8; IMAGE_BYTES];
    image[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    messages[0]["imageInputs"] = json!([{"sourcePath":"diagnostic-image.png","mediaType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(image)}]);
    messages[COUNT - 1]["runId"] = "smoke-run".into();
    messages[COUNT - 1]["isStreaming"] = true.into();
    serde_json::from_value(json!({"session":{"id":ID,"title":"Native diagnostic smoke","createdAt":1,"updatedAt":1,"messageCount":COUNT},"messages":messages,"artifacts":[]})).map_err(|e| e.to_string())
}
fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
async fn run(phase: &str) -> Result<Value, String> {
    let start = Instant::now();
    let mut timings = json!({});
    if matches!(phase, "seed" | "crash-seed" | "all") {
        ensure(
            !storage::config_dir()
                .join("sessions")
                .join(format!("{ID}.json"))
                .exists(),
            "Seed profile already contains fixture; use a fresh profile",
        )?;
        let data = fixture()?;
        let t = Instant::now();
        storage::save_session(data.clone()).await?;
        timings["save_1000_messages_ms"] = t.elapsed().as_secs_f64().mul_add(1000.0, 0.0).into();
        let page = storage::load_session_page(ID, None, 40, None).await?;
        let mut message = data.messages[COUNT - 1].clone();
        message.content.push_str(" durable checkpoint progress");
        let t = Instant::now();
        storage::checkpoint_session_delta(
            ID,
            CheckpointDelta {
                run_id: "smoke-run".into(),
                base_revision: page.window.revision,
                sequence: 1,
                messages: vec![message],
                artifacts: vec![],
            },
        )
        .await?;
        timings["checkpoint_ms"] = (t.elapsed().as_secs_f64() * 1000.0).into();
        ensure(
            storage::config_dir()
                .join("sessions")
                .join(format!("{ID}.checkpoint"))
                .exists(),
            "Durable checkpoint absent",
        )?;
    }
    if matches!(phase, "recover" | "verify" | "all") {
        let t = Instant::now();
        let page = storage::load_session_page(ID, None, 40, None).await?;
        timings["first_process_page_ms"] = (t.elapsed().as_secs_f64() * 1000.0).into();
        ensure(
            page.window.total == COUNT && page.data.messages.len() == 40,
            "Page bound or total mismatch",
        )?;
        ensure(
            page.data
                .messages
                .last()
                .unwrap()
                .content
                .ends_with("durable checkpoint progress"),
            "Checkpoint progress lost",
        )?;
        let t = Instant::now();
        for _ in 0..50 {
            let next = storage::load_session_page(ID, None, 40, Some(page.window.revision.clone()))
                .await?;
            ensure(
                next.data.messages.len() == 40 && next.window.total == COUNT,
                "Warm page mismatch",
            )?;
        }
        timings["warm_page_mean_ms"] = (t.elapsed().as_secs_f64() * 1000.0 / 50.0).into();
        let full = storage::load_session(ID).await?;
        ensure(full.messages.len() == COUNT, "Canonical history truncated")?;
        let image = &full.messages[0]
            .image_inputs
            .as_ref()
            .ok_or("Image missing")?[0];
        ensure(
            base64::engine::general_purpose::STANDARD
                .decode(&image.data)
                .map_err(|e| e.to_string())?
                .len()
                == IMAGE_BYTES,
            "Image bytes changed",
        )?;
        ensure(
            !storage::config_dir()
                .join("sessions")
                .join(format!("{ID}.checkpoint"))
                .exists(),
            "Recovery did not fold journal",
        )?;
    }
    Ok(
        json!({"ok":true,"phase":phase,"profile":storage::config_dir(),"messages":COUNT,"image_payload_bytes":IMAGE_BYTES,"timings":timings,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0,"measurement_scope":"Native storage only; first-process read is not cold disk; PNG-like opaque payload is not image decoding; no FPS/UI claim"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_requires_absolute_temp_child_and_sentinel() {
        assert!(checked_profile(Path::new("relative")).is_err());
        assert!(checked_profile(&std::env::temp_dir()).is_err());
        let root = std::env::temp_dir().join(format!("aether-profile-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        assert!(checked_profile(&root).is_err());
        std::fs::write(root.join("aether-smoke.json"), b"{}").unwrap();
        assert!(checked_profile(&root).is_err());
        std::fs::write(
            root.join("aether-smoke.json"),
            serde_json::to_vec(&json!({"purpose":"aether-native-smoke","version":1})).unwrap(),
        )
        .unwrap();
        assert!(checked_profile(&root).is_ok());
    }
}
