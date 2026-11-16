# Project Skills and Tauri Configuration

This index covers workspace-level skills and core Tauri application configuration.

## Tauri Application Setup

- **`src-tauri/Cargo.toml`**: Rust workspace manifest, dependencies, and feature flags.
- **`src-tauri/tauri.conf.json`**: Primary Tauri configuration (app identity, window dimensions, plugins, and bundle targets).
- **`src-tauri/tauri.linux.conf.json`**: Linux-specific Tauri bundle configuration.
- **`src-tauri/build.rs`**: Build script capturing compile-time version and git commit metadata.
- **`src-tauri/capabilities/default.json`**: Tauri 2 application capability and permission definitions.

## Workspace Skills (`.aether/skills/`)

- **`ponytail`**: Lazy senior developer mode focusing on minimal, complete solutions.
- **`ponytail-audit`**: Repository-wide code quality and bloat audit.
- **`ponytail-review`**: Focused code review for changes, testing coverage, and correctness.
- **`ponytail-debt`**: Ledger tracking deferred shortcuts and technical debt.
- **`ponytail-gain`**: Metrics on lines of code and complexity savings.
- **`ponytail-help`**: Quick command and parameter reference.
