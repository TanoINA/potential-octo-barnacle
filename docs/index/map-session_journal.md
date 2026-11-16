# Module map: `src-tauri/src/agent/session_journal.rs`

**2161 lines total = 808 authored + 1353 test** (63% tests)

24 authored top-level items · 22 functions · 2 test block(s)

**Public surface:** 0 `pub fn`, of which **2 are referenced outside this file**.

Internal-only functions (0 external refs): **20**.

**Reconciliation:** item spans sum to 792 lines vs 808 authored lines → `OK`

## Test-only spans

`#[cfg(test)]` / `mod tests` spans (excluded from all clusters below):

- line **809** — `cfg(test)`
- line **810** — `mod tests`

## Clusters by domain keyword

| domain | fns | ~LOC | fns used externally | ext hits | sample |
| --- | --- | --- | --- | --- | --- |
| `core` | 18 | 577 | 2 | 243 | active, register_active, bases, hash, bytes |
| `artifact` | 3 | 183 | 0 | 0 | artifact_grows, artifact_grows_at_revisions, validate_artifacts |
| `checkpoint` | 1 | 10 | 0 | 0 | checkpoint_counter |

## Largest single functions

| line | fn | ~LOC | ext refs |
| --- | --- | --- | --- |
| 537 | `active_record` | 272 | 0 |
| 379 | `validate_artifacts` | 158 | 0 |
| 157 | `tool_grows` | 58 | 0 |
| 280 | `message_grows_at_epochs` | 50 | 0 |
| 88 | `write` | 33 | 146 |
| 229 | `grows` | 33 | 0 |
| 136 | `narrative` | 18 | 0 |
| 362 | `artifact_grows_at_revisions` | 17 | 0 |
| 73 | `read` | 15 | 97 |
| 121 | `guard` | 15 | 0 |
| 215 | `tools_grow` | 14 | 0 |
| 341 | `validate_messages` | 13 | 0 |
| 330 | `snapshot_message_grows` | 11 | 0 |
| 262 | `checkpoint_counter` | 10 | 0 |
| 39 | `active` | 9 | 0 |

## External surface (functions referenced from other files)

| line | fn | hits | referenced in |
| --- | --- | --- | --- |
| 88 | `write` | 146 | git.rs, images.rs, mcp.rs, native_smoke.rs |
| 73 | `read` | 97 | git.rs, images.rs, native_smoke.rs, profile.rs |

## Types declared

```
17  struct Journal
31  struct Baseline
```
