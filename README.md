# AetherAgent

> [!NOTE]
> **WORK IN PROGRESS**
> This project is under active development. Features, architecture, and builds may change at any time.

Native Windows desktop AI coding assistant built with Tauri 2, Rust, and React.

## Stack

- Runtime & Core: Rust, Tauri 2
- UI: React 19, TypeScript, Tailwind CSS
- Protocols: Streaming SSE providers, local workspace tools, skills catalog, MCP stdio clients

## Development

Requires Node.js and Rust.

```powershell
# Install frontend dependencies
npm install

# Run dev mode
npm run tauri dev
```

Or run the helper script:
```powershell
.\run-aether.ps1
```

## Build

```powershell
npm run build
npm run tauri build
```

The release executable is generated at `src-tauri/target/release/aether-agent.exe`.
Pre-built verified binaries (`AetherAgent.exe` and `AetherAgent-Setup.exe`) are also available directly in the repository root.

## Local Data

Session history, workspace settings, and local configuration are saved at:
`%USERPROFILE%\.aether-agent\`
