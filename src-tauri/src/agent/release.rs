//! Native-only release authority. No endpoint, public key, URL or installer path
//! is accepted from the webview. Generic updater JS permissions are not enabled.
use super::release_policy::{is_newer, validate_key, validate_url};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct ReleaseState {
    pending: tokio::sync::Mutex<Option<Update>>,
    restart_required: AtomicBool,
}

fn configuration() -> Result<(url::Url, &'static str), String> {
    let endpoint = env!("AETHER_UPDATE_ENDPOINT");
    let key = env!("AETHER_UPDATE_PUBLIC_KEY");
    if endpoint.is_empty() || key.is_empty() {
        return Err("Automatic updates not configured".into());
    }
    let endpoint = validate_url(endpoint)?;
    validate_key(key)?;
    Ok((endpoint, key))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    version: &'static str,
    build_id: &'static str,
    platform: &'static str,
    arch: &'static str,
    diagnostic_logs_path: String,
    signing_status: &'static str,
    updates_configured: bool,
    update_status: String,
}

#[tauri::command]
pub fn get_release_info() -> Result<ReleaseInfo, String> {
    // This is the real bounded background-task log directory, not a fabricated app log.
    let logs = super::storage::config_dir().join("tasks");
    std::fs::create_dir_all(&logs).map_err(|_| "Diagnostic log directory unavailable")?;
    let config = configuration();
    Ok(ReleaseInfo {
        version: env!("CARGO_PKG_VERSION"),
        build_id: env!("AETHER_BUILD_ID"),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        diagnostic_logs_path: logs.to_string_lossy().into_owned(),
        signing_status: "Unknown (not verified at runtime)",
        updates_configured: config.is_ok(),
        update_status: match config {
            Ok(_) => "Signed updates configured; availability has not been checked".into(),
            Err(error) => error,
        },
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    configured: bool,
    status: String,
    version: Option<String>,
    notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    phase: &'static str,
    downloaded: u64,
    total: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    status: &'static str,
    restart_required: bool,
}

fn report_progress(app: &tauri::AppHandle, phase: &'static str, downloaded: u64, total: Option<u64>) {
    let _ = app.emit("release:update-progress", UpdateProgress { phase, downloaded, total });
}

#[tauri::command]
pub async fn check_release_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, ReleaseState>,
) -> Result<UpdateResult, String> {
    let mut pending = state.pending.lock().await;
    *pending = None;
    let (endpoint, key) = match configuration() {
        Ok(config) => config,
        Err(status) => {
            return Ok(UpdateResult {
                configured: false,
                status,
                version: None,
                notes: None,
            })
        }
    };
    let updater = app
        .updater_builder()
        .pubkey(key)
        .endpoints(vec![endpoint.clone()])
        .map_err(|_| "Invalid updater endpoint")?
        .version_comparator(|current, release| {
            is_newer(&current.to_string(), &release.version.to_string())
        })
        .build()
        .map_err(|_| "Could not initialize signed updater")?;
    // The plugin's HTTP check buffers metadata without a size limit. Reuse the bounded
    // public transport, then retain official target/comparator and signed-version policy.
    let raw_json = super::release_download::download_manifest(&endpoint).await?;
    let update = updater
        .restore_update(raw_json)
        .map_err(|_| "Signed update check failed; check connectivity and release configuration")?;
    match update {
        Some(update) => {
            validate_url(update.download_url.as_str())?;
            if !is_newer(env!("CARGO_PKG_VERSION"), &update.version) {
                return Err("Update is not newer than the running version".into());
            }
            let version = update.version.clone();
            let notes = update.body.as_ref().map(|body| body.chars().take(12_000).collect());
            *pending = Some(update);
            Ok(UpdateResult {
                configured: true,
                status: format!(
                    "Update {version} available; signature will be verified before installation"
                ),
                version: Some(version),
                notes,
            })
        }
        None => Ok(UpdateResult {
            configured: true,
            status: "No newer release available".into(),
            version: None,
            notes: None,
        }),
    }
}

#[tauri::command]
pub async fn install_release_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, ReleaseState>,
    app_state: tauri::State<'_, crate::AppState>,
    expected_version: String,
    confirmed: bool,
) -> Result<InstallResult, String> {
    if !confirmed {
        return Err("Explicit installation confirmation is required".into());
    }
    configuration()?;
    let permit = app_state.runner.acquire_update_permit()?;
    if super::tasks::get_task_manager().has_running_tasks().await {
        return Err("Stop all background tasks before installing an update".into());
    }
    let mut pending = state.pending.lock().await;
    let update = pending
        .as_ref()
        .ok_or("Check for updates before installing")?;
    if update.version != expected_version || !is_newer(env!("CARGO_PKG_VERSION"), &update.version) {
        return Err("Update selection changed; check again".into());
    }
    validate_url(update.download_url.as_str())?;
    let update = pending.take().ok_or("Update unavailable")?;
    let mut last_report = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let bytes = super::release_download::download(&update.download_url, |downloaded, total| {
        if downloaded == 0 || Some(downloaded) == total || last_report.elapsed() >= std::time::Duration::from_millis(100) {
            report_progress(&app, "downloading", downloaded, total);
            last_report = std::time::Instant::now();
        }
    }).await?;
    report_progress(&app, "verifying", bytes.len() as u64, Some(bytes.len() as u64));
    // Plugin memverifikasi signature dan signed-version sebelum installer boleh dijalankan.
    update.verify(&bytes).map_err(|_| "Signature/version verification failed; installer was not started")?;
    report_progress(&app, "installing", bytes.len() as u64, Some(bytes.len() as u64));
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        update.install(bytes).map_err(|_| "Signed installer could not be started".to_string())
    }).await.map_err(|_| "Installer task failed")??;
    let restart_required = !cfg!(target_os = "windows");
    state.restart_required.store(restart_required, Ordering::Release);
    Ok(InstallResult {
        status: if restart_required { "Signed update installed. Restart AetherAgent to finish updating." }
            else { "Signed installer started. Complete the installer to finish updating." },
        restart_required,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn update_check_bounds_manifest_before_official_restore() {
        let source = include_str!("release.rs");
        let check = source.split("pub async fn check_release_update(").nth(1).unwrap()
            .split("pub async fn install_release_update(").next().unwrap();
        assert!(!check.contains(".check()"), "plugin.check buffers unbounded metadata");
        let fetch = check.find("release_download::download_manifest(&endpoint)")
            .expect("native metadata transport must enforce its own cap");
        let restore = check.find(".restore_update(raw_json)")
            .expect("the official updater must select the update");
        assert!(fetch < restore);
    }
}

#[tauri::command]
pub async fn restart_after_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, ReleaseState>,
    app_state: tauri::State<'_, crate::AppState>,
    confirmed: bool,
) -> Result<(), String> {
    if !confirmed || !state.restart_required.load(Ordering::Acquire) {
        return Err("A verified installed update and explicit restart confirmation are required".into());
    }
    let _permit = app_state.runner.acquire_update_permit()?;
    if super::tasks::get_task_manager().has_running_tasks().await {
        return Err("Stop all background tasks before restarting".into());
    }
    app.restart();
}
