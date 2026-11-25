# Bundled skills

AetherAgent embeds starter skill text in its Rust binary. On startup, it creates missing `SKILL.md` files under `~/.aether-agent/skills/<name>/`. The installer does not need a Git checkout or a network download to supply these guides. This is the existing startup seeding mechanism, not a new onboarding screen or marketplace.

## Included guides

| Group | Skills |
| --- | --- |
| Core workflow | `codebase-inspection`, `systematic-debugging`, `test-driven-development`, `verification-before-completion` |
| Minimal implementation and review | `ponytail`, `ponytail-audit`, `ponytail-debt`, `ponytail-gain`, `ponytail-help`, `ponytail-review` |
| Web development | `frontend-design`, `webapp-testing`, `web-design-guidelines`, `react-best-practices` |

The five quality and web guides in `.aether/skills/` were written directly for AetherAgent. They do not bundle Anthropic, Vercel, or Superpowers files or imply any upstream affiliation. Each guide sets its own activation description in frontmatter (React rules trigger only in React projects), and the web review checklist runs locally without fetching remote rules.

None of these guides pull in extra runtime dependencies. If you run browser checks, you still need your project's own browser setup and runtime; seeding a skill does not install Playwright, Node.js, or Python.

## Existing installations and customization

- Startup only writes missing skill folders and leaves existing files untouched byte-for-byte, including customized guides or upstream skills sharing the same name.
- App updates add newly bundled guides without overwriting any existing folder that already has that name.
- Project skills under `.aether/skills/` override global skills with the same name, while other discovered roots keep their existing priority order.
- User scripts, reference files, and custom skills never get deleted or rewritten during seeding.
- If ~/.aether-agent/skills/ cannot be written on startup, the app logs nothing blocking and continues launching without that starter guide.

REA, specialist security suites, document tools, cloud plugins, and heavy artifact toolchains stay optional. Check their licenses, scripts, runtimes, and update terms before bundling them into a custom build.

## Maintenance

Register new starter text in `ensure_starter_skills` in `src-tauri/src/agent/skills.rs`. Keep each new guide below 500 lines and the loader's 256 KiB per-file limit. A guide must have valid YAML frontmatter, a matching directory/name, a useful description, and an instruction body. Preserve the non-overwrite policy.

Focused checks (run from the repository root):

```sh
cargo test --manifest-path rune/Cargo.toml -p aether-core skills::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml skills::tests --lib
```

Tests cover fresh-install discovery, metadata, edited-file preservation, and optional user skills. Both applications compile the shared skills implementation; the embedded text ships with the next application build. An already-built installer is not updated by editing these source files.
