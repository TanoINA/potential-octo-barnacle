//! Explicit local images only. No network fetches, directory traversal, or image decoding.
use super::types::{AttachedContext, ChatMessage, ImageInput, ProviderConfig};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path},
};

pub const MAX_IMAGES: usize = 4;
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 12 * 1024 * 1024;

fn signature(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Ok("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Ok("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok("image/webp")
    } else {
        Err("Invalid image signature; select a PNG, JPEG, GIF, or WebP image".into())
    }
}

#[cfg(windows)]
fn is_allowed_cloud_reparse_tag(tag: u32) -> bool {
    const IO_REPARSE_TAG_NAME_SURROGATE_BIT: u32 = 0x2000_0000;
    const IO_REPARSE_TAG_CLOUD: u32 = 0x9000_001A;
    const IO_REPARSE_TAG_CLOUD_MASK: u32 = 0x0000_F000;
    const IO_REPARSE_TAG_ONEDRIVE: u32 = 0x8000_0021;
    (tag & IO_REPARSE_TAG_NAME_SURROGATE_BIT) == 0
        && ((tag & !IO_REPARSE_TAG_CLOUD_MASK) == IO_REPARSE_TAG_CLOUD
            || tag == IO_REPARSE_TAG_ONEDRIVE)
}

#[cfg(windows)]
fn is_cloud_reparse_point(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    #[repr(C)]
    struct Win32FindDataW {
        file_attributes: u32,
        creation_time: [u32; 2],
        last_access_time: [u32; 2],
        last_write_time: [u32; 2],
        file_size_high: u32,
        file_size_low: u32,
        reserved0: u32,
        reserved1: u32,
        file_name: [u16; 260],
        alternate_file_name: [u16; 14],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FindFirstFileW(file_name: *const u16, find_file_data: *mut Win32FindDataW) -> isize;
        fn FindClose(find_file: isize) -> i32;
    }
    let clean: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    if clean[..clean.len() - 1].contains(&0) {
        return false;
    }
    let mut data = std::mem::MaybeUninit::<Win32FindDataW>::zeroed();
    let handle = unsafe { FindFirstFileW(clean.as_ptr(), data.as_mut_ptr()) };
    if handle == -1 {
        return false;
    }
    let data = unsafe {
        FindClose(handle);
        data.assume_init()
    };
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    (data.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0
        && is_allowed_cloud_reparse_tag(data.reserved0)
}

fn regular_metadata(path: &Path, metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 && !is_cloud_reparse_point(path) {
            return false;
        }
    }
    #[cfg(not(windows))]
    let _ = path;
    true
}

fn check_path(path: &Path) -> Result<bool, String> {
    // A selected image is a local regular file, not a UNC/network or device path.
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if let Some(Component::Prefix(prefix)) = path.components().next() {
            if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) {
                return Err(
                    "Images must use a local disk path, not a network or device path".into(),
                );
            }
        }
    }
    let mut has_cloud_reparse = false;
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err("Select an image without parent-directory path components".into());
        }
        current.push(component.as_os_str());
        // A bare verbatim drive prefix (\\?\D:) is not an inspectable path.
        // Inspect the root and each subsequent ancestor without rewriting the path.
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|e| format!("Cannot inspect selected image: {e}"))?;
        if metadata.file_type().is_symlink() {
            return Err("Image paths cannot contain symbolic links".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                if is_cloud_reparse_point(&current) {
                    has_cloud_reparse = true;
                } else {
                    return Err("Image paths cannot contain reparse points".into());
                }
            }
        }
    }
    Ok(has_cloud_reparse)
}

/// Hydrate only explicit selected image paths. Non-image files are left to text tools.
pub fn prepare_attachments(attachments: &[AttachedContext]) -> Result<Vec<ImageInput>, String> {
    let mut inputs = Vec::new();
    let mut total = 0;
    for attachment in attachments {
        if attachment.is_dir {
            return Err("Select individual image files, not directories".into());
        }
        let path = Path::new(&attachment.path);
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif") {
            continue;
        }
        if inputs.len() >= MAX_IMAGES {
            return Err("Attach at most 4 images per message".into());
        }
        let cloud_reparse = check_path(path)?;
        let metadata = fs::symlink_metadata(path)
            .map_err(|e| format!("Cannot inspect selected image: {e}"))?;
        if !regular_metadata(path, &metadata) {
            return Err("Selected image must be a regular file".into());
        }
        if metadata.len() > MAX_IMAGE_BYTES as u64 {
            return Err("Each image must be at most 5 MiB".into());
        }
        let mut options = File::options();
        options.read(true);
        #[cfg(windows)]
        if !cloud_reparse {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
        }
        #[cfg(not(windows))]
        let _ = cloud_reparse;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options
            .open(path)
            .map_err(|e| format!("Cannot open selected image: {e}"))?;
        if !regular_metadata(path, &file.metadata().map_err(|e| e.to_string())?) {
            return Err("Selected image must be a regular file".into());
        }
        check_path(path)?;
        let mut bytes = Vec::new();
        file.take(MAX_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Cannot read selected image: {e}"))?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("Each image must be at most 5 MiB".into());
        }
        total += bytes.len();
        if total > MAX_TOTAL_BYTES {
            return Err("Images must total at most 12 MiB per message".into());
        }
        let media_type = signature(&bytes)?.to_string();
        inputs.push(ImageInput {
            source_path: attachment.path.clone(),
            media_type,
            data: STANDARD.encode(bytes),
        });
    }
    Ok(inputs)
}

/// Validate persisted/IPC payloads independently of their source paths. Never reread them.
pub fn validate_inputs(inputs: &[ImageInput]) -> Result<(), String> {
    if inputs.len() > MAX_IMAGES {
        return Err("Attach at most 4 images per message".into());
    }
    let mut total = 0;
    for input in inputs {
        if !matches!(
            input.media_type.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        ) {
            return Err("Unsupported image MIME type; use PNG, JPEG, GIF, or WebP".into());
        }
        if input.data.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
            return Err("Each image must be at most 5 MiB decoded".into());
        }
        let bytes = STANDARD
            .decode(&input.data)
            .map_err(|_| "Image data must be raw standard base64, not a URL".to_string())?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("Each image must be at most 5 MiB decoded".into());
        }
        total += bytes.len();
        if total > MAX_TOTAL_BYTES {
            return Err("Images must total at most 12 MiB per message".into());
        }
        if signature(&bytes)? != input.media_type {
            return Err("Image MIME type does not match its content signature".into());
        }
    }
    Ok(())
}

/// Canonical OpenAI-compatible content; call validate_request_images before dispatch.
/// Text-only messages retain their original string shape.
pub fn user_content(message: &ChatMessage) -> Value {
    let inputs = message.image_inputs.as_deref().unwrap_or(&[]);
    if inputs.is_empty() {
        return json!(message.content);
    }
    let mut blocks = Vec::new();
    // Anthropic menolak blok teks kosong pada turn yang hanya berisi gambar.
    if !message.content.is_empty() {
        blocks.push(json!({"type":"text", "text":message.content}));
    }
    blocks.extend(inputs.iter().map(|input| json!({
        "type":"image_url", "image_url":{"url":format!("data:{};base64,{}", input.media_type, input.data)}
    })));
    Value::Array(blocks)
}

/// Pembanding fallible untuk pengujian payload; runtime memvalidasi seluruh request.
#[cfg(test)]
fn content_blocks(message: &ChatMessage) -> Result<Value, String> {
    validate_inputs(message.image_inputs.as_deref().unwrap_or(&[]))?;
    if message.role != "user"
        && message
            .image_inputs
            .as_ref()
            .is_some_and(|inputs| !inputs.is_empty())
    {
        return Err("Only user messages may contain image attachments".into());
    }
    Ok(user_content(message))
}

pub fn validate_request_images(
    messages: &[ChatMessage],
    provider: &ProviderConfig,
) -> Result<(), String> {
    let mut has_images = false;
    for message in messages {
        let inputs = message.image_inputs.as_deref().unwrap_or(&[]);
        validate_inputs(inputs)?;
        if !inputs.is_empty() {
            if message.role != "user" {
                return Err("Only user messages may contain image attachments".into());
            }
            has_images = true;
        }
    }
    // Model names matter even behind an OpenAI-compatible gateway. Unknown/custom
    // models (including Ollama) are allowed; their provider reports capabilities.
    let model = provider.model.trim().to_ascii_lowercase();
    if has_images && matches!(model.as_str(), "deepseek-chat" | "deepseek-reasoner") {
        return Err(format!("Model '{}' is text-only. Select a vision-capable model or remove the image attachments.", provider.model));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

    fn temp_project() -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("image-attachment-tests")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(name: &str, bytes: &[u8]) -> AttachedContext {
        let path = temp_project().join(name);
        std::fs::write(&path, bytes).unwrap();
        AttachedContext {
            id: "selected".into(),
            path: path.to_string_lossy().into_owned(),
            name: name.into(),
            is_dir: false,
        }
    }

    #[test]
    fn selected_png_is_prepared() {
        let attachment = fixture("screen.PNG", PNG);
        let inputs = prepare_attachments(&[attachment]).unwrap();
        assert_eq!(
            inputs.len(),
            1,
            "selected PNG must be hydrated, not dropped"
        );
    }
    #[test]
    fn preparation_enforces_file_safety_signatures_and_limits() {
        assert!(prepare_attachments(&[fixture("fake.png", b"not an image")]).is_err());
        assert!(prepare_attachments(&[fixture("notes.txt", PNG)])
            .unwrap()
            .is_empty());
        let mut directory = fixture("directory.png", PNG);
        directory.path = std::path::Path::new(&directory.path)
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        directory.is_dir = true;
        assert!(prepare_attachments(&[directory]).is_err());
        let mut missing = fixture("missing.png", PNG);
        missing.path.push_str(".png");
        assert!(prepare_attachments(&[missing]).is_err());
        let large = fixture("large.png", &vec![0; 5 * 1024 * 1024 + 1]);
        assert!(prepare_attachments(&[large]).is_err());
        let image = fixture("tiny.png", PNG);
        assert!(prepare_attachments(&vec![image; 5]).is_err());
        let mut bytes = vec![0; 5 * 1024 * 1024];
        bytes[..PNG.len()].copy_from_slice(PNG);
        let image = fixture("maximum.png", &bytes);
        assert!(prepare_attachments(&[image.clone(), image.clone()]).is_ok());
        assert!(prepare_attachments(&[image.clone(), image.clone(), image]).is_err());
        for (name, bytes, mime) in [
            ("photo.jpg", &b"\xff\xd8\xff"[..], "image/jpeg"),
            ("animation.gif", &b"GIF89a"[..], "image/gif"),
            ("picture.webp", &b"RIFF\x04\0\0\0WEBP"[..], "image/webp"),
            ("mislabeled.jpg", PNG, "image/png"),
        ] {
            let result = prepare_attachments(&[fixture(name, bytes)]).unwrap();
            assert_eq!(result[0].media_type, mime);
            assert_eq!(STANDARD.decode(&result[0].data).unwrap(), bytes);
        }
    }
    #[cfg(windows)]
    #[test]
    fn selected_canonical_png_is_prepared() {
        let mut attachment = fixture("canonical.png", PNG);
        let path = fs::canonicalize(&attachment.path).unwrap();
        assert!(matches!(
            path.components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_))
        ));
        attachment.path = path.to_str().unwrap().to_owned();
        let inputs = prepare_attachments(&[attachment.clone()]).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].source_path, attachment.path);
        assert_eq!(inputs[0].media_type, "image/png");
        assert_eq!(STANDARD.decode(&inputs[0].data).unwrap(), PNG);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn saved_workspace_attachment_is_prepared() {
        let workspace = temp_project();
        let bytes = b"\x89PNG\r\n\x1a\npasted image bytes";
        let attachment = crate::agent::storage::save_attachment(
            Some(workspace.to_str().unwrap()),
            "pasted.png",
            bytes,
        )
        .await
        .unwrap();
        assert!(matches!(
            Path::new(&attachment.path).components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_))
        ));
        assert!(Path::new(&attachment.path).starts_with(
            fs::canonicalize(&workspace)
                .unwrap()
                .join(".aether-agent")
                .join("attachments")
        ));
        assert_eq!(fs::read(&attachment.path).unwrap(), bytes);
        let inputs = prepare_attachments(std::slice::from_ref(&attachment)).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].source_path, attachment.path);
        assert_eq!(inputs[0].media_type, "image/png");
        assert_eq!(STANDARD.decode(&inputs[0].data).unwrap(), bytes);
    }

    #[cfg(windows)]
    #[test]
    fn selected_long_verbatim_disk_path_is_preserved() {
        let mut dir = fs::canonicalize(temp_project()).unwrap();
        for _ in 0..5 {
            dir.push("long-image-path-segment-abcdefghijklmnopqrstuvwxyz-0123456789");
        }
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.png");
        fs::write(&path, PNG).unwrap();
        let path = fs::canonicalize(path).unwrap();
        assert!(path.as_os_str().len() > 260);
        let attachment = AttachedContext {
            id: "long-selected".into(),
            path: path.to_str().unwrap().to_owned(),
            name: "long.png".into(),
            is_dir: false,
        };
        let inputs = prepare_attachments(std::slice::from_ref(&attachment)).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].source_path, attachment.path);
        assert_eq!(STANDARD.decode(&inputs[0].data).unwrap(), PNG);
    }

    #[cfg(windows)]
    #[test]
    fn rejects_network_and_device_prefixes() {
        for path in [
            r"\\server\share\image.png",
            r"\\?\UNC\server\share\image.png",
            r"\\.\C:\image.png",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\image.png",
        ] {
            let attachment = AttachedContext {
                id: "unsafe".into(),
                path: path.into(),
                name: "image.png".into(),
                is_dir: false,
            };
            assert!(
                prepare_attachments(&[attachment])
                    .unwrap_err()
                    .contains("local disk path"),
                "unexpected rejection for {path}"
            );
        }
    }
    fn message() -> ChatMessage {
        serde_json::from_value(
            json!({"id":"m", "role":"user", "content":"describe", "timestamp":1}),
        )
        .unwrap()
    }

    #[test]
    fn image_only_history_has_no_empty_text_block() {
        let message: ChatMessage = serde_json::from_value(json!({
            "id":"image-only", "role":"user", "content":"", "timestamp":1,
            "imageInputs":[{"sourcePath":"deleted.png","mediaType":"image/png","data":STANDARD.encode(PNG)}]
        })).unwrap();
        let content = content_blocks(&message).unwrap();
        assert_eq!(content.as_array().unwrap().len(), 1);
        assert_eq!(content[0]["type"], "image_url");
    }

    #[test]
    fn payload_validation_content_and_legacy_serialization() {
        let mut message = message();
        assert!(message.image_inputs.is_none());
        assert!(serde_json::to_value(&message)
            .unwrap()
            .get("imageInputs")
            .is_none());
        assert_eq!(user_content(&message), json!("describe"));
        let image = prepare_attachments(&[fixture("persisted.png", PNG)])
            .unwrap()
            .remove(0);
        let mut invalid = image.clone();
        invalid.media_type = "image/svg+xml".into();
        assert!(
            validate_inputs(&[invalid]).is_err(),
            "unsafe MIME must be rejected"
        );
        for data in [
            "not base64",
            "data:image/png;base64,iVBORw0KGgo=",
            "",
            "AAAA",
        ] {
            let mut invalid = image.clone();
            invalid.data = data.into();
            assert!(validate_inputs(&[invalid]).is_err());
        }
        let mut invalid = image.clone();
        invalid.media_type = "image/jpeg".into();
        assert!(validate_inputs(&[invalid]).is_err());
        assert!(validate_inputs(&vec![image.clone(); 5]).is_err());
        let mut large = image.clone();
        large.data = STANDARD.encode(vec![0; MAX_IMAGE_BYTES + 1]);
        assert!(validate_inputs(&[large]).is_err());
        let mut bytes = vec![0; MAX_IMAGE_BYTES];
        bytes[..PNG.len()].copy_from_slice(PNG);
        let mut large = image.clone();
        large.data = STANDARD.encode(bytes);
        assert!(validate_inputs(&[large.clone(), large.clone()]).is_ok());
        assert!(validate_inputs(&[large.clone(), large.clone(), large]).is_err());
        message.image_inputs = Some(vec![image.clone()]);
        assert_eq!(
            user_content(&message),
            json!([
                {"type":"text", "text":"describe"},
                {"type":"image_url", "image_url":{"url":format!("data:image/png;base64,{}", image.data)}}
            ])
        );
        let serialized = serde_json::to_value(&message).unwrap();
        assert_eq!(
            serialized["imageInputs"][0]["sourcePath"],
            image.source_path
        );
        assert_eq!(serialized["imageInputs"][0]["mediaType"], "image/png");
        let restored: ChatMessage = serde_json::from_value(serialized).unwrap();
        assert_eq!(user_content(&restored), user_content(&message));
        assert_eq!(content_blocks(&restored).unwrap(), user_content(&message));
        let mut provider = ProviderConfig {
            model: "deepseek-chat".into(),
            ..Default::default()
        };
        assert!(validate_request_images(&[message.clone()], &provider)
            .unwrap_err()
            .contains("vision"));
        provider.model = "deepseek-reasoner".into();
        assert!(validate_request_images(&[message.clone()], &provider).is_err());
        provider.model = "unknown-custom-vision".into();
        assert!(validate_request_images(&[message.clone()], &provider).is_ok());
        message.role = "assistant".into();
        assert!(validate_request_images(&[message], &provider).is_err());
    }

    #[test]
    fn rejects_linked_paths() {
        let image = fixture("real.png", PNG);
        #[cfg(unix)]
        {
            let link = Path::new(&image.path).with_file_name("link.png");
            std::os::unix::fs::symlink(&image.path, &link).unwrap();
            let mut selected = image.clone();
            selected.path = link.to_string_lossy().into_owned();
            assert!(prepare_attachments(&[selected]).is_err());
        }
        #[cfg(windows)]
        {
            let parent = Path::new(&image.path).parent().unwrap();
            let junction = parent.with_file_name(format!("junction-{}", uuid::Uuid::new_v4()));
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&junction)
                .arg(parent)
                .status()
                .unwrap();
            assert!(status.success());
            let mut selected = image.clone();
            selected.path = junction.join("real.png").to_string_lossy().into_owned();
            let error = prepare_attachments(&[selected.clone()]).unwrap_err();
            assert!(error.contains("symbolic links") || error.contains("reparse points"));
            // Keep the junction component intact while giving it a verbatim disk prefix.
            selected.path = fs::canonicalize(junction.parent().unwrap())
                .unwrap()
                .join(junction.file_name().unwrap())
                .join("real.png")
                .to_str()
                .unwrap()
                .to_owned();
            let error = prepare_attachments(&[selected]).unwrap_err();
            assert!(error.contains("symbolic links") || error.contains("reparse points"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_cloud_reparse_tags_distinguish_onedrive_from_surrogates() {
        assert!(is_allowed_cloud_reparse_tag(0x9000_001A));
        assert!(is_allowed_cloud_reparse_tag(0x9000_601A));
        assert!(is_allowed_cloud_reparse_tag(0x8000_0021));
        assert!(!is_allowed_cloud_reparse_tag(0xA000_000C));
        assert!(!is_allowed_cloud_reparse_tag(0xA000_0003));
    }
}
