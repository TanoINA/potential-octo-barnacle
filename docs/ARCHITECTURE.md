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

| Producer | Compact chat representation | Sidecar representation |
| --- | --- | --- |
| `read_file` | path and line-range summary | numbered code viewer |
| `edit_file` | replacement summary | unified diff viewer |
| `run_command` | command status | streaming terminal output |
| `fetch_web_page` | URL and extraction summary | Markdown/text viewer |
| directory/search tools | result count | bounded tree/search output |

## Persistence

Configuration and sessions use version-tolerant Serde structures and atomic replacement writes. Existing early AetherAgent session files are migrated when loaded. Session IDs are validated before becoming filenames, and workspace selection is persisted separately from provider credentials.

## Rendering discipline

Chat rows and large artifact line collections are windowed. Event payloads are scoped, and streaming text is coalesced to animation frames rather than triggering a React commit for every individual token. The app avoids continuous canvas or GPU-animation loops.

Virtualizing the DOM keeps mounted nodes under control, but overall WebView2 memory and frame rates still depend on host hardware and should be tested on target Windows machines.

## Cancellation and cleanup

Cancellation is scoped to a run. It interrupts provider streaming and terminal execution, marks unfinished tool calls appropriately, emits a final done event, and cleans up MCP child processes. A stale run cannot mutate a newly selected session in the UI.
