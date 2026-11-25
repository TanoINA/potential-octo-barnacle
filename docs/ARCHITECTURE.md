# AetherAgent Architecture

## Runtime boundaries

AetherAgent separates untrusted model/provider output from operating-system side effects:

1. React creates an `AgentRunRequest` with a unique run ID and session ID.
2. Tauri validates the selected provider, workspace, and run ownership.
3. `AgentRunner` builds the model conversation and injects the native/skill/MCP tool catalog.
4. Provider adapters stream normalized text, reasoning, and tool-call deltas.
5. Tool calls are validated again at the native execution boundary.
6. Large outputs become artifacts; compact status updates remain in chat.
7. Session state is persisted below the user-private `.aether-agent` directory.

Every streamed event carries both run and session IDs so the UI can discard stale events after cancellation or session navigation.

## Provider normalization

The OpenAI-compatible adapter covers GutsAI, DeepSeek, Ollama's compatibility API, and custom endpoints. The Anthropic adapter translates chat history, system messages, tool definitions, tool calls, and streaming content-block events. Provider-specific headers and body extensions remain available for compatible services without requiring a frontend change.

## Artifact routing

| Producer | Compact chat summary | Docked side pane |
| --- | --- | --- |
| `read_file` | path and line-range summary | numbered code viewer |
| `edit_file` | replacement summary | unified diff viewer |
| `run_command` | command status | streaming terminal output |
| `fetch_web_page` | URL and extraction summary | Markdown/text viewer |
| directory/search tools | result count | bounded tree/search output |

On desktop viewports, the side pane docks next to the transcript and remembers its resized width (`280px`–`720px`); on narrow viewports it opens as an overlay drawer.

## Persistence

Configuration and session files use version-tolerant Serde schemas with atomic file replacement. Older session files migrate automatically on load, session IDs undergo filename validation before touching disk, and workspace folders stay stored separately from provider API keys.

## Rendering discipline

Long chat transcripts and large artifact files virtualize their visible rows in the DOM. Streamed tokens batch onto animation frames instead of forcing a full React re-render per chunk, and the UI avoids continuous background GPU loops or heavy blur filters. DOM windowing keeps node counts low, though actual WebView2 memory and frame times still vary by hardware and deserve quick checks on target Windows and Linux setups.

## Cancellation and cleanup

Cancelling a run targets only that run's ID: it aborts provider streams and child shells, marks interrupted tools as cancelled, emits a final completion event, and cleans up MCP child processes. Background events from an older session cannot mutate whichever thread you switched to.
