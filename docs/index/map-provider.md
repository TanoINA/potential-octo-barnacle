# Module map: `src-tauri/src/agent/provider.rs`

**2589 lines total = 1850 authored + 739 test** (29% tests)

57 authored top-level items · 38 functions · 5 test block(s)

**Public surface:** 4 `pub fn`, of which **11 are referenced outside this file**.

Internal-only functions (0 external refs): **27**.

**Reconciliation:** item spans sum to 1832 lines vs 1850 authored lines → `OK`

## Section markers found in file

| line | label |
| --- | --- |
| 952 | Only documented aliases and Anthropic's -YYYYMMDD snapshot shape opt into |
| 1019 | Accept the public alias or its dated snapshot, not an arbitrary model with |

## Test-only spans

`#[cfg(test)]` / `mod tests` spans (excluded from all clusters below):

- line **30** — `cfg(test)`
- line **34** — `cfg(test)`
- line **1855** — `cfg(test)`
- line **1859** — `cfg(test)`
- line **1860** — `mod tests`

## Clusters by domain keyword

| domain | fns | ~LOC | fns used externally | ext hits | sample |
| --- | --- | --- | --- | --- | --- |
| `core` | 33 | 1382 | 11 | 38 | validate_stream_result, fetch_models, fetch_models_from, collect_model_ids, is_retryable_provider_error |
| `image` | 4 | 84 | 0 | 0 | validate_image_data, validate_image_url, image_source_from_url, image_source_url |
| `lock` | 1 | 55 | 0 | 0 | content_to_blocks |

## Largest single functions

| line | fn | ~LOC | ext refs |
| --- | --- | --- | --- |
| 336 | `stream_chat_completion` | 291 | 10 |
| 1424 | `process_anthropic_event` | 153 | 1 |
| 717 | `normalize_openai_messages` | 119 | 2 |
| 1088 | `convert_anthropic_messages` | 77 | 7 |
| 627 | `openai_body` | 67 | 5 |
| 1362 | `process_openai_delta` | 62 | 1 |
| 1577 | `process_anthropic_message` | 58 | 1 |
| 855 | `anthropic_body` | 55 | 2 |
| 1256 | `content_to_blocks` | 55 | 0 |
| 209 | `fetch_models_from` | 44 | 0 |
| 981 | `anthropic_payload_capabilities` | 40 | 0 |
| 268 | `is_retryable_provider_error` | 37 | 2 |
| 1038 | `openai_reasoning_effort` | 35 | 0 |
| 1165 | `validate_image_data` | 32 | 0 |
| 1671 | `request_headers` | 32 | 0 |

## External surface (functions referenced from other files)

| line | fn | hits | referenced in |
| --- | --- | --- | --- |
| 336 | `stream_chat_completion` | 10 | provider_payload_audit_tests.rs, reflection.rs, review.rs, runner.rs |
| 1088 | `convert_anthropic_messages` | 7 | provider_payload_audit_tests.rs, provider_state_audit_tests.rs |
| 967 | `supports_thinking_off` | 6 | provider_payload_audit_tests.rs, reflection.rs, review.rs |
| 627 | `openai_body` | 5 | provider_payload_audit_tests.rs |
| 268 | `is_retryable_provider_error` | 2 | runner.rs |
| 717 | `normalize_openai_messages` | 2 | provider_payload_audit_tests.rs |
| 855 | `anthropic_body` | 2 | provider_payload_audit_tests.rs |
| 183 | `fetch_models` | 1 | lib.rs |
| 1362 | `process_openai_delta` | 1 | provider_state_audit_tests.rs |
| 1424 | `process_anthropic_event` | 1 | provider_state_audit_tests.rs |
| 1577 | `process_anthropic_message` | 1 | provider_state_audit_tests.rs |

## Types declared

```
14  pub struct ChatCompletionToolCall
25  pub struct ChatCompletionFunctionCall
39  pub struct StreamResult
70  pub struct ToolDelta
79  pub struct CompletionOptions
88  struct ToolBuilder
95  struct StreamAccumulator
148  const COMPLETE_FINISH_REASONS
263  pub const PROVIDER_STALE_TIMEOUT
264  const MAX_PROVIDER_RESPONSE_BYTES
910  enum AnthropicThinkingOff
917  enum AnthropicEffortSupport
924  struct AnthropicPayloadCapabilities
1730  const SSE_EVENT_TOO_LARGE
1733  struct SseDecoder
```
