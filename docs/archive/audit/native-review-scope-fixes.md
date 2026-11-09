# Native review scope fixes — INATIVE-03 / INATIVE-04

## Outcomes

- **INATIVE-03 / H03:** Kept workspace-relative status and made commits consistent with it. Before any `git add`, a subworkspace checks the entire staged index using NUL-delimited, repository-relative paths (`--no-relative --no-renames`), rejecting any staged path outside its prefix, including cross-boundary renames. Staging uses literal `add -A -- .`; subworkspace commits additionally use literal `commit --only -- .`. Repository-root workspaces retain ordinary repository-wide add/commit behavior.
- **INATIVE-04 / M04:** `ordered_transcript` compares content against concatenated segment narrative at segment boundaries, not a single segment. Actual frontend `content = intermediate + final` mirrors no longer repeat in Markdown, standalone HTML, or deterministic summaries. Incomplete legacy segments retain only an absent aggregate tail; genuinely distinct content is retained, including content that is merely a substring of the final segment.

## Executed RED → GREEN evidence

No full Cargo suite/build/commit was run. To avoid the parent's concurrent Cargo/clippy work, compiled exact current production regions and the in-file regression tests with `rustc --edition=2021 --test`, reusing existing `src-tauri/target/debug/deps` libraries. Git tests run real Git commands in disposable parent-repository/subworkspace fixtures under Hermes scratch; production behavior is not mocked.

Harness: `C:/Users/Tano/AppData/Local/hermes/cache/scratch/native_review_harness.py`.

| Filter | RED | Final GREEN |
| --- | --- | --- |
| `regression_commit_scope` | 2 failed, 1 passed: hidden siblings committed; external staged changes accepted | 3 passed, 0 failed |
| `regression_transcript_` | 2 failed: intermediate/final duplicated; partial legacy aggregate repeated | 2 passed, 0 failed |

A further RED case caught overbroad arbitrary-suffix deduplication; corrected matching to segment boundaries and reran GREEN.

Git coverage verifies committed path lists (addition/modification/deletion inside workspace only), unchanged sibling file contents, unchanged sibling HEAD content, clean workspace after scoped commit, rejection before any index-byte/HEAD/worktree diff mutation with inside and outside staged/unstaged changes, rejection of a crossing rename, and root-workspace standard all-changes behavior. Transcript coverage round-trips the real two-tool, three-text-segment frontend shape through session JSON and checks narrative count/order in both export formats and summary, plus tool/result chronology.

`rustfmt --edition 2021 --check` and `git diff --check` passed for both owned Rust files. Git emitted only existing LF/CRLF conversion notices. The patch tool's automatic Rust checker assumes edition 2015; explicit edition-2021 rustfmt and rustc checks succeeded.

## Scope / residual verification

Modified only `src-tauri/src/agent/git.rs`, `src-tauri/src/agent/storage.rs`, and this report. Previous owners' changes were preserved. The parent's full native/frontend integration gates remain outstanding; isolated production-region GREEN is not a full-crate pass. Git hooks and independently concurrent Git clients are not made transactional by this change; the scoped commit pathspec prevents ordinary late-staged sibling entries from being included, but adversarial hooks/concurrent worktree mutation are not tested.

Parent focused commands after source integration:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib regression_commit_scope
cargo test --manifest-path src-tauri/Cargo.toml --lib regression_transcript_
```

## Self-check

Accuracy 4/5: real behavioral RED/GREEN; full-crate verification intentionally deferred. Completeness 4/5: requested fixtures covered; hostile hooks/concurrent-client mutation not exercised. Clarity 4/5: fixes and verification limits separated. Actionability 4/5: in-file tests and exact integration filters supplied. Conciseness 4/5: evidence retained here rather than repeated in handoff. Overall 4.0/5. Highest-value follow-up: parent runs the focused filters against the integrated crate. This assessment does not imply release approval.
