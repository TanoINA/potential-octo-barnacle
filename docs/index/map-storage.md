# Module map: `src-tauri/src/agent/storage.rs`

**5861 lines total = 2727 authored + 3134 test** (53% tests)

101 authored top-level items · 93 functions · 7 test block(s)

**Public surface:** 38 `pub fn`, of which **35 are referenced outside this file**.

Internal-only functions (0 external refs): **58**.

**Reconciliation:** item spans sum to 2694 lines vs 2727 authored lines → `OK`

## Section markers found in file

| line | label |
| --- | --- |
| 55 | Journal fixtures use explicit per-session roots without mutating process-global |

## Test-only spans

`#[cfg(test)]` / `mod tests` spans (excluded from all clusters below):

- line **57** — `cfg(test)`
- line **61** — `cfg(test)`
- line **297** — `cfg(test)`
- line **356** — `cfg(test)`
- line **762** — `cfg(test)`
- line **2750** — `cfg(test)`
- line **2751** — `mod tests`

## Clusters by domain keyword

| domain | fns | ~LOC | fns used externally | ext hits | sample |
| --- | --- | --- | --- | --- | --- |
| `core` | 23 | 917 | 2 | 5 | provider, migrate_provider_kinds, validate_safe_id, canonical_sha256, substantive_changed |
| `session` | 28 | 777 | 11 | 68 | sessions_dir, session_path, list_sessions, visit_session_directory, load_session |
| `file` | 2 | 122 | 0 | 0 | recover_save_ack_file, hydrate_run_history_file |
| `review` | 4 | 121 | 1 | 2 | review_key, merge_review_events, resolve_review_proposal, resolve_review_proposal_file |
| `pending` | 3 | 112 | 1 | 1 | attach_pending_link, load_pending_link, attach_pending_link_file |
| `settings` | 4 | 111 | 3 | 9 | settings_path, default_settings, load_settings, save_settings |
| `session_page` | 4 | 77 | 3 | 32 | load_session_page, save_session_page_file, save_session_page, checkpoint_session_page |
| `artifact` | 3 | 60 | 2 | 6 | list_all_artifacts, list_all_artifacts_in_dir, read_session_artifact |
| `workspace` | 6 | 58 | 4 | 7 | workspace_path, load_workspace_sync, clear_workspace, clear_workspace_file, canonical_workspace |
| `self_improvement` | 3 | 54 | 1 | 5 | merge_self_improvements, record_self_improvement, record_self_improvement_file |
| `path` | 2 | 47 | 1 | 2 | tool_call_path, inspect_paths |
| `export` | 2 | 46 | 0 | 0 | resolve_export_path, export_path_key |
| `dir` | 3 | 43 | 1 | 24 | config_dir, dirty_projections, repair_dirty_projections |
| `atomic` | 1 | 27 | 1 | 11 | atomic_write |
| `image` | 1 | 21 | 1 | 3 | read_message_image_inputs |
| `checkpoint` | 2 | 18 | 2 | 44 | checkpoint_session, checkpoint_session_delta |
| `ack` | 1 | 8 | 1 | 2 | recover_save_ack |
| `pin` | 1 | 8 | 0 | 0 | merge_pin_state |

## Largest single functions

| line | fn | ~LOC | ext refs |
| --- | --- | --- | --- |
| 1753 | `render_standalone_html` | 152 | 0 |
| 1408 | `deterministic_summary` | 120 | 0 |
| 422 | `read_session_file_raw` | 118 | 0 |
| 2506 | `refresh_session_window` | 105 | 2 |
| 2425 | `normalize_for_page` | 81 | 0 |
| 2023 | `save_attachment` | 78 | 2 |
| 1310 | `ordered_transcript` | 77 | 0 |
| 1089 | `save_session_file_with_previous` | 74 | 0 |
| 906 | `attach_pending_link_file` | 72 | 0 |
| 2620 | `hydrate_run_history_file` | 72 | 0 |
| 679 | `replace_session_checked_file` | 66 | 0 |
| 987 | `resolve_review_proposal_file` | 64 | 0 |
| 1528 | `compact_data` | 62 | 0 |
| 302 | `visit_session_directory` | 54 | 0 |
| 2270 | `validate_window` | 54 | 0 |

## External surface (functions referenced from other files)

| line | fn | hits | referenced in |
| --- | --- | --- | --- |
| 2714 | `checkpoint_session_delta` | 42 | native_smoke.rs, runner.rs, session_journal.rs, lib.rs |
| 395 | `load_session` | 27 | native_smoke.rs, runner.rs, session_journal.rs, lib.rs |
| 29 | `config_dir` | 24 | history_maintenance.rs, native_smoke.rs, profile.rs, release.rs |
| 2172 | `load_session_page` | 21 | native_smoke.rs, runner.rs, session_journal.rs, lib.rs |
| 659 | `save_session` | 17 | native_smoke.rs, runner.rs, session_journal.rs, lib.rs |
| 1905 | `atomic_write` | 11 | review_pending.rs, session_journal.rs, skills.rs, tools.rs |
| 2414 | `save_session_page` | 9 | runner.rs, session_journal.rs, lib.rs |
| 135 | `load_settings` | 6 | profile.rs, runner.rs, lib.rs |
| 829 | `record_self_improvement` | 5 | runner.rs, session_journal.rs |
| 1249 | `toggle_pin_session` | 4 | session_journal.rs, lib.rs |
| 1262 | `delete_session` | 4 | session_journal.rs, lib.rs |
| 1961 | `canonical_workspace` | 4 | lib.rs |
| 2194 | `load_session_around` | 4 | session_journal.rs, lib.rs |
| 2242 | `read_session_artifact` | 4 | session_journal.rs, lib.rs |
| 2221 | `read_message_image_inputs` | 3 | session_pages.rs, lib.rs |
| 2692 | `hydrate_run_history` | 3 | lib.rs |
| 205 | `save_settings` | 2 | lib.rs |
| 271 | `list_sessions` | 2 | lib.rs |
| 363 | `list_all_artifacts` | 2 | lib.rs |
| 601 | `recover_save_ack` | 2 | lib.rs |
| 745 | `checkpoint_session` | 2 | lib.rs |
| 978 | `resolve_review_proposal` | 2 | lib.rs |
| 1238 | `rename_session` | 2 | lib.rs |
| 1590 | `compact_session` | 2 | lib.rs |
| 1633 | `export_session` | 2 | lib.rs |
| 1994 | `inspect_paths` | 2 | lib.rs |
| 2023 | `save_attachment` | 2 | images.rs, lib.rs |
| 2506 | `refresh_session_window` | 2 | lib.rs |
| 2611 | `checkpoint_session_page` | 2 | lib.rs |
| 2701 | `rename_session_summary` | 2 | lib.rs |
| 97 | `default_settings` | 1 | profile.rs |
| 866 | `attach_pending_link` | 1 | lib.rs |
| 1932 | `load_workspace_sync` | 1 | lib.rs |
| 1945 | `clear_workspace` | 1 | lib.rs |
| 1976 | `save_workspace` | 1 | lib.rs |

## Types declared

```
296  type DirectorySessionData
540  pub struct CommittedSaveProof
550  pub struct CommittedSaveError
753  struct ReviewKey
1305  pub enum TranscriptPart
```
