# Module map: `src-tauri/src/agent/tools.rs`

**2526 lines total = 1793 authored + 733 test** (29% tests)

40 authored top-level items · 30 functions · 5 test block(s)

**Public surface:** 5 `pub fn`, of which **5 are referenced outside this file**.

Internal-only functions (0 external refs): **25**.

**Reconciliation:** item spans sum to 1771 lines vs 1793 authored lines → `OK`

## Test-only spans

`#[cfg(test)]` / `mod tests` spans (excluded from all clusters below):

- line **637** — `cfg(test)`
- line **1022** — `cfg(test)`
- line **1029** — `cfg(test)`
- line **1808** — `cfg(test)`
- line **1809** — `mod tests`

## Clusters by domain keyword

| domain | fns | ~LOC | fns used externally | ext hits | sample |
| --- | --- | --- | --- | --- | --- |
| `core` | 18 | 1022 | 3 | 11 | get_agent_tools, execute_tool, required_string, optional_string, bounded_chars |
| `file` | 5 | 378 | 0 | 0 | read_file, lock_file_mutation, write_file, edit_file, search_files |
| `workspace` | 2 | 109 | 2 | 15 | resolve_workspace_path, list_workspace_files |
| `dir` | 1 | 97 | 0 | 0 | list_directory |
| `path` | 1 | 43 | 0 | 0 | validate_relative_path |
| `atomic` | 1 | 31 | 0 | 0 | guarded_atomic_write |
| `artifact` | 1 | 19 | 0 | 0 | new_artifact |
| `review` | 1 | 8 | 0 | 0 | review_tool_definitions |

## Largest single functions

| line | fn | ~LOC | ext refs |
| --- | --- | --- | --- |
| 1034 | `run_command` | 235 | 0 |
| 86 | `get_agent_tools` | 222 | 5 |
| 833 | `search_files` | 122 | 0 |
| 1431 | `fetch_web_page` | 105 | 0 |
| 385 | `read_file` | 100 | 0 |
| 736 | `list_directory` | 97 | 0 |
| 643 | `edit_file` | 93 | 0 |
| 1581 | `is_public_ip` | 75 | 0 |
| 1358 | `manage_task` | 73 | 0 |
| 1749 | `list_workspace_files` | 59 | 2 |
| 1656 | `resolve_workspace_path` | 50 | 13 |
| 977 | `spawn_process_reader` | 45 | 0 |
| 1536 | `validate_public_url` | 45 | 0 |
| 1706 | `validate_relative_path` | 43 | 0 |
| 583 | `write_file` | 36 | 0 |

## External surface (functions referenced from other files)

| line | fn | hits | referenced in |
| --- | --- | --- | --- |
| 1656 | `resolve_workspace_path` | 13 | git.rs, reflection.rs, runner.rs, skills.rs |
| 86 | `get_agent_tools` | 5 | review.rs, runner.rs, lib.rs |
| 308 | `execute_tool` | 4 | pty.rs, runner.rs |
| 1303 | `terminate_child` | 2 | tasks.rs |
| 1749 | `list_workspace_files` | 2 | lib.rs |

## Types declared

```
19  const MAX_FILE_BYTES
20  const MAX_TOOL_CHARS
21  const MAX_COMMAND_BYTES
22  const MAX_WEB_BYTES
25  pub enum ToolEvent
34  pub type ToolEventSink
37  pub struct ToolContext
45  pub struct ToolExecutionResult
955  struct ProcessLine
```
