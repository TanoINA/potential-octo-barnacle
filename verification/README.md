# Release UI verification evidence

The final PNG files in this directory are window-only captures of the Tauri custom-protocol release build.

- `aether-release-custom.png`: loaded production shell, workspace, model, sessions, and composer.
- `aether-thinking-final.png`: model picker and all seven reasoning-effort options.
- `aether-composer-final.png`: slash commands and corrected composer footer layout.
- `aether-mentions-final.png`: filtered workspace-file mentions.

Managed Windows UI Automation was also used to open the model picker and Settings, select and reset a session model override, exercise composer popups, and verify favorite persistence. No external provider credentials were used.

These captures demonstrate rendering and interaction at the captured window size. They are not frame-rate or whole-process-memory benchmarks.
