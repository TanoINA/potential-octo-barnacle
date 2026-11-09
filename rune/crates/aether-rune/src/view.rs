//! Retained native scene. No HTML, WebView, or renderer-side backend shortcuts.
use crate::{
    app::AppState,
    native,
    panels::{self, Panel},
    widgets::{button, caption, card},
};
use rune::{Cx, El, GrowthSpec, ScrollSpec, TextStyle, col, row, text, text_input};
use rune_kit::Kit;
#[path = "cards.rs"]
pub mod cards;

pub fn view(s: &AppState, cx: &mut Cx<AppState>) -> El {
    let kit = Kit::neutral(true);
    cx.foreground(kit.c.text_base);
    cx.reduced_motion(s.shell.reduced_motion);
    let size = cx.layout_size("aether-window").unwrap_or([1280.0, 840.0]);
    let compact = size[0] < 1050.0;
    let modal = s.shell.queue.edit.is_some() || s.ui.panel.is_some()
        || s.shell.native.preview.is_some()
        || s.shell.export.confirmation.is_some()
        || s.shell.lifecycle != crate::lifecycle::Lifecycle::Open;
    let mut root = row()
        .key("shell-background")
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(kit.c.page);
    if s.shell.sidebar_open && !compact {
        root = root.child(sidebar(s, &kit));
    }
    let mut center = col().grow(1.0).min_w(0.0).min_h(0.0).h_full();
    center = center.child(header(s, &kit));
    let mut body = row().grow(1.0).min_h(0.0).min_w(0.0);
    body = body.child(chat(s, &kit));
    if !compact {
        body = body.child(timeline(s, &kit));
    }
    center = center.child(body).child(crate::composer::render(s, &kit));
    root = root.child(center);
    if modal {
        root = root.inert();
    }
    let mut root = col()
        .key("aether-window")
        .w_full()
        .h_full()
        .min_h(0.0)
        .min_w(0.0)
        .child(root);
    if let Some(panel) = panels::render(s, cx, &kit) {
        root = root.child(
            col()
                .key("panel-overlay")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .padding(12.0)
                .modal(false, |s: &mut AppState| s.ui.panel = None)
                .focus_return(crate::desktop::palette_focus_return(s))
                .child(
                    col()
                        .w_full()
                        .h_full()
                        .min_h(0.0)
                        .min_w(0.0)
                        .fill(kit.c.bg_base)
                        .border(1.0, kit.c.border_muted)
                        .radius(12.0)
                        .child(panel),
                ),
        );
    }
    if let Some(path) = &s.shell.native.preview {
        let mut picture = col()
            .key("image-preview-content")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .min_w(0.0);
        if let Some(image) = s.shell.native.image(path) {
            if let Some(error) = image.error() {
                picture = picture.child(caption(format!("Preview unavailable: {error}"), &kit));
            } else if let Some([w, h]) = image.size() {
                picture = picture.child(
                    col()
                        .key("preview-pixels")
                        .w_full()
                        .grow(1.0)
                        .min_h(0.0)
                        .aspect_fit(w as f32 / h as f32)
                        .pixels(image.pixels()),
                );
            } else {
                picture = picture.child(caption("Decoding local image...", &kit));
            }
        } else {
            picture = picture.child(caption("Preparing local image preview...", &kit));
        }
        if let Some(error) = &s.shell.error {
            picture = picture.child(caption(error, &kit));
        }
        root = root.child(
            col()
                .key("image-preview-modal")
                .role("dialog")
                .label("Image attachment preview")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .padding(16.0)
                .fill(kit.c.bg_deep)
                .gap(12.0)
                .modal(false, |s: &mut AppState| s.shell.native.preview = None)
                .focus_return("composer")
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .child(caption(path, &kit).grow(1.0).min_w(0.0))
                        .child(button(
                            "close-image-preview",
                            "Close preview (Esc)",
                            &kit,
                            |s, _| s.shell.native.preview = None,
                        )),
                )
                .child(picture),
        );
    }
    if let Some(edit) = crate::statequeue::modal(s, &kit) { root = root.child(edit); }
    if let Some(export) = crate::export::render(s, &kit) {
        root = root.child(export);
    }
    if let Some(overlay) = crate::lifecycle::render(s, &kit) {
        root = root.child(overlay);
    }
    root.on_drop_files(|s: &mut AppState, files, _| {
        if s.shell.lifecycle == crate::lifecycle::Lifecycle::Open {
            native::attach_paths(s, files);
        }
    })
}
fn sidebar(s: &AppState, kit: &Kit) -> El {
    let mut list = col().gap(6.0).w_full();
    let query = s.shell.search.text.to_lowercase();
    let current = s
        .controller
        .model()
        .current
        .as_ref()
        .map(|data| data.session.id.as_str());
    for session in &s.controller.model().sessions {
        if !query.is_empty() && !session.title.to_lowercase().contains(&query) {
            continue;
        }
        let id = session.id.clone();
        let title = if session.pinned {
            format!("? {}", session.title)
        } else {
            session.title.clone()
        };
        let entry = button(&format!("session-{id}"), &title, kit, move |s, tx| {
            native::cancel(s);
            s.controller.open_session(&id);
            s.shell.row_layouts.clear();
            s.shell.selected_message = None;
            if s.shell.reduced_motion {
                tx.instant();
            }
        })
        .w_full()
        .role("button")
        .focus_selected(current == Some(session.id.as_str()));
        list = list.child(entry);
    }
    col()
        .key("sidebar")
        .role("navigation")
        .label("Sessions")
        .w(250.0)
        .h_full()
        .min_h(0.0)
        .padding(14.0)
        .gap(12.0)
        .fill(kit.c.bg_deep)
        .child(
            text("AETHER")
                .font(TextStyle::new(18.0, 400.0))
                .color(kit.c.text_base),
        )
        .child(button("new-chat", "+  New chat", kit, |s, tx| s.new_session(tx)).w_full())
        .child(
            text_input(&s.shell.search, TextStyle::new(13.0, 400.0))
                .key("session-search")
                .label("Search sessions")
                .single_line()
                .placeholder("Search sessions", kit.c.text_faint)
                .on_edit(|s: &mut AppState| &mut s.shell.search)
                .padding(8.0)
                .fill(kit.c.layer_01)
                .radius(6.0),
        )
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        )
        .child(
            button("history", "Session history  -  Ctrl+P", kit, |s, _| {
                s.ui.open(Panel::History, &mut s.controller)
            })
            .w_full(),
        )
        .child(
            button("sidebar-settings", "Settings  -  Ctrl+,", kit, |s, _| {
                s.ui.open(Panel::Settings, &mut s.controller)
            })
            .w_full(),
        )
        .child(caption("Rune  -  native GPU", kit))
}
fn header(s: &AppState, kit: &Kit) -> El {
    let model = s.controller.model();
    let title = model
        .current
        .as_ref()
        .map_or("New chat", |data| data.session.title.as_str());
    let provider = model
        .settings
        .providers
        .iter()
        .find(|provider| provider.id == model.settings.active_provider_id);
    let provider = model
        .current
        .as_ref()
        .and_then(|data| data.model_override.as_ref())
        .and_then(|over| over.provider_id.as_ref())
        .and_then(|id| {
            model
                .settings
                .providers
                .iter()
                .find(|provider| &provider.id == id)
        })
        .or(provider);
    let model_name = provider.map_or("Configure model".to_string(), |provider| {
        format!(
            "{} / {}",
            provider.name,
            model
                .current
                .as_ref()
                .and_then(|data| data.model_override.as_ref())
                .and_then(|over| over.model.as_deref())
                .unwrap_or(&provider.model)
        )
    });
    row()
        .key("header")
        .w_full()
        .min_w(0.0)
        .padding(12.0)
        .gap(8.0)
        .flex_wrap()
        .child(
            button("toggle-sidebar", "?", kit, |s, tx| {
                s.shell.sidebar_open = !s.shell.sidebar_open;
                tx.instant();
            })
            .label("Toggle sidebar"),
        )
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(3.0)
                .child(text(title).font(TextStyle::new(16.0, 400.0)))
                .child(caption(
                    if model.workspace.is_empty() {
                        "No workspace selected"
                    } else {
                        &model.workspace
                    },
                    kit,
                )),
        )
        .child(button(
            "workspace",
            "Open folder",
            kit,
            native::choose_workspace,
        ))
        .child(button("model-picker", &model_name, kit, |s, _| {
            s.ui.open(Panel::Models, &mut s.controller)
        }))
        .child(button("settings", "Settings", kit, |s, _| {
            s.ui.open(Panel::Settings, &mut s.controller)
        }))
}
fn chat(s: &AppState, kit: &Kit) -> El {
    let model = s.controller.model();
    let mut stack = col()
        .key("message-stack")
        .w_full()
        .gap(18.0)
        .padding(24.0)
        .growth(GrowthSpec::default())
        .virtualize(140.0);
    if let Some(data) = model
        .preview
        .as_ref()
        .map(|page| &page.data)
        .or(model.current.as_ref())
    {
        let mut owner = String::new();
        for message in &data.messages {
            if message.role == "user" {
                owner = message.id.clone();
            }
            let id = message.id.clone();
            let owner_id = owner.clone();
            let rich = cards::render_content(s, &data.session.id, message, kit);
            let mut content = col()
                .key(format!("message-body-{id}"))
                .w_full()
                .gap(10.0)
                .streaming(message.is_streaming.unwrap_or(false))
                .child(caption(
                    if message.role == "user" {
                        "YOU"
                    } else {
                        "AETHER"
                    },
                    kit,
                ))
                .child(rich)
                .on_drawn_layout(move |s: &mut AppState, rect| {
                    s.shell.row_layouts.insert(id.clone(), rect);
                    let top = s.shell.viewport[1];
                    let bottom = top + s.shell.viewport[3];
                    if rect[1] <= top + 64.0
                        && rect[1] + rect[3] > top
                        && rect[1] < bottom
                        && !owner_id.is_empty()
                    {
                        s.shell.selected_message = Some(owner_id.clone());
                    }
                });
            if let Some(attachments) = &message.attachments {
                let mut items = row().gap(8.0).flex_wrap();
                for attachment in attachments.iter().filter(|a| native::image_candidate(a)) {
                    let selected = attachment.clone();
                    items = items.child(button(
                        &format!("preview-message-{}-{}", message.id, attachment.id),
                        &format!("Preview {}", attachment.name),
                        kit,
                        move |s, _| native::preview(s, &selected),
                    ));
                }
                content = content.child(items);
            }
            if let Some(reasoning) = cards::reasoning(s, &data.session.id, message, kit) {
                content = content.child(reasoning);
            }
            if let Some(trace) = cards::execution_trace(s, &data.session.id, message, kit) {
                content = content.child(trace);
            }
            if let Some(council) = &message.council_turn {
                let mut candidates = row().gap(12.0).w_full();
                for (name, candidate) in [("A", &council.candidate_a), ("B", &council.candidate_b)]
                {
                    let message_id = message.id.clone();
                    let candidate_body = col()
                        .gap(8.0)
                        .child(s.rich.borrow_mut().render(
                            &candidate.content,
                            &format!("council-{}-{name}", message.id),
                            kit,
                        ))
                        .child(
                            button(
                                &format!("choose-{}-{name}", message.id),
                                &format!("Choose {name}"),
                                kit,
                                move |s, _| {
                                    if let Err(error) =
                                        s.controller.choose_council(&message_id, name)
                                    {
                                        s.set_error(error);
                                    }
                                },
                            )
                            .disabled(
                                model.busy
                                    || model.preview.is_some()
                                    || candidate.is_streaming.unwrap_or(false),
                            ),
                        );
                    candidates = candidates.child(
                        card(
                            &format!("Candidate {name}  -  {}", candidate.model),
                            candidate_body,
                            kit,
                        )
                        .grow(1.0)
                        .min_w(0.0),
                    );
                }
                content = content.child(candidates).child(caption(
                    format!(
                        "Council selection: {}",
                        council.chosen_winner.as_deref().unwrap_or("not selected")
                    ),
                    kit,
                ));
            }
            stack = stack.child(
                panels::wrap_message(s, &message.id, content, kit)
                    .key(format!("message-{}", message.id)),
            );
        }
    }
    if model
        .current
        .as_ref()
        .is_none_or(|data| data.messages.is_empty())
    {
        stack=stack.child(col().key("empty-chat").w_full().padding(30.0).gap(12.0)
            .child(text("What would you like to work on?").font(TextStyle::new(25.0, 400.0)))
            .child(caption("Choose a workspace, configure a provider in Settings, then send a message.\nFiles and images can be pasted, dropped or attached.",kit)));
    }
    let mut shell = col().grow(1.0).min_w(0.0).min_h(0.0);
    if model.preview.is_some() {
        shell = shell.child(button(
            "close-preview",
            "Read-only history  -  Return to latest",
            kit,
            |s, tx| {
                s.controller.close_preview();
                tx.instant();
            },
        ));
    }
    if model
        .window
        .as_ref()
        .is_some_and(|window| window.offset > 0)
    {
        shell = shell.child(
            button(
                "load-older",
                if model.loading_older {
                    "Loading earlier messages - "
                } else {
                    "Load earlier messages"
                },
                kit,
                |s, tx| {
                    if let Err(error) = s.controller.older() {
                        s.set_error(error);
                    }
                    tx.instant();
                },
            )
            .disabled(model.loading_older),
        );
    }
    // The growth/virtualized stack is the FIRST child of a bounded scroll viewport.
    shell = shell.child(
        col()
            .key("chat-viewport")
            .grow(1.0)
            .min_h(0.0)
            .min_w(0.0)
            .w_full()
            .scroll(ScrollSpec { stick_bottom: true })
            .child(stack)
            .on_drawn_layout(|s: &mut AppState, rect| s.shell.viewport = rect),
    );
    shell
}
fn timeline(s: &AppState, kit: &Kit) -> El {
    let mut turns = col().gap(8.0).w_full();
    if let Some(data) = s
        .controller
        .model()
        .preview
        .as_ref()
        .map(|page| &page.data)
        .or(s.controller.model().current.as_ref())
    {
        for (index, message) in data
            .messages
            .iter()
            .filter(|message| message.role == "user")
            .enumerate()
        {
            let id = message.id.clone();
            let label = format!(
                "{}  -  {}",
                index + 1,
                message.content.chars().take(18).collect::<String>()
            );
            turns = turns.child(
                button(&format!("turn-{id}"), &label, kit, move |s, tx| {
                    s.shell.selected_message = Some(id.clone());
                    if s.shell.reduced_motion {
                        tx.instant();
                    }
                    tx.scroll_into_view(format!("message-{id}"));
                })
                .focus_selected(s.shell.selected_message.as_deref() == Some(message.id.as_str()))
                .w_full(),
            );
        }
    }
    col()
        .key("timeline")
        .role("navigation")
        .label("Conversation timeline")
        .w(142.0)
        .padding(10.0)
        .gap(12.0)
        .child(caption("TIMELINE", kit))
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(turns),
        )
}
