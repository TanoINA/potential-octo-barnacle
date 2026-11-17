use super::*;
use std::collections::VecDeque;
use std::sync::Mutex as StdMutex;

const KEY: &str = include_str!("../tests/fixtures/updater/public-key.txt");
const SIGNATURE: &str = include_str!("../tests/fixtures/updater/signature.txt");
const INSTALLER: &[u8] = include_bytes!("../tests/fixtures/updater/installer.exe");
const ENDPOINT: &str = "https://updates.example.com/latest.json";
const ARTIFACT: &str = "https://cdn.example.com/release.exe?token=private";

#[derive(Default)]
struct MockTransport {
    responses: StdMutex<VecDeque<Result<Response, String>>>,
    requests: StdMutex<Vec<(Url, usize, bool)>>,
}
impl MockTransport {
    fn push(&self, status: u16, bytes: Vec<u8>) {
        self.responses
            .lock()
            .unwrap()
            .push_back(Ok(Response { status, bytes }));
    }
    fn manifest(&self, value: serde_json::Value) {
        self.push(200, serde_json::to_vec(&value).unwrap());
    }
}
impl Transport for MockTransport {
    fn get(&self, url: Url, max: usize, json: bool) -> Transfer<'_> {
        self.requests.lock().unwrap().push((url, max, json));
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected download");
        Box::pin(async move { response })
    }
}

struct Profile(PathBuf);
impl Profile {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("aether-updater-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn verified_handoff(path: &Path, _: Format) -> Result<bool, String> {
    assert_eq!(std::fs::read(path).unwrap(), INSTALLER);
    Ok(true)
}
fn staged_handoff(path: &Path, format: Format) -> Result<bool, String> {
    assert!(matches!(format, Format::AppImage));
    assert_eq!(std::fs::read(path).unwrap(), INSTALLER);
    Ok(false)
}
fn updater(profile: &Path, transport: Arc<MockTransport>) -> Updater {
    Updater {
        config: configuration(ENDPOINT, KEY, "0.1.0", "windows-x86_64"),
        profile: profile.into(),
        pending: Mutex::new(None),
        operation: Mutex::new(()),
        transport,
        handoff: verified_handoff,
    }
}
fn manifest() -> serde_json::Value {
    serde_json::json!({ "version": "0.2.0", "url": ARTIFACT, "signature": SIGNATURE.trim() })
}
fn signature() -> Signature {
    Signature::decode(&decode(SIGNATURE).unwrap()).unwrap()
}

#[test]
fn urls_and_redirects_require_public_https_without_credentials_or_fragments() {
    for value in [
        "http://updates.example.com/u",
        "https://user:secret@updates.example.com/u",
        "https://updates.example.com/u#secret",
        "https://localhost/u",
        "https://127.0.0.1/u",
        "https://10.0.0.2/u",
        "https://[::1]/u",
        "https://[::ffff:127.0.0.1]/u",
        "https://host.localhost/u",
        "https://host.local/u",
        "https://host.internal/u",
        "https://host.test/u",
        "https://host.invalid/u",
        "https://example.com./u",
        "not a URL",
    ] {
        assert!(validate_url(value).is_err(), "accepted {value}");
        if let Ok(url) = Url::parse(value) {
            assert!(
                redirect_allowed(&url, 1).is_err(),
                "redirect accepted {value}"
            );
        }
    }
    let url = validate_url(ARTIFACT).unwrap();
    assert_eq!(url.query(), Some("token=private"));
    assert!(redirect_allowed(&url, 5).is_ok());
    assert!(redirect_allowed(&url, 6).is_err());
}

#[test]
fn bounded_download_rejects_overflow_without_appending() {
    let mut bytes = vec![1, 2];
    append_bounded(&mut bytes, &[3, 4], 4).unwrap();
    assert!(append_bounded(&mut bytes, &[5], 4).is_err());
    assert_eq!(bytes, [1, 2, 3, 4]);
    assert!(append_bounded(&mut bytes, &[5], 1).is_err());
}

#[test]
fn configuration_validates_authority_platform_version_and_endpoint_tokens() {
    let config = configuration(
        "https://updates.example.com/{{target}}/{{arch}}/{{current_version}}",
        KEY,
        "0.1.0",
        "linux-x86_64",
    )
    .unwrap();
    assert_eq!(
        config.endpoint.as_str(),
        "https://updates.example.com/linux/x86_64/0.1.0"
    );
    for (endpoint, key, current, platform) in [
        ("", KEY, "0.1.0", "windows-x86_64"),
        (ENDPOINT, "", "0.1.0", "windows-x86_64"),
        (ENDPOINT, "bad key", "0.1.0", "windows-x86_64"),
        (ENDPOINT, KEY, "invalid", "windows-x86_64"),
        (ENDPOINT, KEY, "0.1.0", "linux-aarch64"),
        (
            "https://updates.example.com/{{unknown}}",
            KEY,
            "0.1.0",
            "windows-x86_64",
        ),
        (
            "http://updates.example.com/u",
            KEY,
            "0.1.0",
            "windows-x86_64",
        ),
    ] {
        assert!(configuration(endpoint, key, current, platform).is_err());
    }
    assert!(decode("not base64!").is_err());
    assert!(decode(&STANDARD.encode([0xff])).is_err());
}

#[test]
fn installer_formats_are_platform_specific_and_ignore_query_extensions() {
    for (platform, url, explicit, basename) in [
        ("windows-x86_64", ARTIFACT, None, "installer.exe"),
        (
            "windows-x86_64",
            "https://cdn.example.com/update.MSI",
            None,
            "installer.msi",
        ),
        (
            "linux-x86_64",
            "https://cdn.example.com/update.AppImage",
            None,
            "installer.AppImage",
        ),
        (
            "windows-x86_64",
            "https://cdn.example.com/download",
            Some("EXE"),
            "installer.exe",
        ),
    ] {
        assert_eq!(
            installer_format(platform, &Url::parse(url).unwrap(), explicit)
                .unwrap()
                .basename(),
            basename
        );
    }
    for (platform, url, explicit) in [
        ("linux-x86_64", ARTIFACT, None),
        (
            "windows-x86_64",
            "https://cdn.example.com/update.AppImage",
            None,
        ),
        (
            "windows-x86_64",
            "https://cdn.example.com/download?file=update.exe",
            None,
        ),
        ("windows-x86_64", ARTIFACT, Some("zip")),
    ] {
        assert!(installer_format(platform, &Url::parse(url).unwrap(), explicit).is_err());
    }
}

#[test]
fn authenticates_fixture_bytes_and_exact_authenticated_version() {
    let key = PublicKey::decode(&decode(KEY).unwrap()).unwrap();
    authenticate(&key, INSTALLER, &signature(), "0.2.0").unwrap();
    assert!(authenticate(&key, INSTALLER, &signature(), "0.3.0").is_err());
    assert!(authenticate(&key, &[], &signature(), "0.2.0").is_err());
    let mut tampered = INSTALLER.to_vec();
    tampered[0] ^= 1;
    assert!(authenticate(&key, &tampered, &signature(), "0.2.0").is_err());

    let text = decode(SIGNATURE).unwrap();
    for replacement in [
        "version:0.3.0",
        "version:0.2.0 version:0.2.0",
        "product:0.2.0",
    ] {
        let changed = Signature::decode(&text.replace("version:0.2.0", replacement)).unwrap();
        assert!(signed_version(&changed, "0.2.0").is_err());
        assert!(authenticate(&key, INSTALLER, &changed, "0.2.0").is_err());
    }
    // A structurally matching version is not sufficient when its comment was forged.
    let forged = Signature::decode(&text.replace("version:0.2.0", "version:0.3.0")).unwrap();
    signed_version(&forged, "0.3.0").unwrap();
    assert!(authenticate(&key, INSTALLER, &forged, "0.3.0").is_err());
}

#[tokio::test]
async fn disabled_check_does_not_download_or_issue_a_ticket() {
    let transport = Arc::new(MockTransport::default());
    let mut updater = updater(Path::new("unused"), transport.clone());
    updater.config = Err(DISABLED.into());
    let check = updater.check().await.unwrap();
    assert!(!check.configured);
    assert_eq!(check.status, DISABLED);
    assert!(check.ticket_id.is_none());
    assert!(check.action.is_none());
    assert!(transport.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn checks_do_not_download_installers_and_bound_notes() {
    let transport = Arc::new(MockTransport::default());
    let updater = updater(Path::new("unused"), transport.clone());
    let mut value = manifest();
    value["notes"] = serde_json::json!("é".repeat(17 * 1024));
    transport.manifest(value);
    let check = updater.check().await.unwrap();
    assert!(check.configured);
    assert_eq!(check.version.as_deref(), Some("0.2.0"));
    uuid::Uuid::parse_str(check.ticket_id.as_deref().unwrap()).unwrap();
    assert_eq!(check.action, Some("install"));
    assert_eq!(check.notes.unwrap().chars().count(), 16 * 1024);
    assert!(!check.status.contains("private"));
    let requests = transport.requests.lock().unwrap();
    assert_eq!(
        requests.as_slice(),
        &[(Url::parse(ENDPOINT).unwrap(), MAX_MANIFEST_BYTES, true)]
    );
}

#[tokio::test]
async fn no_update_for_204_or_non_newer_semver() {
    let transport = Arc::new(MockTransport::default());
    let updater = updater(Path::new("unused"), transport.clone());
    transport.push(204, vec![]);
    for version in ["0.0.9", "0.1.0", "0.1.0+build", "0.1.0-rc.1"] {
        transport.manifest(serde_json::json!({ "version": version }));
    }
    for _ in 0..5 {
        let check = updater.check().await.unwrap();
        assert!(check.configured);
        assert!(check.version.is_none());
        assert!(check.ticket_id.is_none());
        assert!(updater.pending.lock().await.is_none());
    }
}

#[tokio::test]
async fn malformed_unsafe_or_oversized_manifests_never_issue_tickets() {
    let transport = Arc::new(MockTransport::default());
    let updater = updater(Path::new("unused"), transport.clone());
    let mut cases = vec![];
    for (field, value) in [
        ("version", serde_json::json!("invalid")),
        ("version", serde_json::json!("0.3.0")),
        (
            "url",
            serde_json::json!("https://127.0.0.1/update.exe?secret=private"),
        ),
        ("signature", serde_json::json!("invalid")),
        ("signature", serde_json::json!("x".repeat(16 * 1024 + 1))),
        ("format", serde_json::json!("zip")),
        ("platforms", serde_json::json!({})),
    ] {
        let mut value_manifest = manifest();
        value_manifest[field] = value;
        cases.push(serde_json::to_vec(&value_manifest).unwrap());
    }
    cases.push(b"not JSON".to_vec());
    cases.push(vec![b' '; MAX_MANIFEST_BYTES + 1]);
    for bytes in cases {
        transport.push(200, bytes);
        let error = updater
            .check()
            .await
            .err()
            .expect("manifest must be rejected");
        assert!(!error.contains("private"));
        assert!(updater.pending.lock().await.is_none());
    }
    transport.push(500, vec![]);
    assert!(updater.check().await.is_err());
}

#[tokio::test]
async fn install_requires_matching_confirmed_selection_and_consumes_ticket() {
    let profile = Profile::new();
    let transport = Arc::new(MockTransport::default());
    let updater = updater(&profile.0, transport.clone());
    assert!(updater.install("0.2.0", "unknown", true).await.is_err());
    transport.manifest(manifest());
    let ticket = updater.check().await.unwrap().ticket_id.unwrap();
    for (version, selection, confirmed) in [
        ("0.2.0", ticket.as_str(), false),
        ("0.3.0", ticket.as_str(), true),
        ("0.2.0", "wrong-ticket", true),
    ] {
        assert!(updater
            .install(version, selection, confirmed)
            .await
            .is_err());
    }
    assert_eq!(transport.requests.lock().unwrap().len(), 1);
    assert!(!profile.0.join("updates").exists());
    transport.push(200, INSTALLER.to_vec());
    let outcome = updater.install("0.2.0", &ticket, true).await.unwrap();
    assert_eq!(outcome.version, "0.2.0");
    assert!(outcome.launched);
    assert!(!outcome.manual_install);
    assert!(outcome.status.contains("completion is not yet known"));
    assert!(!outcome.installer_path.contains("private"));
    assert!(Path::new(&outcome.installer_path).starts_with(profile.0.join("updates")));
    assert!(updater.install("0.2.0", &ticket, true).await.is_err());
    let requests = transport.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1],
        (Url::parse(ARTIFACT).unwrap(), MAX_INSTALLER_BYTES, false)
    );
}

#[tokio::test]
async fn failed_download_or_authentication_consumes_ticket_without_staging() {
    let profile = Profile::new();
    let transport = Arc::new(MockTransport::default());
    let updater = updater(&profile.0, transport.clone());
    for response in [
        Err("network failure".into()),
        Ok(Response {
            status: 500,
            bytes: INSTALLER.to_vec(),
        }),
        Ok(Response {
            status: 200,
            bytes: b"tampered installer".to_vec(),
        }),
    ] {
        transport.manifest(manifest());
        let ticket = updater.check().await.unwrap().ticket_id.unwrap();
        transport.responses.lock().unwrap().push_back(response);
        assert!(updater.install("0.2.0", &ticket, true).await.is_err());
        assert!(updater.install("0.2.0", &ticket, true).await.is_err());
        assert!(!profile.0.join("updates").exists());
    }
    assert_eq!(transport.requests.lock().unwrap().len(), 6);
}

#[tokio::test]
async fn a_new_check_invalidates_old_tickets_even_if_the_check_fails() {
    let transport = Arc::new(MockTransport::default());
    let updater = updater(Path::new("unused"), transport.clone());
    transport.manifest(manifest());
    let old = updater.check().await.unwrap().ticket_id.unwrap();
    transport.manifest(manifest());
    let new = updater.check().await.unwrap().ticket_id.unwrap();
    assert_ne!(old, new);
    assert!(updater.install("0.2.0", &old, true).await.is_err());
    transport.push(200, b"invalid JSON".to_vec());
    assert!(updater.check().await.is_err());
    assert!(updater.install("0.2.0", &new, true).await.is_err());
    assert_eq!(transport.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn platform_artifact_selection_stages_appimage_without_claiming_installation() {
    let profile = Profile::new();
    let transport = Arc::new(MockTransport::default());
    let mut updater = updater(&profile.0, transport.clone());
    updater.config = configuration(ENDPOINT, KEY, "0.1.0", "linux-x86_64");
    updater.handoff = staged_handoff;
    transport.manifest(serde_json::json!({
        "version": "0.2.0", "url": "http://ignored.invalid/wrong.exe", "signature": "ignored",
        "platforms": { "linux-x86_64": {
            "url": "https://cdn.example.com/update.AppImage", "signature": SIGNATURE.trim()
        } }
    }));
    let check = updater.check().await.unwrap();
    assert_eq!(check.action, Some("stage"));
    transport.push(200, INSTALLER.to_vec());
    let outcome = updater
        .install("0.2.0", &check.ticket_id.unwrap(), true)
        .await
        .unwrap();
    assert!(!outcome.launched);
    assert!(outcome.manual_install);
    assert!(outcome
        .status
        .contains("no installed application was modified"));
    assert_eq!(
        Path::new(&outcome.installer_path).file_name().unwrap(),
        "installer.AppImage"
    );
}

#[test]
fn staging_uses_isolated_fixed_names_and_never_replaces_previous_files() {
    let profile = Profile::new();
    let first = stage(&profile.0, Format::Exe, INSTALLER).unwrap();
    let second = stage(&profile.0, Format::Exe, b"second installer").unwrap();
    assert_ne!(first.parent(), second.parent());
    assert_eq!(first.file_name().unwrap(), "installer.exe");
    assert_eq!(std::fs::read(&first).unwrap(), INSTALLER);
    assert_eq!(std::fs::read(&second).unwrap(), b"second installer");
    assert!(!first.parent().unwrap().join("installer.partial").exists());
    assert!(stage(Path::new("relative-profile"), Format::Exe, INSTALLER).is_err());
    assert!(stage(&profile.0.join("missing"), Format::Exe, INSTALLER).is_err());
}

#[test]
fn staging_refuses_existing_non_directory() {
    let profile = Profile::new();
    let updates = profile.0.join("updates");
    std::fs::write(&updates, b"do not replace").unwrap();
    assert!(stage(&profile.0, Format::Exe, INSTALLER).is_err());
    assert_eq!(std::fs::read(updates).unwrap(), b"do not replace");
}

#[cfg(unix)]
#[test]
fn staging_rejects_symlink_ancestors_and_sets_private_appimage_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let profile = Profile::new();
    let target = Profile::new();
    let link = profile.0.join("linked-profile");
    symlink(&target.0, &link).unwrap();
    assert!(stage(&link, Format::AppImage, INSTALLER).is_err());
    assert!(!target.0.join("updates").exists());
    symlink(&target.0, profile.0.join("updates")).unwrap();
    assert!(stage(&profile.0, Format::AppImage, INSTALLER).is_err());
    let path = stage(&target.0, Format::AppImage, INSTALLER).unwrap();
    for private in [
        path.as_path(),
        path.parent().unwrap(),
        &target.0.join("updates"),
    ] {
        assert_eq!(
            std::fs::metadata(private).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
