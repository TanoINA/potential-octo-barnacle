# ⚡ AetherAgent

> **Native, Ultra-Fast AI Coding Agent powered by Rust & Tauri 2.0**  
> Combines the **unrestricted custom provider flexibility of Hermes** with the **buttery-smooth 60 FPS performance, DOM virtualization, and Artifact sidecar architecture of Antigravity**.

---

## 🌟 Why AetherAgent?

| Feature | Hermes Desktop | AetherAgent (Rust + Tauri) |
| :--- | :--- | :--- |
| **Engine** | Node.js + Electron + Three.js | **Native Rust + Tauri 2.0 (WebView2)** |
| **RAM Usage** | ~600 MB - 1 GB | **~35 - 65 MB** |
| **Chat Performance** | Stutters on long sessions (no DOM virtualization) | **60 FPS smooth scrolling via `@tanstack/react-virtual`** |
| **Large Output Handling** | Dumps raw code cards directly into chat stream | **Artifact Sidecar Panel** (Chat stays ultra-light) |
| **Custom Providers** | Yes | **Yes (GutsAI, DeepSeek, Kimi, OpenAI, Ollama, OpenRouter)** |
| **Reasoning Display** | Basic | **Collapsible Thinking Accordion for DeepSeek R1 & Kimi** |

---

## 🚀 How to Run

### Quick Start (Dev Mode with Hot-Reload)
Run in PowerShell:
```powershell
./run-aether.ps1
```
Or manually:
```bash
npm run tauri dev
```

### Build Production Standalone `.exe`
```bash
npm run tauri build
```
The optimized single-file `.exe` will be generated in `src-tauri/target/release/aether-agent.exe`.

---

## 🛠️ Tech Stack
- **Agent Core & Tools:** Rust (Tokio, Reqwest, Serde)
- **GUI Engine:** Tauri 2.0 (Windows WebView2 native)
- **Frontend:** React 19, TypeScript, Vite 6, Tailwind CSS
- **Virtualization:** `@tanstack/react-virtual`
- **Tooling:** Native file I/O (`read_file`, `write_file`, `list_directory`), PowerShell command runner (`run_command`).
