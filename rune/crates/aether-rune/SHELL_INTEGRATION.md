# Shell/controller handshake

Shell owns AppState with public controller, ui (panels::UiState), rich, and a separate shell: ShellState (persistent native composer/search TextEdit, timeline layout and sidebar state). The panel-owned UiState is NOT redeclared.

Expected controller integration: new(), model(), model_mut() (or set_draft), set_waker(Arc<dyn Fn()+Send+Sync>), bootstrap(), poll()/drain(), new_session(), open_session(&str), load_older(), send()/send_message(provider?), stop(), dispatch(&str, Value). Final shell will adapt to actual signatures when controller.rs is present. No state crate changes by shell.

Native dialogs and clipboard use Rune Tx and ClipboardContent, then controller attachment preparation. Full app headless runtime tests will execute real controller with isolated profile and no API calls. Explicit --smoke uses a unique temporary profile.
