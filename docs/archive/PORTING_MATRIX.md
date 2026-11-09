# Porting and Verification Matrix

This matrix is checked against the implementation and release quality gates. It distinguishes architectural completion from machine-specific performance measurements.

| Area | Hermes reference | AetherAgent target | Acceptance evidence |
| --- | --- | --- | --- |
| Model discovery | `apps/desktop/src/components/model-picker.tsx`, model-options providers | Model picker + Rust provider model commands | Search/filter, refresh, custom model, favorites, current selection |
| Reasoning controls | Desktop model/status controls | Per-session thinking and effort override | Toggle, seven effort values, settings-default reset, request payload |
| OpenAI-compatible streaming | Provider adapters | Rust OpenAI-compatible adapter + SSE decoder | Text/reasoning/tool argument delta tests |
| Anthropic streaming | Provider adapters | Rust Anthropic messages adapter | Payload conversion and content-block delta tests |
| DeepSeek | Provider configuration | OpenAI-compatible adapter + reasoning parser | Model discovery and `reasoning_content` handling |
| Ollama | Local provider routes | OpenAI-compatible adapter + native tags fallback | `/v1` chat and `/api/tags` discovery |
| File reads | `tools/file_tools.py` | Native Rust `read_file` | Range options, bounded output, sidecar artifact over 25 lines |
| File writes | `tools/file_tools.py` | Native Rust `write_file` | Automatic parent creation and contained paths |
| Exact edits | `patch_tool` / file operations | Native Rust `edit_file` | Unique-match guard, replace-all option, unified diff |
| Directory listing | File operation search/list support | Native Rust `list_directory` | Sorted bounded tree output |
| Workspace search | `SearchMixin` | Native Rust `search_files` | Regex/glob filters, result caps, binary/oversize guards |
| Terminal | `tools/terminal_tool.py` | Tokio PowerShell runner | Live stdout/stderr artifacts, exit status, run cancellation |
| Web extraction | `tools/web_tools.py` | Rust fetch/extract tool | HTTP validation, timeout/size caps, text/Markdown artifact |
| Skills | `skills/` loader and prompt catalog | Rust `SKILL.md` loader | YAML name/description parsing and system-prompt catalog |
| MCP | `optional-mcps/` integrations | JSON-RPC stdio client | Initialize, initialized notification, tools/list, tools/call, cleanup |
| Sessions | Gateway/state persistence | Atomic JSON below `~/.aether-agent` | Load/save/search/title/rename/delete/export and legacy migration |
| Workspace | Gateway workspace/session policy | Persisted workspace + native picker | CWD display/switch and contained native tools |
| Chat rendering | Desktop chat | TanStack Virtual | Bounded mounted rows and dynamic measurement |
| Artifact sidecar | Desktop artifact workflows | Code/Diff/Terminal viewers | Long output routing, copy, line numbers, diff highlighting |
| Composer | Desktop command/context workflows | Smart composer | Slash commands and workspace-file mentions |
| Release | Desktop packaging | Tauri Windows release | Frontend build, Rust tests/check, release build, copied EXE hash |

## Machine-specific measurements

The implementation can bound DOM node counts and avoid continuous animation loops. A universal claim of 60 FPS or total WebView2 memory below 60 MB requires measurement on the target machine and workload; compilation alone is not evidence for those metrics.
