//! Native composer controls and context completion.
use crate::{app::AppState, native, panels::Panel, widgets::{button, caption}};
use rune::{El, Key, TextStyle, col, row, text, text_input};
use rune_kit::Kit;
pub fn is_local(prompt: &str) -> bool {
    matches!(prompt.split_whitespace().next().unwrap_or("").to_ascii_lowercase().as_str(), "/new" | "/clear" | "/undo" | "/retry" | "/compact" | "/compress" | "/stop" | "/review" | "/reflect" | "/refine" | "/title" | "/pin" | "/export" | "/save" | "/model" | "/settings" | "/history" | "/artifacts" | "/tools" | "/memory" | "/terminal" | "/git" | "/tasks" | "/skills" | "/init")
}
pub fn render(s: &AppState, kit: &Kit) -> El {
    let model = s.controller.model();
    let mut area = col().key("composer-area").w_full().padding(16.0).gap(8.0);
    if !s.configuration_ready() {area=area.child(crate::app::configuration_status(s,kit));}
    if s.shell.queue.total() > 0 { area = area.child(crate::statequeue::render(s, kit)); }
    if let Some(error) = s.shell.error.as_ref().or(model.error.as_ref()) {
        area = area.child(
            text(error)
                .key("error-banner")
                .role("status")
                .selectable(true)
                .color(kit.c.text_base),
        );
    }
    if let Some(status) = &model.run_status {
        area = area.child(caption(
            format!(
                "{:?}{}",
                status.phase,
                if status.save_failed {
                    "  -  Save failed"
                } else {
                    ""
                }
            ),
            kit,
        ));
    }
    if let Some(agents) = crate::view::cards::subagents(s, kit) {
        area = area.child(agents);
    }
    if !model.attachments.is_empty() {
        let mut attachments = row().gap(8.0).flex_wrap();
        for attachment in &model.attachments {
            let id = attachment.id.clone();
            let mut chip = row().gap(4.0);
            if native::image_candidate(attachment) {
                let selected = attachment.clone();
                let mut preview = button(
                    &format!("preview-attachment-{id}"),
                    &format!("Preview {}", attachment.name),
                    kit,
                    move |s, _| native::preview(s, &selected),
                );
                if let Some(image) = s
                    .shell
                    .native
                    .image(&attachment.path)
                    .filter(|i| i.size().is_some())
                {
                    preview = preview.child(col().w(48.0).h(40.0).pixels(image.pixels()));
                }
                chip = chip.child(preview);
            } else {
                chip = chip.child(caption(&attachment.name, kit));
            }
            attachments = attachments.child(chip.child(button(
                &format!("remove-attachment-{id}"),
                "Remove",
                kit,
                move |s, _| {
                    s.controller
                        .model_mut()
                        .attachments
                        .retain(|item| item.id != id)
                },
            )));
        }
        area = area.child(attachments);
    }
    if s.shell.native.preparing() {
        area = area.child(
            row()
                .gap(8.0)
                .child(caption("Preparing attachments - send is paused", kit).role("status"))
                .child(button(
                    "cancel-attachment-preparation",
                    "Cancel preparation",
                    kit,
                    |s, _| native::cancel(s),
                )),
        );
    }
    let text_only = s.candidate_a().is_some_and(|p| {
        p.base_url.to_lowercase().contains("deepseek")
            || p.model.to_lowercase().starts_with("deepseek")
    });
    if text_only && model.attachments.iter().any(native::image_candidate) {
        area=area.child(caption("Selected model is known to be text-only; choose a vision model in Models to send images.",kit).key("text-only-model-warning").role("status"));
    }
    let field = text_input(&s.shell.composer, TextStyle::new(14.0, 400.0))
        .key("composer")
        .label("Message Aether")
        .w_full()
        .min_h(76.0)
        .max_h(190.0)
        .padding(12.0)
        .radius(9.0)
        .fill(kit.c.layer_01)
        .border(1.0, kit.c.border_muted)
        .placeholder(
            "Message Aether -   Shift+Enter for a new line",
            kit.c.text_faint,
        )
        .on_edit(|s: &mut AppState| &mut s.shell.composer)
        .on_input_change(crate::completion::input_changed)
        .on_submit(|s: &mut AppState, tx| s.send(tx))
        .on_input_paste(native::paste)
        .on_input_key(|s: &mut AppState, event, tx| {
            if crate::completion::on_key(s, event, tx) { return true; }
            if event.key == Key::Enter
                && (event.mods.cmd || event.mods.ctrl)
                && s.shell.composer.preedit.is_none()
            {
                s.send(tx);
                true
            } else {
                false
            }
        });
    let mut controls = row()
        .gap(8.0)
        .w_full()
        .flex_wrap()
        .child(
            button("attach", "Attach", kit, native::choose_attachment)
                .disabled(model.preview.is_some()),
        )
        .child(
            button(
                "council-toggle",
                if s.shell.council {
                    "Council: on"
                } else {
                    "Council: off"
                },
                kit,
                |s, _| s.shell.council = !s.shell.council,
            )
            .disabled(model.busy),
        )
        .child(button(
            "execution-mode",
            &format!("Mode: {:?}", model.settings.execution_mode),
            kit,
            |s, _| s.ui.open(Panel::Settings, &mut s.controller),
        ));
    if s.shell.council {
        let b = s.candidate_b().map_or("Configure candidate B".into(), |p| {
            format!("B: {} / {} (next)", p.name, p.model)
        });
        controls = controls.child(
            button("council-candidate-b", &b, kit, |s, _| s.cycle_candidate_b())
                .disabled(model.busy),
        );
    }
    if model.busy {
        controls = controls.child(button("stop", "Stop", kit, |s, _| {
            if let Err(error) = s.controller.abort() {
                s.set_error(error);
            }
        }));
    }
    {
        controls = controls.child(button("send", if model.busy { "Queue message" } else { "Send" }, kit, |s, tx| s.send(tx)).disabled(
            (!s.configuration_ready() && (!is_local(&s.shell.composer.text) || matches!(s.shell.composer.text.split_whitespace().next().unwrap_or("").to_ascii_lowercase().as_str(),"/retry"|"/review"|"/reflect"|"/refine")))
                || s.shell.native.preparing()
                || model.preview.is_some()
                || model.loading
                || (s.shell.composer.text.trim().is_empty() && model.attachments.is_empty()),
        ));
    }
    // Bounded in-flow suggestions sit above the field, outside the chat viewport.
    if let Some(popup) = crate::completion::render(s, kit) { area = area.child(popup); }
    area.child(field).child(controls)
}
