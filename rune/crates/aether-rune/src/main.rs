//! Windows uses Rune/winit/wgpu directly; no Tauri/WebView is linked.
use aether_core::agent::profile;
use aether_rune::{app, cli::Options, desktop, lifecycle};
use std::sync::Arc;
fn main() {
    if let Err(error) = start() {
        eprintln!("Aether native startup: {error}");
        std::process::exit(1);
    }
}
fn native_app(member: Option<(usize, lifecycle::CloseGroup)>) -> rune::App<app::AppState> {
    let primary = member.as_ref().is_none_or(|(id, _)| *id == 0);
    let headless = rune::is_headless();
    let app = rune::App::new(app::spec())
        .gpu_backend(rune::GpuBackend::Wgpu)
        .keep_running()
        .on_close(lifecycle::on_close)
        .on_quit(lifecycle::on_quit)
        .on_focus(lifecycle::on_focus);
    let app = if desktop::should_register_shortcut(primary, headless) {
        app.global_shortcut("alt+space", desktop::toggle_history)
    } else {
        app
    };
    let errors = app.handle();
    let activation = app.handle();
    // Requests queued before run install the same-window reopen callback.
    let open = app.handle();
    let hide = app.handle();
    app.handle().update(move |s, _| {
        s.shell.window_open = Some(Arc::new(move || open.open()));
        s.shell.window_hide = Some(Arc::new(move || hide.hide()));
        s.shell.close_group = member;
        if !headless {
            // This Rune revision exposes take_shortcut_errors, not on_shortcut_error.
            s.shell.desktop.install_native(
                Arc::new(move || {
                    errors
                        .take_shortcut_errors()
                        .into_iter()
                        .map(|error| format!("Global Alt+Space unavailable: {error}"))
                        .collect()
                }),
                Arc::new(move |token| activation.activate(token)),
            );
        }
    });
    app
}
fn start() -> Result<(), String> {
    let options = Options::parse(rune::extra_args())?;
    let home = dirs::home_dir().ok_or("Home directory unavailable; refusing profile fallback")?;
    let requested = if options.smoke {
        Some(std::env::temp_dir().join(format!("aether-rune-smoke-{}", uuid::Uuid::new_v4())))
    } else {
        options.profile
    };
    let selected = profile::resolve_root(&home, requested.as_deref())?;
    if let Some(source) = options.import_profile {
        profile::import_profile(&source, &selected)?;
    }
    profile::configure_storage_root(&selected)?;
    rune::set_app_id("dev.aether.agent.rune");
    if options.smoke {
        let mut state = app::AppState::new();
        let mut tx = rune::Tx::default();
        state.new_session(&mut tx);
        let current = state
            .controller
            .model()
            .current
            .as_ref()
            .ok_or("New chat did not create a session")?;
        println!(
            "Native shell smoke OK: profile={} session={}",
            selected.display(),
            current.session.id
        );
        return Ok(());
    }
    if options.two_windows {
        rune::run_windows(|_| {
            let group = lifecycle::close_group(2);
            vec![
                rune::WindowSpec::from_app("main", native_app(Some((0, group.clone())))),
                rune::WindowSpec::from_app("second", native_app(Some((1, group)))),
            ]
        });
    } else {
        native_app(None).run();
    }
    Ok(())
}
