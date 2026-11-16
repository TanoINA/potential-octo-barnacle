# Module map: `src-tauri/src/agent/runner.rs`

**5477 lines total = 2962 authored + 2515 test** (46% tests)

59 authored top-level items · 32 functions · 3 test block(s)

**Public surface:** 1 `pub fn`, of which **1 are referenced outside this file**.

Internal-only functions (0 external refs): **31**.

**Reconciliation:** item spans sum to 2938 lines vs 2962 authored lines → `OK`

## Test-only spans

`#[cfg(test)]` / `mod tests` spans (excluded from all clusters below):

- line **2671** — `cfg(test)`
- line **2971** — `cfg(test)`
- line **2972** — `mod tests`

## Clusters by domain keyword

| domain | fns | ~LOC | fns used externally | ext hits | sample |
| --- | --- | --- | --- | --- | --- |
| `core` | 26 | 1229 | 1 | 2 | dispatch_tool_batch, cancelled_call, foreground_skill_creation_root, selected_skill_names, load_selected_skill_context |
| `review` | 3 | 61 | 0 | 0 | is_review_skill_tool, review_provider, completed_review_snapshot |
| `workspace` | 2 | 37 | 0 | 0 | subagent_workspace, load_workspace_memory |
| `artifact` | 1 | 5 | 0 | 0 | scope_subagent_artifact |

## Largest single functions

| line | fn | ~LOC | ext refs |
| --- | --- | --- | --- |
| 678 | `run_subagent` | 460 | 0 |
| 2732 | `valid_provider_messages` | 118 | 0 |
| 2539 | `run_reflection` | 67 | 0 |
| 2850 | `append_history_message` | 56 | 2 |
| 2906 | `append_history_segment` | 56 | 0 |
| 2680 | `apply_micro_compaction` | 52 | 0 |
| 399 | `activate_skill` | 42 | 0 |
| 441 | `dispatch_subagent_tool` | 41 | 0 |
| 67 | `cancelled_call` | 39 | 0 |
| 2606 | `completed_review_snapshot` | 36 | 0 |
| 615 | `subagent_system_prompt` | 34 | 0 |
| 348 | `load_selected_skill_context` | 31 | 0 |
| 28 | `dispatch_tool_batch` | 30 | 0 |
| 649 | `subagent_synthesis_failure` | 29 | 0 |
| 2642 | `estimate_replay_tokens` | 29 | 0 |

## External surface (functions referenced from other files)

| line | fn | hits | referenced in |
| --- | --- | --- | --- |
| 2850 | `append_history_message` | 2 | lib.rs |

## Types declared

```
24  const SUBAGENT_MAX_TURNS
25  const TOOL_BATCH_CONCURRENCY
26  const LEAN_WORKFLOW
58  struct PreparedCall
106  struct EmitterContext
217  struct ForegroundSkillTools
322  struct SubagentToolOutcome
388  struct SkillActivation
507  struct SubagentParams
1138  const MAX_WORKSPACE_MEMORY_BYTES
1163  struct ReviewState
1170  struct ReviewCadence
1198  struct UpdateGate
1204  struct NativeWorkLease
1220  pub struct UpdatePermit
1233  pub struct AgentRunner
2469  struct ReflectionLease
2507  struct ReflectionContext
```
