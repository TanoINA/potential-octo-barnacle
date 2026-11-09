# Aether Rune - experimental native desktop host

**Runnable native implementation; incomplete feature parity.** The reference
application remains React/Tauri in `src/` and `src-tauri/`. Rune now has a Cargo
workspace and executable, a native GPU-rendered chat interface, controller/history
state, modal panels, attachments and export. It is no longer just a shared-backend
prototype. It is also **not** a full-parity or production-ready release, and does
not yet fulfill the requirement for every baseline feature with nothing missing.

See [feature-parity.md](docs/feature-parity.md) for all 31 baseline component files,
app/hooks, composer commands, the 63-command IPC inventory, source evidence,
missing behavior and acceptance gates. `native-implemented` means a real code path
exists, not that every baseline interaction or OS workflow has passed final QA.

## Build and run

Requires a Rust toolchain compatible with the pinned Rune/LaTeX dependencies,
platform native build tools (MSVC/Windows SDK on Windows), and a supported graphics
backend. Initial dependency fetching needs network access. This native executable
uses Rune/winit/wgpu, not a Tauri WebView, and does not require the React build.
Current evidence is for **Windows debug development**, not a signed installer or
verified Linux/macOS Rune build.

Prefer running Cargo **inside `rune/`** so it discovers `rune/.cargo/config.toml`:

```powershell
# Start from the repository root.
cd rune
cargo run --locked -p aether-rune

# Or choose an isolated absolute write profile.
cargo run --locked -p aether-rune -- --profile "$env:LOCALAPPDATA\AetherRune\development"

# State/startup smoke: always creates a fresh UUID temporary profile.
cargo run --locked -p aether-rune -- --smoke
```

`--smoke` constructs native application state and a new session, then exits. It
does **not** open a window or prove graphics, clipboard, dialogs or provider access.
It rejects combination with `--profile` or `--import-profile`.

The manifest also supports invocation from the repository root:

```powershell
cargo run --manifest-path rune/Cargo.toml --locked -p aether-rune -- --profile "$env:LOCALAPPDATA\AetherRune\development"
```

Cargo configuration discovery follows the **invocation directory and ancestors**;
`--manifest-path` does not automatically load the manifest directory's nested
`.cargo` configuration. Root and `cd rune` invocations can therefore use different
target directories. `rune/.cargo/config.toml` specifies `target-dir = "../target"`;
an explicit `CARGO_TARGET_DIR` overrides it. Existing logs also use older or
explicit verification targets: do not infer the newest executable from a legacy
path. Identify the actual target/profile/build and hash for final QA.

### First use and available workflows

A new isolated profile has default providers/settings, not your production API
keys. Configure an actual provider/model/key in **Settings / Models**, choose a
workspace and send a message. Native paths include:

- Chat/send/stop, reasoning/tool/council presentation and candidate selection.
- Session sidebar/history, deep-search preview, older pages and return-to-latest;
  guarded edit/resend, retry, undo and rollback through History/message actions.
- File/folder dialogs, file/image attach and clipboard/drop handling, preparation
  state and image preview. Source implementation is present; genuine OS roundtrip
  acceptance remains separate from injected runtime tests.
- Settings basic controls plus full typed configuration JSON, provider/model
  discovery/editing, artifacts, tools/skills/MCP, workspace memory, Git, task output
  and **actual confirmed task termination**.
- A bounded noninteractive workspace command form and OS-terminal launch. This
  is not an embedded PTY or persistent interactive terminal session.
- Native `/export` and `/save` workflows for **Markdown, HTML and JSON**, with a
  save dialog or explicit-path confirmation, scope/cancellation guards and actual
  backend writes. The older History panel's manual form advertises Markdown/JSON;
  the dedicated export workflow also supports HTML.
- Native modal/menu focus handling, local keyboard actions and a reduced-motion
  path. These do not establish full accessibility or shortcut parity.

Composer routing is real: `/new`, `/clear`, `/undo`, `/retry`, `/title`, `/stop`,
`/compact` (`/compress`), `/review` (`/reflect`, `/refine`), panel commands,
`/export`, `/save` and `/plan` have native/controller paths. `/agent` and `/ask`
apply request-only execution-mode overrides. A `/plan` run is implemented; a
baseline-equivalent interactive fenced-plan card is not. See the inventory for
arguments, guards, extra commands and missing autocomplete/help behavior.

## Safe profile contract

- Default write profile: **`~/.aether-agent-rune`**, separate from production
  **`~/.aether-agent`**. No automatic import, migration or fallback to production.
- Actual executable flags are `--profile ABSOLUTE_PATH` and
  `--import-profile ABSOLUTE_SOURCE`. CLI parsing is in
  `crates/aether-rune/src/cli.rs`; startup ordering is in `src/main.rs`.
  An import source can be the baseline profile only through this explicit copy
  request. That never authorizes selecting the baseline as Rune's write root.
- `aether-core` enables `shared-profile` by default. The Tauri crate declares it
  for shared-source compatibility but leaves it **off by default**. Do not disable
  the core routing feature in a production Rune build.
- Startup resolves/validates the chosen root, performs any explicit import, calls
  `configure_storage_root` **once**, and only then constructs the backend,
  controller, workers or UI and loads settings/workspace. The selected path is
  canonical; switching it requires a process restart.
- Baseline aliases/descendants and ancestors containing the baseline, relative
  roots, traversal, Windows trailing-dot/space or stream components, symlinks and
  reparse points (including nested junctions) are refused. Invalid defaults fail
  closed. No process-global HOME/environment mutation is needed for routing.
- Settings, workspace choice, canonical sessions, journals and derived projection
  use the selected root. Workspace files and external skill/MCP/attachment paths
  are separate resources: **profile isolation is not a permission sandbox**.

### Explicit import is an allowlisted copy, not a full migration

Close the source application first; preferably import a private snapshot. Choose a
new empty isolated target. For example, from `rune/`:

```powershell
# Only run this if you explicitly want to copy baseline configuration/history.
cargo run --locked -p aether-rune -- --profile "$env:LOCALAPPDATA\AetherRune\imported-copy" --import-profile "$env:USERPROFILE\.aether-agent"
```

No import is necessary to run Rune. Import reads the source only, validates the
allowed copy plan before creating the destination, rejects nonempty/overlapping
targets, and uses create-new writes without overwrites or cleanup deletion.

Allowed files are `config.json`, `workspace.json`, `sessions/*.json`, and matching
valid `sessions/*.checkpoint` recovery sidecars. Modern/legacy session shapes,
safe matching filenames, checkpoint identity/checksum and canonical stamp or
completed-target hash relationships are validated. Canonical modification times
are preserved for recovery. Orphan/corrupt/conflicting journals and deletion
tombstones abort import rather than silently discarding recovery data.

**Not copied:** tasks, vectors, arbitrary attachments/skills, logs, other user data
or `session-pages.sqlite` (a derived path-keyed projection that must rebuild).
Imported API keys, workspace choice, external paths, MCP commands and execution
settings need review before enabling tools. No process or task is resumed.

Files are limited to 64 MiB each, with import-plan limits of 10,000 files and
256 MiB. An I/O failure may retain partial output for inspection; it is not deleted,
and re-import refuses that nonempty destination. Startup-time path checking does
not prove safety against hostile concurrent namespace replacement or hard links.
Do not use attacker-writable sources/roots. Unix-specific safety still requires
actual platform runs.

## Known missing or incomplete features

These are delivery gaps, not merely undocumented options:

- **Composer:** no baseline queued-instruction edit/remove/resume workflow, `@`
  workspace-file autocomplete, full dynamic-skill/slash suggestion UX or Spotlight
  command palette. File attachment dialogs are not mention autocomplete.
- **Context menu:** native message-source/plain-text copy and guarded history
  actions exist, but not raw-turn JSON copy or the full selection/code/input/link/
  file/global menu. Quote/explain/refactor/audit/summarize/translate, code test/save
  prompts and cut/paste-as-code context actions are not ported.
- **Plans/traces/subagents/diffs:** fenced `plan` blocks remain ordinary code;
  generic task-list editing is not the baseline three-state progress card. Tool
  details are all expanded rather than phase-scoped/autocollapsed; subagents are
  flat role/status cards, not tree inspection. Diff-source rendering is not an
  aligned native original/modified side-by-side viewer.
- **Rich content:** native math is real bounded `latex-rust` SVG rendering, with
  original-source preservation and visible unsupported fallback. Its supported
  LaTeX subset and inline equation layout/cards are not full KaTeX/baseline
  academic-math fidelity. Screenshot examples do not establish general parity.
- **Configuration/recovery UX:** advanced JSON preserves configuration and
  redacted secrets, but is not the full baseline semantic settings/model/MCP UX.
  Initial asynchronous settings completion may replace edits made before load;
  the fixed fixture-ordering test does not prove a production loading barrier.
  Error banners/controller save retry are not a general baseline Retry All UI.
- **OS integration:** no native speech/dictation, sound playback/test, notification
  delivery or global Alt+Space wiring. Baseline chime is optional and OFF by
  default; baseline Web Speech is availability-dependent. Stored settings alone
  do not implement these effects.
- **Release/platforms:** no Rune signed updater host, supported update install/
  restart, signed installer acceptance or verified Rune Linux/macOS build. About
  reports release information and explicitly disables unavailable installation.
- **Acceptance:** final-source close/failure/multi-window tests, real OS clipboard/
  drop/dialog/IME checks, screen-reader/keyboard/high-contrast acceptance and
  long-session scrolling/memory/performance budgets remain gates.

## Test commands and evidence status

From `rune/`:

```powershell
cargo test --workspace --locked
cargo test --locked -p aether-core --lib agent::profile
cargo test --locked -p aether-core --no-default-features --lib -- --test-threads=1
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo fmt --all -- --check

# Separate baseline feature-off regression gate, not a Rune GUI test.
cargo test --manifest-path ../src-tauri/Cargo.toml --lib -- --test-threads=1
```

The documentation update did **not** rerun these commands. Existing Windows logs
show six profile tests passing, historical native/controller/history runs and a
successful temporary-profile startup smoke. The test-gate handoff at
`../verification/native-rich-content/gate-finisher-handoff.md` reports two later
locked default-parallel workspace passes, each **884 passing executions / 4
ignored**; it explicitly warns these include repeated shared-backend tests
(**521 passing / 2 ignored distinct fully qualified names**, not 884 independent
regressions). Consult its underlying logs before using totals in a final report.

Earlier parallel core logs failed a registry-contention timing test while serial
runs passed. The later handoff reports a test-only harness repair and green
repeats; it also reports **strict workspace Clippy and formatting still failing**.
Do not present either those earlier failures or later passes as a freshly executed
final-source result. The startup-settings test fixture was synchronized; production
edit-before-load behavior remains a separate review question.

Prior GUI QA under `verification/rune-qa-20261109-032751/` predates later lifecycle/
close changes and does not identify acceptance of the final executable hash.
Runtime/injected dialog/clipboard tests, loopback provider fixtures and screenshots
are useful narrow evidence, not genuine OS or external-provider certification.
**Final integration testing and final-build QA remain with the parent task.**
The parity inventory records exact sources, evidence limits and next tests; no
full-parity or production-readiness claim is made.
