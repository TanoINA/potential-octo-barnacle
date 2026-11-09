//! Standalone Rune updater: no Tauri runtime, webview, IPC URLs, or runtime keys.
//! Build with AETHER_RUNE_UPDATE_ENDPOINT and AETHER_RUNE_UPDATE_PUBKEY (base64
//! minisign public-key file). Missing configuration is a disabled, successful check.
//! Only explicit checks fetch manifests; only confirmed, single-use tickets fetch
//! installers. Windows opens a verified installer wizard; Linux stages an AppImage
//! for manual replacement, never overwriting the running application.
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, future::Future, io::{Read, Write}, path::{Path, PathBuf}, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use url::Url;

pub const MAX_INSTALLER_BYTES: usize = 128 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const DISABLED: &str = "Automatic updates not configured for this Rune build";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub configured: bool,
    pub status: String,
    pub version: Option<String>,
    pub ticket_id: Option<String>,
    pub notes: Option<String>,
    pub action: Option<&'static str>,
}
impl UpdateCheck {
    pub fn disabled(status: impl Into<String>) -> Self {
        Self { configured: false, status: status.into(), version: None, ticket_id: None, notes: None, action: None }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOutcome {
    pub version: String,
    pub status: String,
    pub installer_path: String,
    pub launched: bool,
    pub manual_install: bool,
}

struct Configuration { endpoint: Url, key: PublicKey, current: String, platform: String }
#[derive(Clone, Copy)]
enum Format { Exe, Msi, AppImage }
impl Format {
    fn basename(self) -> &'static str {
        match self { Self::Exe => "installer.exe", Self::Msi => "installer.msi", Self::AppImage => "installer.AppImage" }
    }
}
struct Pending { version: String, ticket: String, url: Url, signature: Signature, format: Format }
struct Response { status: u16, bytes: Vec<u8> }
type Transfer<'a> = Pin<Box<dyn Future<Output = Result<Response, String>> + Send + 'a>>;
trait Transport: Send + Sync { fn get(&self, url: Url, max: usize, json: bool) -> Transfer<'_>; }
struct HttpTransport;
impl Transport for HttpTransport {
    fn get(&self, url: Url, max: usize, json: bool) -> Transfer<'_> {
        Box::pin(async move {
            validate_url(url.as_str())?;
            let client = reqwest::Client::builder().https_only(true).no_proxy()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if redirect_allowed(attempt.url(), attempt.previous().len()).is_ok() {
                        attempt.follow()
                    } else { attempt.error("Unsafe or excessive updater redirect") }
                }))
                .build().map_err(|_| "Could not initialize HTTPS updater")?;
            let response = client.get(url)
                .header(reqwest::header::ACCEPT, if json { "application/json" } else { "application/octet-stream" })
                .header(reqwest::header::USER_AGENT, "Aether-Rune-Updater")
                .send().await.map_err(|_| "Update request failed (network, timeout or redirect policy)")?;
            let status = response.status().as_u16();
            if status != 200 && status != 204 { return Err("Update server returned an unsuccessful status".into()); }
            if response.content_length().is_some_and(|n| n > max as u64) { return Err("Update response exceeds size limit".into()); }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| "Update response could not be read")?;
                append_bounded(&mut bytes, &chunk, max)?;
            }
            Ok(Response { status, bytes })
        })
    }
}
fn append_bounded(bytes: &mut Vec<u8>, chunk: &[u8], max: usize) -> Result<(), String> {
    if chunk.len() > max.saturating_sub(bytes.len()) { return Err("Update response exceeds size limit".into()); }
    bytes.extend_from_slice(chunk); Ok(())
}
/// Public HTTPS DNS names only. Query strings are allowed for signed CDN redirects,
/// but never included in errors, logs, UI replies, filenames, or saved manifests.
fn validate_url(input: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|_| "Invalid update URL")?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("Update URL must use HTTPS without credentials or fragments".into());
    }
    match url.host() {
        Some(url::Host::Domain(host)) if host.contains('.') && !host.ends_with('.')
            && ![".localhost", ".local", ".internal", ".test", ".invalid"].iter().any(|s| host.ends_with(s)) => Ok(url),
        _ => Err("Update URL must use a public DNS hostname (no literal/local IPs)".into()),
    }
}
fn redirect_allowed(url: &Url, previous: usize) -> Result<(), String> {
    if previous > 5 { return Err("Too many updater redirects".into()); }
    validate_url(url.as_str()).map(|_| ())
}
fn decode(input: &str) -> Result<String, String> {
    String::from_utf8(STANDARD.decode(input.trim()).map_err(|_| "Invalid updater signature/key encoding")?)
        .map_err(|_| "Invalid updater signature/key text".into())
}
fn signed_version(signature: &Signature, expected: &str) -> Result<(), String> {
    let versions: Vec<_> = signature.trusted_comment().split_whitespace().filter_map(|t| t.strip_prefix("version:")).collect();
    if versions != [expected] { return Err("Expected exactly one authenticated, exact product version".into()); }
    Ok(())
}
fn authenticate(key: &PublicKey, bytes: &[u8], signature: &Signature, version: &str) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_INSTALLER_BYTES { return Err("Invalid installer size".into()); }
    // false forbids legacy signatures. This also authenticates the trusted comment;
    // a version token in an unauthenticated comment is never an authority.
    key.verify(bytes, signature, false).map_err(|_| "Installer signature verification failed")?;
    signed_version(signature, version)
}
fn configuration(endpoint: &str, key: &str, current: &str, platform: &str) -> Result<Configuration, String> {
    if endpoint.is_empty() || key.is_empty() { return Err(DISABLED.into()); }
    if !matches!(platform, "windows-x86_64" | "linux-x86_64") { return Err("Signed Rune updates are unavailable for this platform".into()); }
    semver::Version::parse(current).map_err(|_| "Invalid current product version")?;
    crate::agent::release_policy::validate_key(key)?;
    let key = PublicKey::decode(&decode(key)?).map_err(|_| "Invalid updater public key")?;
    let (target, arch) = platform.split_once('-').ok_or("Unsupported update platform")?;
    let endpoint = endpoint.replace("{{target}}", target).replace("{{arch}}", arch).replace("{{current_version}}", current);
    if endpoint.contains("{{") || endpoint.contains("}}") { return Err("Unknown update endpoint token".into()); }
    Ok(Configuration { endpoint: validate_url(&endpoint)?, key, current: current.into(), platform: platform.into() })
}
#[derive(Deserialize)]
struct Artifact { url: String, signature: String, format: Option<String> }
#[derive(Deserialize)]
struct Manifest {
    version: String, notes: Option<String>, url: Option<String>, signature: Option<String>, format: Option<String>,
    platforms: Option<HashMap<String, Artifact>>,
}
fn installer_format(platform: &str, url: &Url, explicit: Option<&str>) -> Result<Format, String> {
    let inferred = url.path().rsplit('.').next().unwrap_or("");
    match (platform, explicit.unwrap_or(inferred).to_ascii_lowercase().as_str()) {
        ("windows-x86_64", "exe") => Ok(Format::Exe),
        ("windows-x86_64", "msi") => Ok(Format::Msi),
        ("linux-x86_64", "appimage") => Ok(Format::AppImage),
        _ => Err("Unsupported installer format; use exe/msi on Windows or AppImage on Linux".into()),
    }
}

pub struct Updater {
    config: Result<Configuration, String>,
    profile: PathBuf,
    pending: Mutex<Option<Pending>>,
    operation: Mutex<()>,
    transport: Arc<dyn Transport>,
    handoff: fn(&Path, Format) -> Result<bool, String>,
}
impl Updater {
    /// Only compile-baked Rune-specific values are read. Never reads runtime env
    /// for endpoints/keys, nor falls back to the shared Tauri release authority.
    pub fn compiled() -> Option<Self> {
        let endpoint = option_env!("AETHER_RUNE_UPDATE_ENDPOINT").unwrap_or("");
        let key = option_env!("AETHER_RUNE_UPDATE_PUBKEY").unwrap_or("");
        if endpoint.is_empty() && key.is_empty() { return None; }
        Some(Self {
            config: configuration(endpoint, key, env!("CARGO_PKG_VERSION"), &format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)),
            profile: crate::agent::storage::config_dir(), pending: Mutex::new(None), operation: Mutex::new(()),
            transport: Arc::new(HttpTransport), handoff: native_handoff,
        })
    }
    pub fn status(&self) -> Result<(), String> { self.config.as_ref().map(|_| ()).map_err(Clone::clone) }
    pub fn disabled() -> UpdateCheck { UpdateCheck::disabled(DISABLED) }
    pub async fn check(&self) -> Result<UpdateCheck, String> {
        let _operation = self.operation.lock().await;
        *self.pending.lock().await = None;
        let config = match &self.config { Ok(c) => c, Err(status) => return Ok(UpdateCheck::disabled(status.clone())) };
        let response = self.transport.get(config.endpoint.clone(), MAX_MANIFEST_BYTES, true).await?;
        if response.bytes.len() > MAX_MANIFEST_BYTES { return Err("Update manifest exceeds size limit".into()); }
        if response.status == 204 { return Ok(no_update()); }
        if response.status != 200 { return Err("Update manifest request failed".into()); }
        let manifest: Manifest = serde_json::from_slice(&response.bytes).map_err(|_| "Invalid update manifest")?;
        if manifest.version.len() > 128 || semver::Version::parse(&manifest.version).is_err() { return Err("Invalid update version".into()); }
        if !crate::agent::release_policy::is_newer(&config.current, &manifest.version) { return Ok(no_update()); }
        let artifact = match manifest.platforms {
            Some(mut platforms) => platforms.remove(&config.platform).ok_or("Release has no installer for this platform")?,
            None => Artifact { url: manifest.url.ok_or("Release has no installer URL")?, signature: manifest.signature.ok_or("Release has no signature")?, format: manifest.format },
        };
        if artifact.signature.len() > 16 * 1024 { return Err("Invalid updater signature size".into()); }
        let url = validate_url(&artifact.url)?;
        let format = installer_format(&config.platform, &url, artifact.format.as_deref())?;
        let signature = Signature::decode(&decode(&artifact.signature)?).map_err(|_| "Invalid updater signature")?;
        // Structural version check only; authentication happens after downloading.
        signed_version(&signature, &manifest.version)?;
        let ticket = uuid::Uuid::new_v4().to_string();
        let result = UpdateCheck { configured: true, status: format!("Update {} available; signature verification required before handoff", manifest.version),
            version: Some(manifest.version.clone()), ticket_id: Some(ticket.clone()), notes: manifest.notes.map(|s| s.chars().take(16 * 1024).collect()),
            action: Some(if matches!(format, Format::AppImage) { "stage" } else { "install" }) };
        *self.pending.lock().await = Some(Pending { version: manifest.version, ticket, url, signature, format });
        Ok(result)
    }
    pub async fn install(&self, expected_version: &str, ticket_id: &str, confirmed: bool) -> Result<UpdateOutcome, String> {
        if !confirmed { return Err("Explicit update confirmation is required".into()); }
        let _operation = self.operation.lock().await;
        let config = self.config.as_ref().map_err(Clone::clone)?;
        let mut pending = self.pending.lock().await;
        let selection = pending.as_ref().ok_or("Check for updates before installing")?;
        if selection.ticket != ticket_id || selection.version != expected_version || !crate::agent::release_policy::is_newer(&config.current, expected_version) {
            return Err("Update selection changed; check again".into());
        }
        let selection = pending.take().ok_or("Update unavailable")?;
        drop(pending);
        // Ticket is consumed even on failure, preventing duplicate launch/replay.
        let response = self.transport.get(selection.url, MAX_INSTALLER_BYTES, false).await?;
        if response.status != 200 { return Err("Installer download failed".into()); }
        authenticate(&config.key, &response.bytes, &selection.signature, &selection.version)?;
        let profile = self.profile.clone();
        let key = config.key.clone();
        let handoff = self.handoff;
        tokio::task::spawn_blocking(move || {
            let path = stage(&profile, selection.format, &response.bytes)?;
            // Re-open without write/delete sharing on Windows and reauthenticate
            // the on-disk object immediately before launch. Keep it open through spawn.
            no_reparse(&path)?;
            let mut options = std::fs::OpenOptions::new(); options.read(true);
            #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.share_mode(1).custom_flags(0x00200000); }
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.custom_flags(libc::O_NOFOLLOW); }
            let mut file = options.open(&path).map_err(|_| "Verified installer could not be reopened")?;
            if !file.metadata().map_err(|_| "Installer metadata unavailable")?.is_file() { return Err("Installer is not a regular file".into()); }
            let mut bytes = Vec::new();
            (&mut file).take(MAX_INSTALLER_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|_| "Verified installer could not be read")?;
            authenticate(&key, &bytes, &selection.signature, &selection.version)?;
            let launched = handoff(&path, selection.format)?;
            Ok(UpdateOutcome { version: selection.version, status: if launched { "Verified installer started; installation completion is not yet known. Close Rune when the installer asks." } else { "Verified installer staged only. Close Rune and manually replace/relaunch the AppImage; no installed application was modified." }.into(),
                installer_path: path.to_string_lossy().into_owned(), launched, manual_install: !launched })
        }).await.map_err(|_| "Update staging task failed")?
    }
}
fn no_update() -> UpdateCheck { UpdateCheck { configured: true, status: "No newer release available".into(), version: None, ticket_id: None, notes: None, action: None } }
fn no_reparse(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(m) => {
                let linked = m.file_type().is_symlink();
                #[cfg(windows)] let linked = { use std::os::windows::fs::MetadataExt; linked || m.file_attributes() & 0x400 != 0 };
                if linked { return Err("Update staging refuses symlink/reparse paths".into()); }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err("Update staging directory could not be inspected".into()),
        }
    }
    Ok(())
}
fn private_directory(path: &Path, allow_existing: bool) -> Result<(), String> {
    no_reparse(path)?;
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
    match builder.create(path) {
        Ok(()) => {}, Err(e) if allow_existing && e.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(_) => return Err("Could not create isolated update directory".into()),
    }
    no_reparse(path)?;
    if !path.is_dir() { return Err("Update staging path is not a directory".into()); }
    Ok(())
}
fn stage(profile: &Path, format: Format, bytes: &[u8]) -> Result<PathBuf, String> {
    if !profile.is_absolute() || !profile.is_dir() { return Err("Validated update profile is unavailable".into()); }
    no_reparse(profile)?;
    let updates = profile.join("updates"); private_directory(&updates, true)?;
    let directory = updates.join(uuid::Uuid::new_v4().to_string()); private_directory(&directory, false)?;
    let temporary = directory.join("installer.partial");
    let destination = directory.join(format.basename());
    let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600).custom_flags(libc::O_NOFOLLOW); }
    let mut file = options.open(&temporary).map_err(|_| "Could not create update staging file")?;
    file.write_all(bytes).map_err(|_| "Could not write verified installer")?;
    #[cfg(unix)] if matches!(format, Format::AppImage) { use std::os::unix::fs::PermissionsExt; file.set_permissions(std::fs::Permissions::from_mode(0o700)).map_err(|_| "Could not set AppImage permissions")?; }
    file.sync_all().map_err(|_| "Could not sync verified installer")?;
    drop(file); no_reparse(&directory)?;
    // Publishing via hard_link is atomic and fails if destination exists; unlike
    // rename on POSIX it never replaces another file. Both names are in our UUID dir.
    std::fs::hard_link(&temporary, &destination).map_err(|_| "Could not atomically publish verified installer")?;
    std::fs::remove_file(&temporary).map_err(|_| "Could not finalize verified installer")?;
    #[cfg(unix)] std::fs::File::open(&directory).and_then(|d| d.sync_all()).map_err(|_| "Could not sync update staging directory")?;
    no_reparse(&destination)?;
    Ok(destination)
}
#[cfg(windows)]
fn native_handoff(path: &Path, format: Format) -> Result<bool, String> {
    let mut command = match format {
        Format::Exe => std::process::Command::new(path),
        Format::Msi => {
            // Resolve the OS binary without PATH search or a command interpreter.
            #[link(name = "kernel32")]
            unsafe extern "system" { fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32; }
            let mut buffer = [0u16; 32768];
            let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
            if length == 0 || length >= buffer.len() { return Err("Windows installer service unavailable".into()); }
            let mut c = std::process::Command::new(PathBuf::from(String::from_utf16_lossy(&buffer[..length])).join("msiexec.exe"));
            c.arg("/i").arg(path); c
        }
        Format::AppImage => return Err("AppImage cannot be launched as a Windows installer".into()),
    };
    command.current_dir(path.parent().ok_or("Installer directory unavailable")?);
    let mut child = command.spawn().map_err(|_| "Verified installer could not be started; it remains staged")?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(true)
}
#[cfg(not(windows))]
fn native_handoff(_: &Path, format: Format) -> Result<bool, String> {
    if !matches!(format, Format::AppImage) { return Err("Automatic installer launch is unsupported on this platform".into()); }
    Ok(false)
}
#[cfg(test)]
#[path = "updater_tests.rs"]
mod tests;