# Independent native review fixes

## INATIVE-01 — Anthropic helper / snapshot validator mismatch

**Fixed in `src-tauri/src/agent/runner.rs`.** Native assistant `content` tool-use blocks now supply the authoritative pending invocation IDs. Each native invocation requires a nonblank ID/name, object input, and a globally unique ID. Optional legacy `tool_calls` mirrors must have the same ID/name/parsed object input set; mirrors do not add a second pending invocation. Missing/null mirrors are accepted. Signed thinking and redacted blocks are neither rebuilt nor altered.

Regression coverage:
- Calls the real `StreamResult::assistant_message(ProviderKind::Anthropic)` unchanged; asserts it has no `tool_calls`, validates signed thinking + redacted thinking + tool use / result / final assistant, JSON-round-trips persisted history, and verifies identical replay.
- Checks pending-only rejection and fully paired checkpoint acceptance.
- Rejects invalid native ID/name/input, duplicate/reused IDs, duplicate results, and assistant continuation before results.
- Checks matching mirrors with different argument whitespace/key order and rejects phantom/missing/duplicate mirrors, name/input disagreements, malformed arguments, and non-object arguments.
- Existing system/orphan/unpaired snapshot rejection and phantom legacy-call filtering remain covered.

### Executed proof

RED, before validator change:

`cargo test --manifest-path src-tauri/Cargo.toml --lib native_anthropic_helper_snapshot_validates_and_replays_unchanged -- --nocapture`

Result: **0 passed; 1 failed**, expected `assertion failed: valid_provider_messages(&native)` at runner.rs:2372. (An initial test-import compilation error was corrected before this behavioral RED.)

GREEN, same command after validator change: **1 passed; 0 failed**.

Focused runner suite:

`cargo test --manifest-path src-tauri/Cargo.toml --lib agent::runner::tests:: -- --nocapture`

Result: **24 passed; 0 failed; 168 filtered out**. Existing user-local Aether skill files emitted invalid-YAML diagnostics during skill-loading tests; these did not fail any test and were not modified.

`rustfmt --edition 2021 --check src-tauri/src/agent/runner.rs` and `git diff --check -- src-tauri/src/agent/runner.rs`: exit **0**. Git emitted only its LF/CRLF advisory. The edit tool's standalone lint incorrectly assumed Rust 2015; the actual edition-2021 Cargo build/test succeeded.

No commit, full-suite build, or Clippy invocation. INATIVE-02/03/04 are outside this worker's ownership.

### Reusable regression rule

Exercise native replay validation with the real provider helper's untouched return value, not a hand-built mixed-adapter fixture. Native invocation records are authoritative; optional compatibility mirrors must agree without becoming additional invocations. Preserve opaque signatures exactly.

### Self-check

Accuracy 4/5: real RED/GREEN and runner tests; no live-provider request. Completeness 4/5: scoped finding and invalid-path regressions covered; global gates remain parent-owned. Clarity 4/5: explicit native/mirror contract and exact commands. Actionability 4/5: tested changes ready for integration, no commit. Conciseness 4/5: bounded to one validator and its regressions. Overall 4.0/5. Highest-value follow-up: parent-owned integration/full gates after the parallel adapter-binding work settles.
