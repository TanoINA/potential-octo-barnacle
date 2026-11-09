//! Pure Rune controls shared by the shell. Every action has a semantic role/name.
use crate::app::AppState;
use rune::{El, TextStyle, Tx, col, text};
use rune_kit::Kit;
pub fn button(
    key: &str,
    label: &str,
    kit: &Kit,
    action: impl Fn(&mut AppState, &mut Tx) + 'static,
) -> El {
    col()
        .key(key)
        .role("button")
        .label(label)
        .focusable(true)
        .padding(9.0)
        .radius(7.0)
        .fill(kit.c.layer_02)
        .on_press_tx(action)
        .child(
            text(label)
                .font(TextStyle::new(13.0, 400.0))
                .color(kit.c.text_base),
        )
}
pub fn caption(value: impl Into<String>, kit: &Kit) -> El {
    text(value)
        .font(TextStyle::new(12.0, 400.0))
        .color(kit.c.text_muted)
}
pub fn card(title: &str, body: El, kit: &Kit) -> El {
    col()
        .w_full()
        .gap(8.0)
        .padding(12.0)
        .radius(8.0)
        .fill(kit.c.layer_01)
        .border(1.0, kit.c.border_muted)
        .child(caption(title, kit))
        .child(body)
}
