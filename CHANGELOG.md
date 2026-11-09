# Changelog

## 0.3.0

### Desktop and conversation UX
- Simplified static Aether logo, empty state, sidebar hierarchy, composer, menus, plans and modal surfaces; finite motion with reduced-motion support.
- Lazy optional panels with local error boundaries, accessible focus management and non-blocking chat startup.
- Screenshot/paste/drop image attachment previews, native image validation, provider-specific multimodal payloads and actionable text-only model errors.
- Academic Markdown mathematics using remark-math/rehype-katex and local KaTeX assets, with prose/currency/code regressions covered.
- Scoped search/navigation, context-menu and task/status affordance fixes; regression tests for command, council and plan interactions.

### History, performance and persistence
- Indexed bounded session pages, older-message loading and virtualized scroll anchors without truncating persisted history or provider context.
- Immutable committed baselines, native revision acknowledgement/rebinding, run-scoped delta checkpoints and append journals protect history from stale and competing saves.
- Bounded byte-aware session caching, idle index maintenance and local-only session load samples.
- Reduced streaming publication and Markdown reparsing on unrelated updates; preserved task/artifact navigation without forcing panes open.

### Agent safety and skill maintenance
- Phase-scoped self-improvement review with configurable cadence, provider, destination and approval flow; exact linked-file inspection and staged proposal diffs.
- Update-first skill maintenance with scoped discovery, conflict checks and preserved metadata; no synthetic write success or escalation from review into unrestricted tools.
- Native workspace/path, task lifecycle, provider, image and persistence hardening plus isolated diagnostic/smoke-test foundations.

### Release foundation
- Synchronized exact product version 0.3.0 and compile-time UTC/git/dirty build identity; About displays platform, architecture, genuine task-log location and bounded local load diagnostics.
- Native official Tauri signed updater integration, public HTTPS/no-credential URL policy, strict newer-version gating, signed artifact-version binding, signature checks and explicit install confirmation.
- Automatic updates remain disabled unless a valid build-time endpoint and Ed25519 public key are configured. Installer signing is reported as unknown unless genuinely verified; no invented publisher or signing identity.
- Opt-in signed release script, SHA-256 plus pinned trusted Authenticode offline verifier, retained artifact records and explicit manual rollback procedure. No automatic downgrade, URL-based installer execution or claimed public deployment.
