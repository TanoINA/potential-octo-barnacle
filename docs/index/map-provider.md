# Module Guide: `src-tauri/src/agent/provider.rs`

The `provider` module provides a normalized client interface across different LLM backends, translating chat histories, tool schemas, and streaming deltas into unified data structures.

## Core Responsibilities

1. **Provider Adapters**: Translates requests and responses between AetherAgent's internal format and target provider APIs:
   - **OpenAI-Compatible**: Covers standard endpoints (DeepSeek, Ollama, OpenRouter, Groq, custom APIs).
   - **Anthropic**: Translates messages, tool definitions, tool use blocks, and streaming content blocks.
2. **Streaming Execution (`stream_chat_completion`)**: Manages Server-Sent Events (SSE) connections, token buffering, reasoning blocks, and incremental tool-call assembly.
3. **Model Discovery (`fetch_models`)**: Queries remote `/models` endpoints with credentials and maps capabilities.
4. **Error Handling**: Distinguishes transient, retryable network or rate-limit errors (`is_retryable_provider_error`) from fatal authentication or configuration failures.
5. **Multimodal Input Processing**: Validates incoming images (format, dimensions, payload size) and formats them into provider-compatible structures.

## Key Types and Functions

- `StreamResult`: Normalized enum emitted during generation (`Token`, `ReasoningDelta`, `ToolCallDelta`, `Completed`).
- `CompletionOptions`: Carries runtime parameters including temperature, thinking budget, and reasoning effort.
- `stream_chat_completion`: Primary entry point for executing chat turns and yielding async event streams.
- `fetch_models`: Discovers available models for a given provider configuration.
- `assistant_message`: Assembles final model output and completed tool calls into stored history messages.
