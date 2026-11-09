# Self-Improvement Review

## Reference and scope

The reference implementation is the locally installed Hermes Agent at commit `bf0203a794a`, primarily `agent/background_review.py`, `agent/turn_finalizer.py`, and `agent/agent_init.py`. Aether now uses a tool-driven post-turn skill-review agent, not the former one-shot JSON extractor.

This ports the skill-review behavior into Aether's Rust/Tauri architecture. It is not a claim of literal parity with every Hermes subsystem. The separate memory-review/USER.md loop, curator, profile router, managed-local idle queue, routed-model context digest, token-usage accounting and focus arguments to `/refine` are not implemented by this change. See the parity table below.

## Runtime behavior

- Automatic skill review defaults to enabled for fresh or missing-field settings. An existing explicit `self_improvement_enabled: false` is preserved.
- `reviewConfig.creationNudgeInterval` defaults to **10 provider iterations**, accumulated across foreground turns in the same session—not ten tool calls or ten successful runs. Zero disables automatic cadence.
- Iterations count once per outer provider loop, not per retry or individual tool. Attempted skill mutations reset cadence. Reaching the interval consumes/resets the counter before checking eligibility.
- Eligible reviews start after delivery of a finished, uncancelled, nonempty final response. Runs that already produced foreground skill changes report those changes without launching another review of the same run.
- `/review`, `/reflect`, and bare `/refine` explicitly review the full completed conversation even when automatic review is disabled. They do not insert a user message or restart the original task. A workspace is not required.
- Review runs are independent from chat history and bounded to **16 iterations and 120 seconds**. They cannot recursively trigger reviews. Partial applied/staged changes are reported even if a later request fails or the review is cancelled.
- New foreground work cancels a same-session review. Identity-scoped registry cleanup prevents an old review from removing a successor. There is one review per session, at most four concurrently.

## What the reviewer learns

The reviewer receives the full available conversation snapshot and prioritizes durable user corrections and preferences:

1. Inspect and patch a skill loaded during the original task.
2. Update an existing relevant class-level skill.
3. Add or update topical `references/`, reusable `templates/`, or `scripts/` support files.
4. Create a new class-level skill only when no existing skill fits.

Capture procedures, verified techniques and task-governing preferences—not incident logs, temporary failures, credentials, unresolved attempts presented as successes, or per-session skill proliferation. `Nothing to save` remains valid.

Same-provider automatic reviews inherit the parent's system/history prefix, tool schemas and completion options. Active provider credentials are redacted, and provider body overrides cannot replace review messages or tools. Explicit reviews reconstruct history; routed reviews use the selected provider with their own options. No provider-cache hit is claimed or measured.

## Restricted tools and mutations

Advertised schemas are not authorization. Dispatch permits only `skills_list`, `skill_view`/`use_skill`, restricted `read_file`/`search_files`, and `skill_manage`. Shell, network, generic writes, delegation and legacy learning shortcuts cannot execute in a review.

`skill_manage` supports:

- `create`: name, description, content.
- `patch`: name, old_string, new_string; old text must match exactly once.
- `edit`: name, content containing the complete file.
- `write_file` / `remove_file`: name and relative support-file path; content for writes.

Before mutating an existing file, the reviewer must freshly read that exact file during the review. Transcript quotations and search snippets do not authorize writes. Reads retain raw bytes, including CRLF, rather than normalized activation text. Successful mutations consume read authorization. Existing metadata/frontmatter is preserved byte-for-byte. Whole-skill deletion is denied in background review.

Manual and global discovered skills can be updated, not only reflection-generated skills. A `.locked` marker in the skill directory or `metadata.aether.locked: true` prevents mutation. Filename validation rejects traversal, ADS/device names, symlink/reparse paths and oversized or unsafe content. Exact raw-content CAS is rechecked after staging immediately before commit. In-process direct, approval and legacy writers share a mutation mutex. This is not handle-relative protection from hostile concurrent OS filesystem swaps.

## Skill destinations and discovery

New skills default to `~/.aether-agent/skills/`. `reviewConfig.createDir` can select an absolute path, `~/...`, or a path relative to `~/.aether-agent`.

Effective discovery precedence is workspace `.aether/skills`, configured skill roots, selected creation root, then default global roots. The custom destination participates in normal catalogs, activation, delegated catalog paths and UI listing without a second configuration entry. Existing skills are updated where discovered.

The parent foreground agent uses the same guarded store for maintenance. Legacy `learn_skill` creation is routed through it; overwrite requests must use fresh-read patch/edit instead. Subagents report proposed maintenance to the parent rather than bypassing approval through legacy writes.

## Approval and durable proposals

`reviewConfig.writeApproval` defaults to false, matching Hermes' default direct-write policy. When enabled, mutations stage durable proposals under `~/.aether-agent/pending/skills/` and do not modify source files or expose staged creations as installed skills.

The review inspector lists changes, displays inert unified diffs, and supports Approve/Reject. Pending records survive restarts. IDs and operations are session-scoped; the native sealed staging snapshot—not client paths—is authority for approval. Approval checks locks and original bytes again; stale source changes leave the proposal pending. Duplicate UI decisions are guarded.

Foreground proposals are associated with their origin run as well. Approval/rejection updates native session history and emits the complete authoritative review event to refresh live state and caches. Native stale-save merging, event merging and restored-cache guards prevent a late staged event from reverting a resolved proposal.

A skill write, pending cleanup and session-history update are not a single cross-file transaction. Partial-success failures are surfaced explicitly. If the origin assistant has not yet been persisted, decision APIs ask the user to wait and retry.

## Settings and events

Settings → Agent defaults → Agent Intelligence / Reflection exposes automatic enablement, iteration interval, approval, creation destination and auxiliary provider selection.

```typescript
interface ReviewConfig {
  creationNudgeInterval?: number; // default 10; 0 disables automatic review
  writeApproval?: boolean;       // default false
  createDir?: string;
  auxiliaryProviderId?: string;  // absent: current session provider/model
}
```

`agent:self-improvement` retains legacy fields and adds a list of changes:

```typescript
{
  run_id: string;
  session_id: string;
  action: 'skill_created' | 'skill_updated' | 'skill_staged' | 'no_action';
  skill_name?: string;
  summary?: string;
  workspace_cwd?: string;
  changes?: Array<{
    action: 'skill_created' | 'skill_updated' | 'skill_staged' | 'no_action';
    skill_name: string;
    file_path?: string;
    skill_path?: string;
    pending_id?: string;
    summary?: string;
  }>;
}
```

Inspectors authorize reads through stored run/session/change associations, not arbitrary client-supplied paths. Legacy badges and workspace-generated skill reads remain supported. The former JSON extractor is retained only for regression tests, not used by the live runner.

## Parity boundaries

| Hermes skill-review behavior | Aether status |
|---|---|
| Default enabled, interval 10 provider iterations across turns | Implemented; existing explicit opt-out retained |
| Full-conversation, detached, tool-driven reviewer; 16 iterations | Implemented, with an additional 120-second deadline |
| Update-first learning, user corrections, support files, multiple changes | Implemented |
| Fresh-read mutation guard, lock, global/custom destination, approval | Implemented with Aether-native paths and stricter immutable frontmatter |
| Same-provider parent tool/options/history snapshot | Implemented; credentials redacted; no measured cache guarantee |
| Auxiliary provider | Implemented via an existing configured provider |
| Foreground priority / identity-safe cancellation | Implemented; no managed-local idle/requeue service |
| `/refine focus text` | Not implemented; only the bare alias is a review command |
| Memory/USER.md review, curator, profile lifecycle | Outside this skill-review port |
| Routed-model digest, aggregate input-token budget, usage attribution | Not implemented; full snapshot may exceed a selected model's context |
| Arbitrary skill metadata rewrite, whole-skill delete | Deliberately more restrictive |

## Verification

Tests use deterministic local HTTP providers and disposable filesystem fixtures, not paid credentials. Coverage includes provider tool cycles, exact prefix inheritance, denial of bypasses, cancellation, timeout/budget, partial results, raw-file reads/patches/edits, support files, locks, CAS conflicts, shared writer serialization, durable proposals, foreign session denial, native attachment, late-event ordering, cache restoration, command routing, settings and inspector decisions.

Required final commands:

```sh
npm test -- --fileParallelism=false
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

No literal power-loss, live paid-provider, cache-hit, packaged desktop or long-running managed-local scheduling test is implied by these unit/integration suites.
