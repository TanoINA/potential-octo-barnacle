//! Native export consent and scope fencing. Only the backend writes export files.
use crate::{
    app::AppState,
    native,
    widgets::{button, caption},
};
use rune::{El, FileDialogOptions, Tx, col, row};
use rune_kit::Kit;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Scope {
    origin: native::Origin,
    navigation: u64,
}
impl Scope {
    fn capture(s: &mut AppState) -> Self {
        Self {
            origin: native::origin(s),
            navigation: s.controller.navigation_generation(),
        }
    }
    fn current(&self, s: &AppState) -> bool {
        let model = s.controller.model();
        self.navigation == s.controller.navigation_generation()
            && self.origin.generation == s.shell.native.generation
            && self.origin.workspace == model.workspace
            && self.origin.session_id.as_deref()
                == model.current.as_ref().map(|d| d.session.id.as_str())
            && model.preview.is_none()
            && !model.loading
    }
}
#[derive(Clone)]
pub struct Confirmation {
    scope: Scope,
    session_id: String,
    format: String,
    destination: PathBuf,
}
#[derive(Default)]
pub struct ExportState {
    pub confirmation: Option<Confirmation>,
    chooser: Option<Scope>,
    pending: BTreeMap<u64, (Scope, String)>,
    pub status: Option<String>,
    status_scope: Option<Scope>,
}
fn destination(path: &str, workspace: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() || path.chars().any(char::is_control) {
        return Err("Export destination must be a nonempty file path".into());
    }
    let path = Path::new(path);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else if !workspace.is_empty() {
        Path::new(workspace).join(path)
    } else {
        return Err("A relative export path requires an active workspace".into());
    };
    if !resolved.is_absolute() {
        return Err("Export destination must resolve to an absolute path".into());
    }
    Ok(resolved)
}
fn dispatch(s: &mut AppState, request: Confirmation) {
    if !request.scope.current(s) {
        return;
    }
    let path = request.destination.to_string_lossy().into_owned();
    let id = s.controller.dispatch(
        "export_session",
        json!({
            "sessionId": request.session_id, "format": request.format, "destination": path
        }),
    );
    s.shell.export.status_scope = Some(request.scope.clone());
    s.shell.export.status = Some(format!("Exporting to {path}…"));
    s.shell.export.pending.insert(id, (request.scope, path));
}
/// Consume an export intent. Receiving a destination is never itself write consent.
pub fn handle(s: &mut AppState, args: &str, tx: &mut Tx) -> bool {
    let parsed = (|| -> Result<(String, String, Option<String>), String> {
        let value: Value =
            serde_json::from_str(args).map_err(|e| format!("Invalid export intent: {e}"))?;
        if !value.is_object() {
            return Err("Export intent must be a JSON object".into());
        }
        let id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .ok_or("Export requires sessionId")?
            .to_owned();
        let format = value
            .get("format")
            .and_then(Value::as_str)
            .ok_or("Export requires format")?
            .to_owned();
        if !matches!(format.as_str(), "json" | "markdown" | "html") {
            return Err("Export format must be json, markdown, or html".into());
        }
        let path = match value.get("destination") {
            None | Some(Value::Null) => None,
            Some(Value::String(path)) => Some(path.clone()),
            _ => return Err("Export destination must be a file path string".into()),
        };
        Ok((id, format, path))
    })();
    let (session_id, format, path) = match parsed {
        Ok(v) => v,
        Err(e) => {
            s.set_error(e);
            return true;
        }
    };
    let scope = Scope::capture(s);
    if !scope.current(s) || scope.origin.session_id.as_deref() != Some(session_id.as_str()) {
        s.set_error("Export session is no longer the active session");
        return true;
    }
    if s.shell.export.confirmation.is_some() || s.shell.export.chooser.is_some() {
        s.set_error("An export is already awaiting your choice");
        return true;
    }
    s.shell.export.status = None;
    s.shell.export.status_scope = None;
    if let Some(path) = path {
        match destination(&path, &scope.origin.workspace) {
            Ok(destination) => {
                s.shell.export.confirmation = Some(Confirmation {
                    scope,
                    session_id,
                    format,
                    destination,
                })
            }
            Err(error) => s.set_error(error),
        }
        return true;
    }
    let (label, extension) = match format.as_str() {
        "markdown" => ("Markdown", "md"),
        "html" => ("HTML", "html"),
        _ => ("JSON", "json"),
    };
    let options = FileDialogOptions {
        title: format!("Export session as {label}"),
        file_name: Some(format!("session-{session_id}.{extension}")),
        directory: (!scope.origin.workspace.is_empty())
            .then(|| PathBuf::from(&scope.origin.workspace)),
        ..Default::default()
    }
    .filter(label, &[extension]);
    s.shell.export.chooser = Some(scope.clone());
    tx.save_file(options, move |s: &mut AppState, result, _| {
        // In particular, returning A -> B -> A cannot revive this chooser.
        if s.shell.export.chooser.as_ref() != Some(&scope) {
            return;
        }
        s.shell.export.chooser = None;
        if !scope.current(s) {
            return;
        }
        match result {
            Ok(Some(path)) => match destination(&path.to_string_lossy(), &scope.origin.workspace) {
                Ok(destination) => dispatch(
                    s,
                    Confirmation {
                        scope,
                        session_id,
                        format,
                        destination,
                    },
                ),
                Err(error) => s.set_error(error),
            },
            Ok(None) => {}
            Err(error) => s.set_error(format!("Export: {error}")),
        }
    });
    true
}
/// Drain only our results and only publish errors/status into the originating scope.
pub fn sync(s: &mut AppState) -> bool {
    let mut changed = false;
    if s.shell
        .export
        .status_scope
        .as_ref()
        .is_some_and(|scope| !scope.current(s))
    {
        s.shell.export.status = None;
        s.shell.export.status_scope = None;
        changed = true;
    }
    if s.shell
        .export
        .confirmation
        .as_ref()
        .is_some_and(|r| !r.scope.current(s))
    {
        s.shell.export.confirmation = None;
        changed = true;
    }
    if s.shell
        .export
        .chooser
        .as_ref()
        .is_some_and(|scope| !scope.current(s))
    {
        s.shell.export.chooser = None;
        changed = true;
    }
    let ready: Vec<_> = s
        .shell
        .export
        .pending
        .keys()
        .copied()
        .filter(|id| s.controller.model().results.contains_key(id))
        .collect();
    for id in ready {
        let (scope, path) = s.shell.export.pending.remove(&id).expect("tracked export");
        let result = s.controller.take_result(id).expect("ready export result");
        changed = true;
        if !scope.current(s) {
            continue;
        }
        if let Some(error) = result.error {
            s.shell.export.status = None;
            s.set_error(format!("Export: {error}"));
        } else {
            s.shell.export.status = Some(format!("Exported to {path}"));
        }
    }
    changed
}
/// The destination is visible text, not merely a notification before an implicit write.
pub fn render(s: &AppState, kit: &Kit) -> Option<El> {
    let Some(request) = &s.shell.export.confirmation else {
        return s.shell.export.status.as_ref().map(|status| {
            col()
                .key("export-status")
                .role("status")
                .absolute(12.0, 72.0)
                .max_w(700.0)
                .padding(8.0)
                .fill(kit.c.layer_01)
                .child(caption(status, kit))
        });
    };
    let request = request.clone();
    Some(
        col()
            .key("export-confirm-modal")
            .role("dialog")
            .label("Confirm session export")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(20.0)
            .center()
            .modal(false, |s: &mut AppState| s.shell.export.confirmation = None)
            .focus_return("composer")
            .child(
                col()
                    .w_full()
                    .max_w(640.0)
                    .padding(20.0)
                    .gap(16.0)
                    .radius(12.0)
                    .fill(kit.c.layer_01)
                    .child(caption(
                        format!("Export session as {}?", request.format),
                        kit,
                    ))
                    .child(
                        caption(format!("Path: {}", request.destination.display()), kit)
                            .key("export-destination")
                            .w_full()
                            .min_w(0.0),
                    )
                    .child(caption(
                        "Confirm writing this file. An existing file at this path may be replaced.",
                        kit,
                    ))
                    .child(
                        row()
                            .gap(12.0)
                            .child(button("export-cancel", "Cancel", kit, |s, _| {
                                s.shell.export.confirmation = None
                            }))
                            .child(button(
                                "export-confirm",
                                "Confirm export",
                                kit,
                                move |s, _| {
                                    let Some(active) = s.shell.export.confirmation.take() else {
                                        return;
                                    };
                                    if active.scope == request.scope
                                        && active.destination == request.destination
                                    {
                                        dispatch(s, active);
                                    }
                                },
                            )),
                    ),
            ),
    )
}
