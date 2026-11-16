# Project Index (generated)

root: `D:/AetherAgent`

**268 authored files · 90866 lines · 1114 test cases**

tauri commands: 65 · frontend invokes: 53 · event topics: 9

Excluded from indexing: `.local/` (QA evidence, vendored `rune-upstream`, snapshot `working-files`), `src-tauri/gen/`, `docs/archive/`, `node_modules/`, `target/`, `dist/`, lockfiles.

## Areas

| area | files | lines | tests | file |
| --- | --- | --- | --- | --- |
| backend | 32 | 35796 | 452 | `docs/index/files-backend.md` (4700B) |
| config | 14 | 378 | 0 | `docs/index/files-config.md` (318B) |
| docs | 9 | 652 | 0 | `docs/index/files-docs.md` (338B) |
| frontend | 114 | 27551 | 428 | `docs/index/files-frontend.md` (6511B) |
| other | 11 | 625 | 0 | `docs/index/files-other.md` (468B) |
| release | 26 | 1925 | 17 | `docs/index/files-release.md` (1249B) |
| rune | 62 | 23939 | 217 | `docs/index/files-rune.md` (5568B) |

## Directory rollup

```
    378 L   14 files  .
     53 L    1 files  .aether/skills/ponytail
    104 L    1 files  .aether/skills/ponytail-audit
     45 L    1 files  .aether/skills/ponytail-debt
     53 L    1 files  .aether/skills/ponytail-gain
     77 L    1 files  .aether/skills/ponytail-help
    102 L    1 files  .aether/skills/ponytail-review
    260 L    3 files  .github/workflows
    652 L    9 files  docs
   2788 L    3 files  rune
      3 L    1 files  rune/.cargo
     39 L    1 files  rune/crates/aether-core
   2105 L    4 files  rune/crates/aether-core/src
    780 L    2 files  rune/crates/aether-core/tests
    140 L    4 files  rune/crates/aether-rune
   7953 L   18 files  rune/crates/aether-rune/src
    808 L    3 files  rune/crates/aether-rune/src/panels
   2298 L    9 files  rune/crates/aether-rune/tests
    111 L    2 files  rune/crates/aether-state
   5260 L    8 files  rune/crates/aether-state/src
   1360 L    6 files  rune/crates/aether-state/tests
    294 L    1 files  rune/docs
   1516 L   19 files  scripts
     59 L    2 files  scripts/release-policy-tests
     90 L    2 files  scripts/release-policy-tests/src
    984 L    6 files  src
    177 L    4 files  src-tauri
     14 L    1 files  src-tauri/capabilities
   1811 L    2 files  src-tauri/src
  33985 L   30 files  src-tauri/src/agent
  17846 L   74 files  src/components
   7954 L   26 files  src/hooks
     89 L    2 files  src/test
    357 L    1 files  src/types
    321 L    5 files  src/utils
```

## Event topics

```
agent-event
agent:micro-compaction
agent:self-improvement
agent:self-improvement-error
controller:result
irrelevant
release:update-progress
terminal
toggle-spotlight
```

## IPC cross-check

**Rust commands never invoked by frontend (12)**
```
approve_review_proposal
call_mcp_tool
checkpoint_session
checkpoint_session_page
get_background_task_output
get_workspace_cwd
git_fetch
reject_review_proposal
rename_session
save_session
save_session_page
save_skill
```

**Frontend invokes with no Rust command (0)**
```
(none)
```
