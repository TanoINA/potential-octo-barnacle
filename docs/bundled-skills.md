# Bundled skills

AetherAgent embeds starter skill text in its Rust binary. On startup, it creates missing `SKILL.md` files under `~/.aether-agent/skills/<name>/`. The installer does not need a Git checkout or a network download to supply these guides. This is the existing startup seeding mechanism, not a new onboarding screen or marketplace.

## Included guides

| Group | Skills |
| --- | --- |
| Core workflow | `codebase-inspection`, `systematic-debugging`, `test-driven-development`, `verification-before-completion` |
| Minimal implementation and review | `ponytail`, `ponytail-audit`, `ponytail-debt`, `ponytail-gain`, `ponytail-help`, `ponytail-review` |
| Web development | `frontend-design`, `webapp-testing`, `web-design-guidelines`, `react-best-practices` |

The five quality/web additions are original AetherAgent guides in `.aether/skills/`. They are not copies of the Anthropic, Vercel, or Superpowers packages with similar names. In particular, they do not redistribute third-party document skills or imply an upstream endorsement. Their frontmatter describes task-specific triggers; React guidance applies only to React projects. The local web review checklist requires no remote rules download.

These guides add no runtime dependency. Browser tests still require the project's own browser tooling and installed browser runtime. Installing a skill does not install Playwright, Node.js, Python, or other tools.

## Existing installations and customization

- Only missing skill files are created. Existing files are preserved byte-for-byte, including edited guides and upstream packages using the same names.
- Updating AetherAgent therefore adds missing guides but does not upgrade or replace an existing guide with a matching name.
- Workspace skills under `.aether/skills/` take precedence over global skills with the same name. Other discovered skill sources retain their existing precedence.
- Optional skills, references, scripts, and user-created files are not removed or rewritten.
- If a skill directory cannot be written, startup continues without surfacing that write error. The skill may be absent; seeding is not a transactional package installation.

REA, specialist security tools, document suites, cloud integrations, and large artifact toolchains remain optional. Review their licenses, scripts, runtime requirements, and update policies before distributing them in an installer.

## Maintenance

Register new starter text in `ensure_starter_skills` in `src-tauri/src/agent/skills.rs`. Keep each new guide below 500 lines and the loader's 256 KiB per-file limit. A guide must have valid YAML frontmatter, a matching directory/name, a useful description, and an instruction body. Preserve the non-overwrite policy.

Focused checks (run from the repository root):

```sh
cargo test --manifest-path rune/Cargo.toml -p aether-core skills::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml skills::tests --lib
```

Tests cover fresh-install discovery, metadata, edited-file preservation, and optional user skills. Both applications compile the shared skills implementation; the embedded text ships with the next application build. An already-built installer is not updated by editing these source files.
