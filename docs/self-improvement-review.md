# Self-Improvement Review

After a conversation turn finishes, Aether can review what happened and refine local skills. The reviewer runs as a native agent loop in Rust, inspecting the session history and using dedicated tools to patch, update, or create skills.

## Runtime behavior

- Automatic review is enabled by default for fresh installations. If you previously set `self_improvement_enabled: false` in settings, that choice is preserved.
- `reviewConfig.creationNudgeInterval` defaults to **10 provider iterations** across foreground turns in the same session (not ten individual tool calls or separate messages). Setting this to `0` disables automatic reviews.
- Iterations count once per outer provider loop. Attempting a skill mutation resets the counter. When the count hits the interval, the counter resets before checking eligibility.
- Reviews trigger only after a complete, uncancelled response. If a run already made foreground skill changes, it reports them directly and skips the background review.
- You can run `/review`, `/reflect`, or `/refine` manually to inspect the full conversation, even if automatic reviews are disabled. Manual reviews do not add user messages or restart your task, and they do not require an active workspace.
- Review runs are isolated from chat history and limited to **16 iterations and 120 seconds**. They cannot spawn recursive reviews. Any changes applied or staged before a cancellation or error are preserved and reported.
- Starting new foreground work cancels an active review in the same session. A session can have at most one active review at a time, and the app allows up to four concurrent reviews across different sessions.

## What the reviewer learns

The reviewer receives a snapshot of the conversation and looks for reusable patterns:

1. Inspect and patch skills that were loaded during the original task.
2. Update existing class-level skills when relevant.
3. Add or update topical files in `references/`, `templates/`, or `scripts/`.
4. Create a new skill only when no existing skill fits the problem.

Focus on durable procedures, verified patterns, and user preferences. The reviewer should ignore transient errors, session-specific tokens, credentials, and failed experiments. If nothing generalizable happened, leaving skills unchanged is the expected outcome.

Automatic reviews using the same provider reuse the system prompt prefix, tool schemas, and model options from the parent run. Active API keys are redacted, and request overrides cannot replace review prompts or tools. Routed reviews use the selected auxiliary provider with their own options.

## Restricted tools and mutations

Tool dispatch in review mode is strictly limited to `skills_list`, `skill_view`/`use_skill`, restricted `read_file`/`search_files`, and `skill_manage`. Terminal commands, network requests, arbitrary file writes, delegation, and legacy learning shortcuts are completely blocked.

`skill_manage` supports four operations:
- `create`: requires name, description, and initial content.
- `patch`: requires name, old text, and new text; old text must match exactly once.
- `edit`: replaces the entire file content.
- `write_file` / `remove_file`: manages relative support files inside the skill directory.

Before modifying an existing file, the reviewer must read that exact file during the review run. Quoting text from chat or search snippets is not enough to authorize a write. Reads preserve exact raw bytes (including CRLF), and each successful mutation consumes its read authorization. Frontmatter metadata is preserved byte-for-byte. Deleting an entire skill is not allowed during background reviews.

The reviewer can update both built-in and manually created skills. Adding a `.locked` file to a skill directory or setting `metadata.aether.locked: true` prevents changes. File paths are checked against directory traversal, Windows device names, alternate data streams, and symlinks. Direct file writes, approval handlers, and legacy writers share an in-process mutex to avoid write races.

## Skill locations and discovery

New skills are stored in `~/.aether-agent/skills/` by default. You can change this path with `reviewConfig.createDir`, using an absolute path, `~/...`, or a path relative to `~/.aether-agent`.

Discovery follows this order:
1. Workspace directory (`.aether/skills`)
2. Configured skill roots
3. Custom creation root (`reviewConfig.createDir`)
4. Default global roots (`~/.aether-agent/skills`)

Skills are updated in place wherever they were found. Foreground runs use the same guarded storage, so legacy `learn_skill` calls route through it. Subagents suggest maintenance tasks to the parent agent instead of writing to disk directly.

## Approval and staging

`reviewConfig.writeApproval` defaults to `false`. When turned on, proposed changes are staged under `~/.aether-agent/pending/skills/` instead of directly modifying files. Staged skills are not loaded into the active skill catalog until approved.

The review inspector displays unified diffs and provides Approve and Reject actions. Pending proposals persist across app restarts. The native snapshot on disk acts as the authoritative source for approval, and approving a change rechecks file locks and raw bytes. If the underlying file changed while a proposal was pending, the proposal remains pending to prevent accidental overwrites.

Approving or rejecting a proposal updates session history and emits an event to refresh UI state. File writes, pending file cleanup, and history updates happen sequentially. If an error occurs midway, the failure is reported clearly. If the original assistant message is still saving when you click Approve, the UI will ask you to wait a moment and retry.

## Settings and events

Configure review behavior in **Settings → Agent defaults → Agent Intelligence / Reflection**:

```typescript
interface ReviewConfig {
  creationNudgeInterval?: number; // default 10; 0 disables automatic review
  writeApproval?: boolean;        // default false
  createDir?: string;
  auxiliaryProviderId?: string;   // absent: uses current session provider/model
}
```

The review event carries the list of proposed or applied changes:

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

Inspectors read skill content using stored run and session IDs rather than accepting arbitrary client file paths. The legacy JSON extractor is kept for test backwards-compatibility, but the live runtime uses this agent loop.

## Parity summary

| Reflection & Review Capability | Status in Aether |
|---|---|
| Default enabled, interval of 10 provider iterations | Supported; existing explicit opt-outs are kept |
| Detached tool-driven review with up to 16 iterations | Supported, with a 120-second timeout limit |
| Update-first learning, user preferences, and support files | Supported |
| Fresh-read mutation guard, locks, custom destinations, and approval | Supported with strict frontmatter preservation |
| Same-provider parent tool and options snapshot | Supported; credentials redacted |
| Auxiliary provider configuration | Supported via any configured provider |
| Foreground priority with cancellation | Supported |
| `/refine focus text` with custom focus prompts | Not implemented; bare `/refine` runs standard review |
| Memory review and profile lifecycle | Planned for future releases |
| Routed-model token budgeting | Not implemented; full conversation snapshot is passed directly |
| Arbitrary skill metadata rewrites and skill deletion | Intentionally restricted |

## Verification

Tests use local mock HTTP endpoints and temporary test directories instead of live paid APIs. Test coverage includes provider tool loops, prefix inheritance, tool security boundaries, cancellation, timeouts, raw file reads and patches, support files, locks, conflict detection, durable proposals, UI events, and inspector approval actions.

To run the verification suite:

```sh
npm test -- --fileParallelism=false
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

These suites run on local fixtures and do not call external providers.
