# Runner fixes — scoped remediation

## Scope and verified outcome

Source changed only in `src-tauri/src/agent/runner.rs`; this report is the only additional repository file created by this worker. No provider/types/tools/tasks/storage/frontend/lib changes, full native gate, or commit performed. Read all 17 runtime findings; implementation scope prioritizes RT-01/04/11/12/13/14/15. Existing concurrent changes were preserved.

Final verification:

```text
rustfmt --edition 2021 src-tauri/src/agent/runner.rs
cargo test --manifest-path src-tauri/Cargo.toml agent::runner::tests -- --test-threads=1
21 passed; 0 failed; 153 filtered out; 1.14s
```

Baseline: same focused suite passed 10 tests before edits. Final suite has 11 added tests, with the existing compaction test extended. Remaining compile warning in final run: unused `git::parse_porcelain_output`, outside this scope. Standalone patch lint mistakenly assumes Rust 2015; edition-aware rustfmt and Cargo are the authoritative results.

## Exact fixes

- **RT-04/15:** replay populated chronological `segments` as assistant → paired tool results per segment; do not duplicate aggregated content/tools. Empty/missing segments use legacy preamble before calls and final content after results. Only terminal done/error/cancelled calls with explicit result, nonempty unique ID/name and valid object JSON arguments are exported. Pending/running, malformed, orphan and duplicate calls are discarded. Explicit cancellation results remain paired.
- **RT-11:** parent builds immutable inherited context from operator instructions, root AGENTS.md and execution mode; children receive no parent transcript. Child prompt loads additional AGENTS.md along the validated root-to-subpath directory chain, preserving root instructions. Child context-load failure terminates child rather than silently skipping those extra instructions.
- **Completion options:** `subagent_completion_options` clones every parent setting (thinking, reasoning effort, temperature, token cap, streaming); investigation and synthesis no longer hardcode divergent values.
- **RT-12:** a child requires an explicitly selected valid workspace. Empty/whitespace cwd fails before progress, skills or provider work. Shared OS temp root is no longer activated as a child workspace.
- **RT-13:** check child stream cancellation before tool dispatch/completion. Synthesis handles cancellation token, provider cancellation flag, provider error, empty report and investigation-budget exhaustion explicitly. These return unsuccessful/cancelled outcomes and failed/cancelled progress, retain labelled partial findings, and never emit Completed artifacts or success prose. Provider errors in synthesis are redacted. Investigation budget exhaustion remains incomplete even if final synthesis returns prose.
- **RT-14:** forward child artifact/terminal events through a child-scoped sink, namespace artifact IDs consistently, emit child-scoped running/final tool identities containing child + turn + stream index, and retain artifact link, files_changed, terminal output, duration and exit status in tool updates. Child malformed/nonobject arguments now fail rather than executing `{}`.
- **RT-01 minimal enforcement:** one fail-closed RequestReview allow-list gates both parent and child dispatch before native/MCP side effects. Reject write/edit/command/learn_skill/task mutation/unknown/MCP tools; allow workspace reads, fetch_web_page, use_skill, task list/status and policy-inheriting children. Parent also skips MCP connections entirely in RequestReview because connecting stdio MCP can itself spawn an executable. No fake approval UI or prose-based grant. Existing missing-mode Turbo default and other modes' compatibility remain unchanged.
- **Parent-requested P04 integration:** live parent/child conversation records use `StreamResult.assistant_message(kind)` before consuming calls. `provider_state` emits only current-run assistant/tool state, excluding system/history/user, after final assistant or fully paired tool boundary. It never publishes an unmatched partial tool invocation. Replay prefers validated `providerMessages`, preserving ordered signed/redacted Anthropic blocks and Deepseek reasoning fields supplied by the provider helper. Invalid/orphan/system/user snapshots fall back to normalized presentation replay. Snapshot validator enforces unique call IDs, object JSON args, complete tool responses and native tool_use ID correspondence.
- **Compaction compatibility:** prune aggregate tool outputs and mirror replacements into canonical segment results plus native role=tool output. Preserve signed assistant blocks unchanged rather than allowing canonical replay to bypass pruning.

## Regression tests added

1. `replay_segments_preserves_two_tool_turns_without_duplicate_aggregate_text`
2. `replay_filters_phantom_invalid_and_duplicate_calls_but_keeps_terminal_pairs`
3. `replay_legacy_preamble_precedes_tools_and_final_content_follows_results`
4. `subagent_rejects_missing_workspace_without_temp_fallback`
5. `child_policy_preserves_operator_root_and_nested_memory_without_history`
6. `child_options_preserve_every_parent_completion_setting`
7. `request_review_blocks_child_write_and_exec_before_side_effects`
8. `synthesis_cancel_error_empty_and_budget_never_report_success`
9. `child_artifact_and_terminal_events_share_scoped_identity`
10. `request_review_does_not_start_mcp_servers_during_connection`
11. `native_replay_preserves_signed_blocks_and_rejects_unpaired_snapshots`

RED tests were written before each coherent slice. Child/policy/synthesis/artifact/native-state and MCP-connection slices recorded expected missing-helper/signature compilation REDs before implementations. Replay's initial RED execution was blocked by concurrent provider test privacy error; subsequent attempts were blocked by tools/storage/lib intermediate edits. Compaction's RED attempt was blocked by concurrent MCP missing helper. These are not represented as behavioural assertion failures. Final focused suite compiles and passes all tests on the integrated concurrent tree.

## Residual / integration ownership

- Tests prove pure replay/policy/outcome/scoping helpers and real child dispatcher rejection (including absence of a written file), not an end-to-end Tauri run with mock provider, live process/PID capture, saved-session reload or native UI. Full gates and frontend event/persistence integration belong to parent. No claim that RT-14 durable UI/session evidence was exercised end to end.
- RequestReview blocks native writes/exec/MCP; it does not implement approvals. Default/Auto natural-language safeguards and missing-mode Turbo are unchanged. Workspace/operator prose constraints other than execution mode still depend on model compliance.
- RT-05 mutation/resource locking is owned by tools worker, not modified here. RT-02/03/06/07/08/09 tasks, RT-10 frontend attempt rollback, RT-16 council scope validation, RT-17 review lease atomicity remain outside implemented slice. Historical malformed native snapshots fall back safely but cannot reconstruct unavailable signed thinking.
- Legacy aggregate snapshots without segments inherently lack exact interleaving; fallback preserves available preamble/tool/final order without inventing absent turn records. Canonical native state protects new parent replay from flattened child-evidence tool updates.
- Several patch calls timed out after writing due to automatic lint; source was inspected before continuing, formatted with edition 2021 and compiled with Cargo. Temporary concurrent compile failures were not repaired outside ownership.

## Self-evaluation

Accuracy 4/5: final Cargo proof, but no live Tauri provider scenario. Completeness 4/5: assigned priority slices implemented; UI persistence remains parent gate. Clarity 4/5: fixes, proof and residual separated; detailed report is longer than final handoff. Actionability 4/5: buildable tested source, with explicit integration ownership. Conciseness 4/5: short handoff plus detailed evidence here. Overall 4.0/5. Highest-value follow-ups: parent end-to-end provider_state save/reload check; child cancel-during-synthesis event test; persisted child artifact verification. Self-check: these ratings do not equate helper coverage with native runtime proof.
