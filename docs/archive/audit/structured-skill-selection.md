# Structured skill selection

## Implemented

Skill selection no longer inserts a `Please follow...` prompt template. The composer keeps a separate selected-skill list and sends `selectedSkills` on the user turn.

- Searchable, keyboard-accessible picker with loading/error/empty states, bounded scrolling, descriptions and source-path tooltip.
- Up to five skills; removable composer chips. Selection is for the next message, not a hidden persistent session setting.
- Slash autocomplete and leading `/skill-name task` resolve installed skill names into metadata. Built-in names keep their command identity. Remaining slash-like task text after a skill prefix is not dispatched as a destructive local command. An explicit `/stop` remains reachable.
- Accepted send clears its unchanged skill draft. Rejected send keeps it. Queue snapshots selections; Edit queue, Undo, Retry and Edit/Resend preserve them. User message shows a skill receipt separately from copied text.
- Session/workspace navigation clears unsent selections and reloads discovery; stale discovery responses are ignored. Native `list_skills` accepts the expected workspace, using the same effective roots as the run. Council receives the configured skill creation directory too.
- Rust resolves names from the trusted catalog and reads validated `SKILL.md` contents before MCP/provider execution. Both Council candidates receive the same loaded context. Missing/unreadable/invalid selection fails visibly before provider execution.
- Only the latest user turn's explicit selection is loaded. Historical names are not reloaded or broken by a removed old skill. No selected-skill body is written into visible user text or the stored system prompt.
- Native loading is limited to five submitted names and 256 KiB aggregate instructions. Activation events emit only after all selected files pass. Existing images and native provider transcript remain preserved.

No new dependency, settings screen, skill marketplace, persistence migration, or Rune change.

## Evidence

- [Focused tests](file:///D:/AetherAgent/verification/skill-selection/resume-focused.log): 54 passed for composer/picker/hook before packaging.
- [Native full suite](file:///D:/AetherAgent/verification/skill-selection/native-full.log): 431 passed, 0 failed, 2 ignored subprocess fixtures.
- [Production Chromium report](file:///D:/AetherAgent/verification/skill-selection/browser/report.json): 7 passed, 0 failed; 101 catalog entries at 900x600, result list 210px, no horizontal overflow, search focus/ArrowDown/Enter/Escape, unchanged text, real hook outgoing metadata and visible receipt; zero JS errors/unexpected IPC.
- [Picker screenshot](file:///D:/AetherAgent/verification/skill-selection/browser/picker-900x600.png).
- [Sent-message screenshot](file:///D:/AetherAgent/verification/skill-selection/browser/selected-message-900x600.png).
- [Source fingerprints](file:///D:/AetherAgent/verification/skill-selection/source-manifest.json).
- [Diff check](file:///D:/AetherAgent/verification/skill-selection/diff-check.log): no whitespace errors.

The old full-suite failure and earlier focused failures remain as evidence, not final status: they caught missing metadata lifecycle behavior, a command-dispatch/Council root issue, an ambiguous test selector, and a test incorrectly waiting for automatic discovery while streaming. Each was inspected and fixed before the final run. The browser's initial run used a blocked localhost port; its final run uses an available preview port without weakening assertions.

## Final packaging

- Final full frontend regression: **714 passed, 65 test files**, exit 0. [Log](file:///D:/AetherAgent/verification/skill-selection/frontend-final.log).
- Final native suite: **431 passed, 0 failed, 2 ignored**. [Log](file:///D:/AetherAgent/verification/skill-selection/native-full.log).
- Windows release: **build exit 0**, NSIS/MSI/standalone compiled and archived. [Build log](file:///D:/AetherAgent/release/windows/0.3.0/20261109T235557-skill-selection/build.log). Startup isolated profile alive at12s with AetherAgent window title. [Startup result](file:///D:/AetherAgent/release/windows/0.3.0/20261109T235557-skill-selection/startup-smoke.json).
- [Windows EXE installer](file:///D:/AetherAgent/release/windows/0.3.0/20261109T235557-skill-selection/AetherAgent_0.3.0_x64-setup.exe), [MSI](file:///D:/AetherAgent/release/windows/0.3.0/20261109T235557-skill-selection/AetherAgent_0.3.0_x64_en-US.msi), [standalone executable](file:///D:/AetherAgent/release/windows/0.3.0/20261109T235557-skill-selection/aether-agent.exe).
- Linux release: **build exit 0**, DEB/RPM/AppImage/native compiled, package metadata checked and source snapshot validated. [Build status](file:///D:/AetherAgent/release/linux/0.3.0/20261109T165604Z-activity-update/build-status.txt). The reused output suffix says activity-update; the timestamp/source snapshot identifies this new skill-selection build, not the older activity release.
- [Linux DEB](file:///D:/AetherAgent/release/linux/0.3.0/20261109T165604Z-activity-update/AetherAgent_0.3.0_amd64.deb), [RPM](file:///D:/AetherAgent/release/linux/0.3.0/20261109T165604Z-activity-update/AetherAgent-0.3.0-1.x86_64.rpm), [AppImage](file:///D:/AetherAgent/release/linux/0.3.0/20261109T165604Z-activity-update/AetherAgent_0.3.0_amd64.AppImage).
- SHA-256: all three Windows and all four Linux artifacts rechecked after packaging. [Windows manifest](file:///D:/AetherAgent/release/windows/0.3.0/20261109T235557-skill-selection/artifact-manifest.json), [Linux checksums](file:///D:/AetherAgent/release/linux/0.3.0/20261109T165604Z-activity-update/SHA256SUMS).
- Source fingerprint recheck: **151 files, zero mismatches** after build; Linux allowlisted snapshot also matches current source.
- Linux isolated startup smoke: **three passed** (native ELF, extracted DEB, extracted AppImage). Each opened an AetherAgent window, survived10s, then was deliberately stopped at15s; timeout124 is expected. [Log](file:///D:/AetherAgent/release/linux/0.3.0/20261109T165604Z-activity-update/startup-smoke.log). Xvfb/temp profiles/mount namespace; no system package installed. Root sandbox override only inside smoke processes, not app defaults.
- Installed application and production update endpoint have not been changed. Verification preview server was stopped.

## Limits

Production browser uses in-memory IPC fixtures, not paid/live provider calls. Native tests inspect actual skill loading/provider conversation assembly and failure ordering without a paid provider. The selected skill is not automatically inherited by a child subagent; the agent can explicitly activate its applicable skills through the existing tool.

Packages are local unsigned builds. Updater identity/hosting remain intentionally unconfigured. Linux native requires GLIBC >=2.39 with its GTK/WebKit dependencies; the Arch-built AppImage carries dependencies requiring GLIBC >=2.44, not universal distro compatibility. Full native suite ran on Windows; Linux startup/package metadata checks are separate.
