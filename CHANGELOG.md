# Changelog

Notable changes to AetherAgent are listed here.

## [Unreleased]

## [0.3.0-beta.3] - 2026-11-26

### Fixed
- Replaced all Tailwind CSS-variable opacity modifier classes (`[var(--...)]/NN`, unsupported in Tailwind v3) with explicit `color-mix()` border and surface tokens in `src/index.css` across `FilesChangedCard`, `TerminalOutput`, `ExecutionTrace`, `CouncilView`, `MessageRow`, `Sidebar`, `InteractivePlanCard`, `InteractiveQuestionCard`, `ChatCanvas`, and `App`, eliminating unintended light default borders around diff badges and action buttons.
- Made `+N` / `-N` diff counter pills in `FilesChangedCard` borderless and added a static regression test in `src/components/Fase3Transcript.test.tsx` to catch any reintroduced `[var(--...)]/opacity` classes.

## [0.3.0-beta.2] - 2026-11-25

### Changed & Added

- Added five built-in themes (`Nous Dark`, `Antigravity Graphite`, `Nous Light`, `Catppuccin Mocha`, and `Hermes Ember`) backed by semantic CSS variables, switchable from Settings, the sidebar palette button, or the bottom workspace bar.
- Replaced hardcoded modal and drawer dark styles with theme-aware tokens across chat messages, Markdown output, diff cards, plan cards, Spotlight search, and modals.
- Changed the right artifact pane into a docked, resizable desktop side pane (`280px` to `720px` via pointer drag or `ArrowLeft`/`ArrowRight` on the splitter) while keeping narrow viewports in overlay mode.
- Grouped recent sidebar chats by workspace folder with collapsible headers, simplified header controls, moved live activity, queued prompts, and context status into a collapsible stack above the composer, and added a bottom bar showing workspace, MCP count, skill count, and theme skin.
- Removed the bordered card box around assistant responses so prose renders directly on the canvas surface and tool traces stay quiet by default.

## [0.3.0-beta.1] - 2026-11-23

Unsigned Windows and Linux pre-release focusing on security hardening, persistence reliability, and UI polish.

### Added

- Five bundled quality and web guides (`verification-before-completion`, `frontend-design`, `webapp-testing`, `web-design-guidelines`, and `react-best-practices`) seeded on first launch without touching existing user skills or downloading extra tools.
- Per-session, per-workspace composer drafts that retain text, selected skills, and attachments until their originating send finishes, along with clickable skill metadata popovers and command copy buttons on background task cards.
- Domain scoping (`site:...`), optional `fetch_content` page text extraction, instant-answer fallbacks, clickable source chips, and `/search` for web lookups.
- Multi-choice `ask_question` cards in chat, session auto-titling after the first model reply (leaving manual renames intact) plus `/title`, a context usage meter, `Ctrl+F` in-chat search, sidebar pinning, `/goal`, and `/grill-me`.
- Settings controls for provider presets, optional main-agent turn limits, live MCP server checks, and unsaved-change guards.

### Fixed & Improved

- SVG artifact previews now render inside an empty-sandbox iframe instead of raw DOM markup, HTML previews drop `allow-modals` and `allow-forms`, Tauri enforces an explicit Content Security Policy, and markdown and native openers block obfuscated script URLs and UNC/device targets.
- Network and filesystem validation now blocks local `64:ff9b:1::/48` NAT64 and Teredo client IPv6 bypasses, rejects dangling symlinks in `resolve_workspace_path`, accepts Windows cloud-placeholder tags on real image files, keeps workspace symlinks intact during Git file actions, and tears down full child process trees on both Windows and Unix.
- SQLite full-text search uses WAL mode with busy timeouts, indexes message segments and terminal output without attaching the session title to every message row, and paginates queries within a bounded result budget.
- Session recovery checks every `.checkpoint` file and repairs dirty projections without bumping timestamps, serialized persistence stops auto-title updates from racing final saves, and deleting a session cancels its running background tasks.
- History preview blocks composer sends, drops, pastes, and mutating context menu actions; `MessageRow`, `SettingsModal`, `ChatCanvas`, and `TimelineScrubber` fix hook ordering, track MCP env edits as dirty, and trim redundant work during streaming.
- MCP stdio servers initialize concurrently at launch, `.env` parsing handles CRLF line endings and escapes, `ask_question` calls no longer duplicate between tool lists and segments, background tasks stay `stopping` until process-tree cleanup completes, and main-agent turns default to unlimited (`maxTurns = 0`).
- Removed unused `@tauri-apps/plugin-fs`, `@tauri-apps/plugin-shell`, `clsx`, `tailwind-merge`, and their unused Rust plugins and capabilities; added Rust unit tests to the Windows CI quality matrix, added an MIT `LICENSE` and `SECURITY.md`, and simplified documentation.

## [0.3.0] - 2026-11-09

### Added

- Lazy session history pagination with virtualized scroll anchors so long sessions open quickly without dropping disk history or model context.
- Native Rust self-improvement review loop that inspects completed sessions and proposes or updates local skills with optional approval.
- Native image pickers, drag-and-drop, clipboard paste, and image previews formatted per provider.
- Inline and block math rendering via `remark-math` and KaTeX.
- Prompt queue controls for immediate send, editing, and removal alongside per-session activity status.
- Release verification scripts checking Windows Authenticode signatures, minisign updater signatures, and SHA-256 hashes before installation.

### Changed

- Simplified the chat header hierarchy, composer footer, and reduced-motion styles.
- Switched session writes to atomic replacement with checkpoint sidecars.
- Tightened tool path checks against directory traversal and symlink escapes.
