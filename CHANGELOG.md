# Changelog

All notable changes to AetherAgent are documented here.

## [Unreleased]

### Added

- Bundled five lightweight quality and web guides: verification before completion, frontend design, web application testing, accessibility/usability review, and React best practices. Missing guides are seeded locally on startup without network downloads or new runtime dependencies; existing user skill files are preserved.

- Per-session, per-workspace composer drafts retain text, selected skills, and attachments while the app stays open. Late send acknowledgements only clear the originating, unchanged draft.
- Selected skill chips open an accessible preview with description, source, and path; missing metadata is explained without inventing instructions.
- Background task cards can copy the original command, with clipboard success and error feedback.

- **Web search improvements**: Domain scoping (`site:...`), direct page text extraction via `fetch_content`, instant-answer fallbacks, source chips in chat, and a `/search` slash command.
- **Interactive questions**: An `ask_question` tool that renders multi-choice prompt cards directly in chat when the agent needs clarification.
- **Auto-titling**: Sessions are automatically named after the first model response, with manual renames protected and `/title` available on demand.
- **Context meter**: A visual gauge in the composer showing estimated context window usage.
- **In-chat search**: In-session search (`Ctrl+F`) with match highlighting and keyboard navigation.
- **Sidebar pinning**: Pin important chat sessions to the top of the sidebar.
- **New slash commands**: Added `/goal` for unattended task runs and `/grill-me` for design alignment interviews.
- **Settings enhancements**: Provider setup improvements, agent defaults, live MCP server connection testing, and dirty guards for unsaved changes.

### Fixed & Improved

- Duplicate clarification cards no longer appear when the same `ask_question` call is stored in both a message's tool list and its transcript segment.
- Main-agent turns now default to unlimited, with an optional validated maximum in Settings. Explicit saved limits are preserved, including values above 200; cancellation, tool approvals, retries, and separate subagent limits remain unchanged.

- Refined the chat welcome layout, grouped the empty prompt with its composer, and added workspace-aware folder opening. Removed duplicate model labels, clarified reasoning and execution controls, and cleaned Markdown from sidebar previews while preserving safe search highlights.

- Background tasks remain `stopping` until process-tree cleanup completes; repeated stop requests stay idempotent and stopping tasks still count as active.
- Selecting a slash skill removes only its token, preserving surrounding draft text and cursor position.
- Activity tray interaction tests use small fixtures separately from the 100-item stress and navigation checks.
- **Concurrent MCP startup**: MCP servers now initialize concurrently at launch rather than sequentially.
- **SQLite WAL mode**: Enabled Write-Ahead Logging and busy timeouts for smoother database read and write concurrency.
- **Session cleanup**: Deleting a session now cancels its active background tasks and cleans up task tracking.
- **Session titles**: Manually edited session names are flagged so auto-titling will not overwrite them.
- **Environment file parsing**: Improved `.env` parser to cleanly handle Windows CRLF line endings and escaped characters.
- **Search performance**: Added limits and pagination to SQLite full-text search queries.
- **Codebase cleanup**: Modularized the chat header into a dedicated component and cached compiled regexes in tools.
- **Documentation**: Refined and simplified guides across the repository for clearer developer onboarding and release procedures.

## [0.3.0] - 2026-11-09

### Added
- **Session history pagination**: Large sessions load lazily with virtualized scroll anchors, keeping initial load times fast without truncating disk history or context.
- **Self-improvement reviews**: Native Rust review loop that inspects completed sessions and patches or creates local skills, with optional user approval.
- **Multimodal attachments**: Native image pickers, drag-and-drop, clipboard paste, and image previews with provider-specific payload formatting.
- **LaTeX math support**: Clean math rendering using remark-math and KaTeX for inline and block formulas.
- **Queue and controls**: Prompt queue with immediate send, editing, and removal controls, plus per-session status indicators.
- **Offline release verification**: Tools to verify Windows Authenticode signatures, minisign updater signatures, and SHA-256 hashes before installation.

### Changed

- Refined the UI layout: simplified header hierarchy, cleaner composer footer, and reduced motion options.
- Switched session persistence to atomic replacement writes with recovery checkpoint sidecars.
- Hardened tool execution against directory traversal and symlink escapes.
