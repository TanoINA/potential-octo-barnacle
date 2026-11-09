//! Native clipboard/dialog/attachment operations are bound to the scope at initiation.
use crate::app::AppState;
use aether_core::agent::{AttachedContext, ImageInput};
use base64::{Engine, engine::general_purpose::STANDARD};
use rune::{ClipboardContent, FileDialogOptions, Image, ImageCache, Tx, Waker};
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
const CACHE_BYTES: usize = 20 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub session_id: Option<String>,
    pub workspace: String,
    pub generation: u64,
}
#[derive(Clone)]
pub enum PendingAttachment {
    Paths,
    Paste,
    Prepared {
        attachments: Vec<AttachedContext>,
        preview_only: bool,
    },
}
#[derive(Clone)]
pub struct Pending {
    pub origin: Origin,
    pub kind: PendingAttachment,
}
pub struct CachedImage {
    pub path: String,
    pub image: Image,
    bytes: usize,
}
pub struct NativeState {
    pub pending: BTreeMap<u64, Pending>,
    pub generation: u64,
    scope: Option<(Option<String>, String)>,
    pub dialog_pending: bool,
    pub preview: Option<String>,
    pub images: VecDeque<CachedImage>,
    cache: ImageCache,
    changed: Arc<AtomicBool>,
}
impl Default for NativeState {
    fn default() -> Self {
        Self::new(Arc::new(|| {}))
    }
}
impl NativeState {
    pub fn new(wake: Waker) -> Self {
        let changed = Arc::new(AtomicBool::new(false));
        let flag = changed.clone();
        let cache = ImageCache::new(Arc::new(move || {
            flag.store(true, Ordering::Release);
            wake();
        }));
        Self {
            pending: BTreeMap::new(),
            generation: 0,
            scope: None,
            dialog_pending: false,
            preview: None,
            images: VecDeque::new(),
            cache,
            changed,
        }
    }
    pub fn preparing(&self) -> bool {
        self.dialog_pending
            || self.pending.values().any(|p| {
                !matches!(
                    p.kind,
                    PendingAttachment::Prepared {
                        preview_only: true,
                        ..
                    }
                )
            })
    }
    pub fn image(&self, path: &str) -> Option<&Image> {
        self.images
            .iter()
            .find(|i| i.path == path)
            .map(|i| &i.image)
    }
    pub fn cached_bytes(&self) -> usize {
        self.images.iter().map(|i| i.bytes).sum()
    }
}
fn scope(s: &AppState) -> (Option<String>, String) {
    (
        s.controller
            .model()
            .current
            .as_ref()
            .map(|d| d.session.id.clone()),
        s.controller.model().workspace.clone(),
    )
}
pub fn cancel(s: &mut AppState) {
    s.shell.native.generation = s.shell.native.generation.wrapping_add(1);
    s.shell.native.pending.clear();
    s.shell.native.dialog_pending = false;
    s.shell.native.preview = None;
}
fn observe_scope(s: &mut AppState) -> bool {
    let current = scope(s);
    if s.shell.native.scope.as_ref() == Some(&current) {
        return false;
    }
    cancel(s);
    s.shell.native.scope = Some(current);
    s.shell.native.images.clear();
    true
}
pub fn origin(s: &mut AppState) -> Origin {
    observe_scope(s);
    let (session_id, workspace) = scope(s);
    Origin {
        session_id,
        workspace,
        generation: s.shell.native.generation,
    }
}
fn current(s: &AppState, o: &Origin) -> bool {
    let (id, workspace) = scope(s);
    id == o.session_id
        && workspace == o.workspace
        && s.shell.native.generation == o.generation
        && s.controller.model().preview.is_none()
}
fn track(
    s: &mut AppState,
    origin: Origin,
    command: &str,
    args: serde_json::Value,
    kind: PendingAttachment,
) {
    let id = s.controller.dispatch(command, args);
    s.shell.native.pending.insert(id, Pending { origin, kind });
}
pub fn image_candidate(a: &AttachedContext) -> bool {
    !a.is_dir
        && Path::new(&a.path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp"
                )
            })
}
pub fn attach_paths(s: &mut AppState, paths: &[PathBuf]) {
    if paths.is_empty() || s.controller.model().preview.is_some() {
        return;
    }
    let origin = origin(s);
    track(
        s,
        origin,
        "inspect_paths",
        json!({"paths":paths.iter().map(|p|p.to_string_lossy()).collect::<Vec<_>>()}),
        PendingAttachment::Paths,
    );
}
pub fn choose_attachment(s: &mut AppState, tx: &mut Tx) {
    if s.shell.native.dialog_pending || s.controller.model().preview.is_some() {
        return;
    }
    let origin = origin(s);
    s.shell.native.dialog_pending = true;
    tx.open_file(
        FileDialogOptions {
            title: "Attach context or image".into(),
            ..Default::default()
        },
        move |s: &mut AppState, result, _| {
            if !current(s, &origin) {
                return;
            }
            s.shell.native.dialog_pending = false;
            match result {
                Ok(Some(path)) => attach_paths(s, &[path]),
                Ok(None) => {}
                Err(e) => s.set_error(e.to_string()),
            }
        },
    );
}
pub fn choose_workspace(s: &mut AppState, tx: &mut Tx) {
    let origin = origin(s);
    tx.open_folder(
        FileDialogOptions {
            title: "Choose workspace".into(),
            ..Default::default()
        },
        move |s: &mut AppState, result, _| {
            if !current(s, &origin) {
                return;
            }
            match result {
                Ok(Some(path)) => {
                    cancel(s);
                    s.controller.dispatch(
                        "set_workspace_cwd",
                        json!({"newCwd":path.to_string_lossy()}),
                    );
                }
                Ok(None) => {}
                Err(e) => s.set_error(e.to_string()),
            }
        },
    );
}
pub fn paste(s: &mut AppState, content: &ClipboardContent, _: &mut Tx) -> bool {
    if s.controller.model().preview.is_some() {
        return !content.files().is_empty() || content.image().is_some();
    }
    if !content.files().is_empty() {
        attach_paths(s, content.files());
        return true;
    }
    if let Some(image) = content.image() {
        let origin = origin(s);
        track(
            s,
            origin.clone(),
            "save_pasted_attachment",
            json!({"name":format!("paste-{}.png",uuid::Uuid::new_v4()),"bytes":image.png(),"expectedWorkspace":origin.workspace}),
            PendingAttachment::Paste,
        );
        return true;
    }
    false
}
fn prepare(
    s: &mut AppState,
    origin: Origin,
    attachments: Vec<AttachedContext>,
    preview_only: bool,
) {
    track(
        s,
        origin,
        "prepare_image_attachments",
        json!({"attachments":attachments}),
        PendingAttachment::Prepared {
            attachments,
            preview_only,
        },
    );
}
pub fn preview(s: &mut AppState, attachment: &AttachedContext) {
    let origin = origin(s);
    if !image_candidate(attachment) || !current(s, &origin) {
        return;
    }
    s.shell.native.preview = Some(attachment.path.clone());
    if s.shell.native.image(&attachment.path).is_none() && !s.shell.native.pending.values().any(|p|matches!(&p.kind,PendingAttachment::Prepared{attachments,..} if attachments.iter().any(|a|a.path==attachment.path))){prepare(s,origin,vec![attachment.clone()],true);}
}
fn cache_inputs(
    s: &mut AppState,
    inputs: &[ImageInput],
    attachments: &[AttachedContext],
) -> Result<(), String> {
    aether_core::agent::images::validate_inputs(inputs)?;
    for input in inputs {
        if !attachments.iter().any(|a| a.path == input.source_path) {
            return Err("Unexpected image source in attachment response".into());
        }
        if s.shell.native.image(&input.source_path).is_some() {
            continue;
        }
        let bytes = STANDARD.decode(&input.data).map_err(|e| e.to_string())?;
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|e| format!("Image header: {e}"))?;
        let budget = (w as usize)
            .checked_mul(h as usize)
            .and_then(|p| p.checked_mul(6))
            .and_then(|p| p.checked_add(bytes.len()))
            .ok_or("Image dimensions exceed preview budget")?;
        if w == 0 || h == 0 || budget > CACHE_BYTES {
            return Err("Image exceeds the 20 MiB native preview budget".into());
        }
        while s.shell.native.images.len() >= 8
            || s.shell.native.cached_bytes() + budget > CACHE_BYTES
        {
            s.shell.native.images.pop_front();
        }
        let image = s.shell.native.cache.load_mime(
            Arc::<[u8]>::from(bytes),
            &input.media_type,
            1024.0,
            1.0,
        );
        s.shell.native.images.push_back(CachedImage {
            path: input.source_path.clone(),
            image,
            bytes: budget,
        });
    }
    Ok(())
}
pub fn sync(s: &mut AppState) -> bool {
    let mut changed = observe_scope(s) || s.shell.native.changed.swap(false, Ordering::AcqRel);
    let ready: Vec<_> = s
        .shell
        .native
        .pending
        .iter()
        .filter_map(|(&id, p)| {
            s.controller
                .model()
                .results
                .get(&id)
                .cloned()
                .map(|r| (id, p.clone(), r))
        })
        .collect();
    changed |= !ready.is_empty();
    for (id, p, result) in ready {
        s.shell.native.pending.remove(&id);
        s.controller.take_result(id);
        if !current(s, &p.origin) {
            continue;
        }
        if let Some(error) = result.error {
            s.set_error(error);
            continue;
        }
        let Some(value) = result.value else {
            s.set_error("Attachment response was empty");
            continue;
        };
        let outcome = (|| -> Result<(), String> {
            match p.kind {
                PendingAttachment::Paths | PendingAttachment::Paste => {
                    let attachments: Vec<AttachedContext> =
                        if matches!(p.kind, PendingAttachment::Paste) {
                            vec![serde_json::from_value(value).map_err(|e| e.to_string())?]
                        } else {
                            serde_json::from_value(value).map_err(|e| e.to_string())?
                        };
                    for a in attachments {
                        if image_candidate(&a) {
                            prepare(s, p.origin.clone(), vec![a], false);
                        } else {
                            s.add_attachments(vec![a]);
                        }
                    }
                }
                PendingAttachment::Prepared {
                    attachments,
                    preview_only,
                } => {
                    let inputs: Vec<ImageInput> =
                        serde_json::from_value(value).map_err(|e| e.to_string())?;
                    if attachments
                        .iter()
                        .any(|a| !inputs.iter().any(|i| i.source_path == a.path))
                    {
                        return Err("Image preparation omitted the selected image".into());
                    }
                    // A preview failure must not remove otherwise valid model input.
                    let preview_error = cache_inputs(s, &inputs, &attachments).err();
                    if !preview_only {
                        aether_core::agent::images::validate_inputs(&inputs)?;
                        let replay = &mut s.controller.model_mut().replay_image_inputs;
                        for input in inputs { replay.retain(|i| i.source_path != input.source_path); replay.push(input); }
                        s.add_attachments(attachments);
                    }
                    if let Some(error) = preview_error {
                        s.set_error(error);
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            s.set_error(format!("Attachment: {error}"));
        }
    }
    changed
}
