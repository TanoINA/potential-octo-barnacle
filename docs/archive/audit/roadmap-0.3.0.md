# Roadmap implementation — AetherAgent 0.3.0

## Scope and release identity

This batch implements the six improvement areas selected in the user's screenshots, while retaining academic math, vision attachments, lazy history, self-improvement review, UI polish and existing tools. It is not a framework rewrite or a claim that every historical audit finding is closed.

Product version is synchronized at **0.3.0** in npm, Cargo, lockfiles and Tauri. A separate compile-time build ID contains UTC time, git SHA and working-tree state. Signing credentials are not embedded. The working tree contains earlier changes and user assets; it was not reset or committed.

## 1. Incremental checkpoint and recovery

Periodic frontend ticks now send `checkpoint_session_delta`, not a complete `SessionData`. The request contains the current run's assistant snapshots, a monotonically increasing sequence, canonical revision, and only artifacts modified by that run. Historical user images and lazily read old artifacts are excluded.

Native storage atomically replaces one cumulative `<session>.checkpoint` sidecar. Warm ticks stat the canonical file and read exact-ID baseline rows; they do not parse or rewrite canonical history. The initial baseline can still require a cold projection rebuild. Final saves validate that durable progress is present before folding into canonical JSON and clearing the sidecar.

Recovery validates run/message identity, file fingerprint, sequence, corruption checksum and monotonic progress. Tool transitions are typed: pending placeholders may resolve to real names/arguments and progress through execution; completed results cannot reopen or be rewritten. Explicit checkpoint epochs allow retry reasoning resets without discarding visible narrative. Artifact stream revisions authorize replacements; stale versions are rejected.

An active-process ownership registry distinguishes live sidecars from crash leftovers. Catalog/page/around/artifact reads overlay live progress without advancing canonical revisions. A process restart folds abandoned journals. A sealed target checksum handles interruption after canonical replacement but before projection repair/sidecar removal. Failed projection writes retain recovery data. Tombstones prohibit resurrection.

Stop pauses new and queued checkpoint submission before optimistic cancellation. Actual native tool completion may still enrich its result, and the final save includes that data. Initial-save failures clear startup flags in both normal and Council paths.

**Measured invariant:** a fixture with 2 MiB historical image data produces a delta under 2 KiB and sidecar under 4 KiB; warm ticks leave canonical bytes/mtime unchanged. This is cumulative active-turn snapshotting, not a token append log or unlimited-size guarantee. A very large active turn/provider transcript/artifact can still make the sidecar large.

## 2. Loading, background indexing and cache

Cold catalog listing uses streaming metadata visitors rather than allocating all messages/artifacts or building every projection. It still scans JSON bytes and checks syntax; semantic message validation occurs on page/index loading.

`warm_session_index` uses `spawn_blocking`, bounded batches, cooperative time budgets and nonblocking per-session lock acquisition. Frontend idle scheduling waits for initialization/navigation/runs/reviews/persistence/checkpoints to finish and is cancelled on teardown. Individual large files can exceed the cooperative budget; an already-running rebuild is not interrupted.

Clean inactive sessions are evicted using an estimated **32 MiB budget** plus a six-inactive-entry cap. Current/running/dirty/starting/in-flight entries are protected and can exceed the nominal cap. Estimates avoid JSON serialization, count object graphs once within snapshot pairs, and are cached weakly for immutable snapshots. No graph measurement runs per token or RAF.

A local 100-entry ring records cold/cache/older load durations and loaded/total counts. It contains no session IDs, message content, titles or telemetry. About displays local aggregate samples.

History pages now omit `imageInputs` and `providerMessages`, with derived presence markers. Active-journal overlays apply the same thinning. Canonical bytes stay native; partial and full append saves restore omitted payloads before validation. Explicit retry/edit can hydrate exact message image inputs before destructive operations; model preparation still uses canonical native history. Page IPC tests with multi-megabyte replay fixtures remain under 10 KiB. Selected SQLite rows still contain and decode canonical payloads internally; this is IPC/webview memory reduction, not complete blob externalization.

## 3. Transparent job state

A scoped runtime status surface distinguishes preparing attachments, waiting for provider, receiving answer, executing tools, saving results and terminal outcomes. It shows elapsed and per-stage duration using one timer only while visible/active. Stale run events and offscreen updates cannot change another session's status.

Preparation cancellation invalidates its send. Active execution retains the existing Stop control. Saving has no fake cancellation button. Done enters Saving results; completion appears only after native save acknowledgement. Failure reads Answer complete / Save failed rather than pretending persistence succeeded.

Changes applied is based on successful tool file-diff metadata. Unstructured commands leave file changes unknown. Arbitrary command text or test-looking stdout never establishes verification success. Current native tools have no structured test-verification result or approval-request flow, so the UI does not invent passed/failed verification or enter approval state speculatively. Status is runtime-only, not a persisted execution audit log.

## 4. Native QA, recovery and stable fixtures

The opt-in `diagnostics` Cargo feature adds a sentinel-protected temporary profile and smoke CLI. It is excluded from shipping defaults; standard EXEs reject diagnostic flags. Profile selection precedes storage/skill/Tauri initialization; symlink/reparse contents and production profile locations are rejected. Engineer GUI runs use their own WebView profile and skip global shortcuts.

The PowerShell smoke runner starts a native writer, waits for a durable checkpoint marker, kills that process, and launches fresh recovery and verification processes. It then launches the real Tauri GUI and uses WebView2 CDP without mocking `__TAURI_INTERNALS__` to verify catalog/page/recovered-text/native rename/full-read/on-disk persistence and rendered DOM. No installer is executed and the user's installed application/profile is not replaced.

Latest diagnostic release EXE tested:

`E38419E34BE4784055C08BAABD06A9ED6B635267A1047554A7D7D574A3E47297`

Retained engineer executable: `verification/AetherAgent-0.3.0-diagnostics.exe`. It is not the shipping artifact.

Local workload: 1,000 messages, 2 MiB opaque PNG-signature payload, 40-message pages, 50 warm reads:

| Operation | Observed local sample |
|---|---:|
| Save 1,000 messages | 44.271 ms |
| Durable incremental checkpoint | 2.003 ms |
| First page with crash recovery | 37.372 ms |
| Recovery-process warm-page mean | 0.321 ms |
| New verification-process first page | 1.709 ms |
| Verification-process warm-page mean | 0.367 ms |

These are local native storage samples, not cold-disk benchmarks, FPS, image decoding or paid-provider measurements. The smoke emits repeatable JSON and checks data correctness; no hardware-independent millisecond release threshold is invented.

Fixture fixes preserve production deadlines: mock review peer has its own I/O executor, oversized provider events cross the same budget with fewer allocations, MCP uses ready handshakes and separates request timing from teardown, and idle-index tests inject elapsed time. UUID IDs and isolated journal directories prevent unrelated keyed locks/corruption fixtures from affecting each other. Corruption within the actual listed directory still fails visibly. Default-parallel native tests now pass; no RUST_TEST_THREADS restriction is used in the current final run.

Evidence: `verification/roadmap-native-summary.json`, `roadmap-native-smoke.log`, `roadmap-native-webview.png`, `native-smoke.ps1`, `native-webview-smoke.mjs`.

## 5. Attachment preview and search

### Attachments

PNG/JPEG/GIF/WebP receive an explicit preview gesture. Bytes are prepared lazily through the existing native bounded image API. Loaded previews use owned Blob URLs and remain as thumbnails after closing. Session/workspace/attachment scopes discard late results and revoke URLs. There are at most four preview controls; the preview cache is limited to eight entries / 20 MiB estimated storage. This cache is intentionally separate from send preflight.

The UI labels supported images Vision input, known text-only choices Vision blocked, and unsupported files File context. It does not guess a complete custom-model capability database. Preview images are browser-scaled, not native resized thumbnails.

### Search

FTS retains exact source message IDs. `load_session_around` performs an indexed lookup and loads up to 50 rows around the hit, rather than the entire session. Title/artifact-only hits still open a conversation.

Middle windows are detached read-only viewport state, never LiveSession/persistence/model input. Native saves, checkpoints and model hydration reject detached non-tail windows. Composer and mutation controls remain unavailable until Return to latest. Streaming underneath the preview stays with its original session.

Search targets are highlighted and centered using measured row geometry with bounded settling/resize observation. User interaction cancels settling. Timeline/prepend anchoring retain their separate semantics. Both banner and floating Return to latest exit preview; normal older-history navigation can also return to the loaded latest tail.

Final production-browser QA: **31/31 checks passed**, including search hit center error **0.25 px**, independent return controls, local Blob preview, text-only warning, native-contract-shaped delta sequencing, deferred save state, honest verification labels, About, math fonts, and 900px layout. No exceptions, console/network failures or unknown IPC. This browser uses in-memory IPC fixtures; native persistence evidence is separate.

Evidence and eleven screenshots: `verification/measured-jump-browser/`.

## 6. Release, update integrity and recovery

About shows actual version/build/platform/architecture, diagnostic task-log path, update configuration and local load metrics. Native metadata failure is surfaced with Retry; closing/reopening prevents late async results. Rechecking invalidates a previous installer offer. Opening About does not save settings.

The official Tauri updater 2.14.1 is wired through native commands, with build-time HTTPS endpoint/public key, constrained redirects, strictly newer semver, exact checked-version confirmation and authenticated signed-version binding. Without configuration, it fails closed and reports Automatic updates not configured. Check does not download or install. No untrusted IPC supplies an endpoint, public key or URL.

The signed-release script requires a real publisher certificate, timestamp service, updater private key and public configuration. It selects only fresh exact-version EXE/MSI outputs, cryptographically authenticates artifact bytes and version comments, validates Authenticode, and archives copies without overwriting. Previous-release records inside build outputs or junction aliases are rejected. Offline verification requires independent SHA-256 plus a trusted publisher certificate. Rollback is an explicit verified/manual recovery procedure with retained records, not an automatic schema downgrade.

Windows PowerShell 5.1 native stderr is handled explicitly, preserving true exit codes; informational Cargo stderr cannot falsely abort tests. Script coverage includes positive signatures from disposable fixture-only keys, wrong key/signature/version, tampering, stale outputs, path aliasing, archive collisions, hash mismatch and unsigned installer rejection.

**Unprovisioned:** no genuine code-signing certificate, production private key or update hosting endpoint was supplied/found for this build. Shipping 0.3.0 is unsigned; automatic updates are disabled. Cryptographic fixture success is not evidence of a trusted public release. No real signed upgrade, installer execution, installed-package smoke or prior-version data rollback has been performed.

## Quality gates

Final frontend suite: **58 files, 562 passed, 0 failed**. Final native default-parallel suite: **359 passed, 0 failed, 2 ignored subprocess fixtures**, plus successful main/doc tests. Clippy all-targets/all-features and Rustfmt pass after production-item ordering cleanup. Production build includes TypeScript and passes with a nonblocking >500 kB chunk advisory.

Release-tool verification: five isolated Rust policy/signature tests, PowerShell archive/hash/signature fixtures, real PS5 subprocess execution, and 21 version/artifact-planning assertions pass. The diagnostic-profile test and opt-in native release smoke pass.

Logs: `verification/roadmap-frontend.log`, `roadmap-rust.log`, `release-version-clippy.log`, `roadmap-build.log`, `roadmap-release-policy.log`, `roadmap-whitespace.log`.

Shipping release build completed with exit **0**. Executable is x64 PE, product/MSI version 0.3.0, and all artifact times are newer than this build's start. NSIS installer: **6.06 MiB**; MSI: **9.14 MiB**; direct executable: **16.79 MiB**. SHA-256 for the direct EXE is `4C829819C6EBA7AFBE5AE14534F22A4A7A34B204F747F777A9629866148BC20D`. Artifacts are explicitly `NotSigned`.

Shipping release build and checksum/metadata verification: `verification/roadmap-release-build.log`, `roadmap-release-artifacts.json`, `roadmap-release-sha256.txt`. The separately compiled shipping build omits engineer diagnostics; the recorded native smoke hash is deliberately not claimed to match that artifact. A direct shipping-EXE gate confirmed diagnostic flags are rejected with exit code 2 (`verification/roadmap-shipping-gate.txt`). Installers were not run.

## Explicit remaining boundaries

- Sidecars are cumulative active-run snapshots. Finalization/recovery still performs full canonical writes, and cold indexing still scans/decodes canonical data.
- SQLite shared connection work can still serialize; idle budgets are cooperative and pinned cache entries can exceed caps.
- No structured verification or actual tool-approval protocol was fabricated; status and timing are runtime-only.
- No automatic blob migration, native image resampling, FPS/RSS/CPU guarantee, paid-provider smoke or schema rollback was added.
- Signing, public update hosting, real installed upgrade/rollback and Windows installer acceptance remain operator release gates.
- No user history was deleted, no production installation replaced, and no commit/push made.
