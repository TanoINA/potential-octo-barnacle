# Tools / protocol audit — 2026-11-08

## Scope and confidence
Static, read-only source comparison of `D:\AetherAgent` (A) and `D:\hermes-agent-main` (H), using PowerShell Get-Content and rg. Only this requested report was written; no source changes, server launches, package imports, credentials reads, or live integration tests. References below are relative to the indicated repository. Source presence is not proof of installed dependencies, credentials, enabled toolsets, or packaged executable parity.

## Executive conclusion
Aether has a real, small native coding-tool implementation and real stdio MCP, skills and subagents—not merely a UI mock. Hermes has a substantially wider registry-driven tool/runtime platform, remote MCP/auth/lifecycle support, browser and background desktop automation, multimodal tools, and actual approval enforcement. The highest-priority Aether gap is not tool count: its Request Review mode is prompt instruction, not a dispatcher-enforced security boundary. Workspace file containment does not sandbox shell commands or MCP children.

## Actual tool surface
A `src-tauri/src/agent/tools.rs:76-287` defines exactly **11** native model function names: read_file, write_file, edit_file, list_directory, search_files, run_command, manage_task, fetch_web_page, use_skill, spawn_subagent, learn_skill. Nine are dispatched in tools.rs:307-317; use_skill and spawn_subagent are handled explicitly in runner.rs:1178-1218. Thus the latter are not stubs just because execute_tool lacks their names. MCP schemas are appended at runner.rs:829 and dispatched at 1128-1177. Native schema count excludes MCP-dependent aliases and UI commands.

H `tools/registry.py:84-120` detects self-registering modules through AST and scans flat tools/*.py plus tools/*/tool.py. `model_tools.py:151-170` invokes builtin discovery separately from plugin discovery; model schemas are toolset/availability filtered. A PowerShell regex over actual top-level tools/*.py found **43 literal named registry.register calls / 43 distinct names**. This is a reproducible static lower bound, NOT a complete schema or enabled runtime total: loop-driven registrations, nested package entry points, plugins and MCP are excluded. No inflated full-runtime count is claimed.

## Execution environments and filesystem safety
- A local execution is real: tools.rs:995-1016 starts powershell.exe with NoProfile, NonInteractive and ExecutionPolicy Bypass on Windows, or sh -c otherwise. cwd is selected at 990-994, but is not an isolation boundary. Commands can address external paths or invoke arbitrary network/process operations.
- Background processes are real: tools.rs:977-987 invokes TaskManager; tasks.rs:66-128 spawns a process with piped stdin and a message channel; manage_task is implemented at tools.rs:1237. The process manager is a singleton (tasks.rs:45), so per-session ownership deserves a separate security review.
- A workspace path containment is real: tools.rs:1510-1557 rejects NT namespace paths, missing workspace, parent traversal, canonicalized escapes and escaped existing ancestors. Exact edits and guarded atomic writes are implemented at 508-665. This protects native file tools, not unrestricted shell or MCP.
- H built-in terminal backend table declares local, docker, modal, daytona, vercel_sandbox, ssh (tools/terminal_tool_backends.py:52); imports real local/Docker/Modal/SSH implementations at 12-16, lazy-loads Daytona/Vercel at 185-187, and invokes plugin environment providers at 237-256. Resource, volume, network and environment configuration is explicit at 55-77. These are implemented backend choices, not six simultaneously available sandboxes. Local/SSH are not automatically sandboxes; Docker/cloud containment depends on configuration and deployment.
- H persistent code execution is a separate registered tool execute_code (tools/code_execution_tool.py:969), rather than just calling a shell once.

## MCP protocol comparison
### Aether: implemented but narrow
- `agent/mcp.rs:24-39`: launches configured executable/args/env in workspace cwd using piped stdin/stdout; stderr is discarded. Config is command/args/env rather than URL/auth transport (`types.rs:93-98`). Environment inheritance is not cleared in this launch path.
- `mcp.rs:53-83`: line-delimited JSON reader resolves pending numeric IDs. Messages without numeric IDs are ignored; no server-notification routing or server-request responder is implemented here.
- `mcp.rs:93-106`: initialize uses protocolVersion 2024-11-05 and sends notifications/initialized; initialization response capabilities/version are not used to construct richer behavior.
- `mcp.rs:129-164`: concurrent request-ID pending map and fixed 20-second timeout, with cleanup on error/timeout.
- `mcp.rs:167-185`: tools/list and tools/call only. tools/list reads a single tools array; no nextCursor iteration.
- `mcp.rs:217-292`: connects configured servers, produces provider schemas, records warnings, routes aliases back to original names. Alias sanitation/64-character limits and dedup are actual code at 317-364, with tests at 601-738.
- `mcp.rs:302-314`: converts text blocks to joined text; if any text exists, nontext siblings are not represented in that output. Otherwise JSON fallback. Not a native multimodal MCP tool-result pipeline.
- `runner.rs:1143-1167`: honors isError and creates a long-text artifact; cancellation is selected at 1129-1132. `mcp.rs:188-205` kills child tree on shutdown.
- Missing in this implementation: HTTP/SSE transports, OAuth lifecycle, prompt/resource utility tools, pagination, listChanged refresh, richer capability negotiation, server-side request handling and robust reconnect/cache lifecycle. Env variables can carry credentials to local servers, but that is not OAuth support.

### Hermes: implementation evidence, not catalog claims
- `tools/mcp_tool_transport.py:365-414`: stdio connection with safe-env resolution/preflight and process tracking.
- `mcp_tool_transport.py:520-560`: OAuth provider creation and SSE transport including forwarded auth; :562-659 streamable HTTP and constrained SSE fallback. Client certificate/TLS/header handling participates in the transport path, not merely docs.
- `mcp_tool_transport.py:691-692`: tool enumeration through pagination helper; `mcp_tool_handlers.py:831-839` paginates resources/list and prompts/list and implements resources/read. Registration includes get_prompt as well (`mcp_tool_registration.py:19-35`).
- `mcp_tool_handlers.py:315-338`: OAuth recovery/needs_reauth; :346-444 session-expiry/dead-child recovery with attention to retry ambiguity; :569 onward supports image/audio caching and resource rendering.
- `mcp_tool_discovery.py:544-572`: restricted worker handling and lazy schema-cache discovery. :764-766 reconnects when resolved launch inputs change. :870-883 exposes transport/live/auth status.
- `mcp_tool_registration.py:463-478`: connection identities account for authentication/TLS/client identity, rather than borrowing connections based only on server name.
- These are core MCP implementations. No tools from optional-mcps or plugin-catalog were counted as builtins.

## Browser, computer, web and media
**Important correction:** H tools/browser_tool.py:1-11 is explicitly a frozen updater compatibility stub; legacy browser_* tools and agent-browser are gone. Do not count browser_navigate/browser_click/browser_snapshot as current builtins. Current browser builtin is **browser_exec** registered in tools/browser_use_cli.py:1035-1038, with schema at :989 and executable implementation at :774. browser_cdp, browser_dialog and browser_vault are also literal core registrations. Browser backend/install prerequisites still matter.

H computer_use is genuinely registered in tools/computer_use_tool.py:13-26 and imports handler/checker from tools/computer_use/tool.py. The latter defines multimodal capture results at :1-4, blocks unsafe shortcuts/typed shell patterns before approval at :42-76, and has session-specific backend state at :83-97. This is a core wrapper/implementation, not evidence every platform driver is installed.

H actual registration evidence:
- web_search / web_extract — tools/web_tools.py:560,566.
- image_generate — tools/image_generation_tool.py:1050 (plugin provider lookup also exists at :573; distinguish registered core surface from selected provider).
- vision_analyze / video_analyze — tools/vision_tools.py:1031,1186.
- video_generate — tools/video_generation_tool.py:368.
- text_to_speech — tools/tts_tool.py:739.
- transcription implementations exist in tools/transcription_tools.py, but this audit did NOT identify a corresponding model registry registration, so STT is not counted as an extra builtin model function.

A has only fetch_web_page in this category: tools.rs:1299-1403 implements bounded public-URL HTTP extraction; :1310-1335 sets timeouts, disables automatic redirects and validates each redirect target. Private IP checks are real at :1404-1508. It is not search, a JS browser, screenshot/click automation or media analysis/generation. Installed computer-use, Google Workspace, image or other SKILL.md instructions are not equivalent to native tool implementations. A lib.rs:618-627 opens a URL in the default browser; that is also not browser automation. Image input/UI presentation, if present elsewhere, must not be conflated with native media tools.

## Approvals, sandbox and plugin integration
### P0: A approval mode is advisory
A runner.rs:800-801 and :1370-1371 inject Request Review wording into prompts. The actual tool routing at :1128-1220 executes MCP/native tools without a corresponding approval decision/token. tools.rs:290-317 checks cancellation, not human authorization. ExecutionMode defaults to Turbo (types.rs:120-121,184). No claim of enforced Request Review or sandbox should be made from this prompt/UI setting.

H terminal_tool.py:1562 calls _run_approval_guards before launching (:1575-1588). :1127-1138 produces a blocked result when approval is absent/denied. There are configurable auto-approval paths (:1140-1146), so this is an actual configurable gate, not a claim of universal manual confirmation. Memory/skill write approval is optionally enforced/staged by tools/write_approval.py:201-216; it defaults off when unset (:44-55). computer_use separately hard-blocks unsafe inputs as above.

A Tauri plugins (lib.rs:834-838: shell/fs/dialog/global shortcut) are app framework extensions, NOT an agent plugin discovery/provider ecosystem. Skills are prompt packages; MCP is an external-tool extension mechanism. No analogous native agent plugin host was found in audited A source paths.

H model_tools.py:159-167 calls hermes_cli.plugins.discover_plugins. hermes_cli/plugins.py:1334,1444 discovers entry points; :569 constrains builtin overrides, and :1820-1825 exposes prompt-section/middleware hooks. Terminal providers are integrated into dispatch as above. tools/plugin_guard.py:383-408 scans installation candidates and returns allow/confirmation/block outcomes. These guards are not an OS sandbox for installed Python plugins.

`plugin-catalog/*.yaml` is a catalog, not installed implementation; external integrations such as aave-mcp or acedatacloud-media were observed there but are NOT credited as builtins. `plugins/` also contains shipped provider implementation families, which are different from catalog metadata and may need enabling/dependencies. Optional MCP/skills likewise do not add default builtin model schemas.

## Missing tools ranked by practical value
1. **P0 — enforce approval before adding more power:** common dispatcher gate for command/native writes/MCP, cancellation-safe pending approval, scoped grants, child/subagent inheritance and tests proving denied operations never reach handlers. Separate this from any eventual sandbox.
2. **P1 — web_search + richer web_extract:** highest everyday research value beyond A fetch_web_page; add provenance/citations, output bounds and URL policy. Hermes core names are web_search/web_extract, not generic catalog claims.
3. **P1 — browser_exec and computer_use:** authenticated JS applications, forms, downloads, GUI-only workflows. Require isolated profiles, session ownership, screenshot/multimodal delivery and approvals; do not simply expose arbitrary automation through an advisory mode.
4. **P1 — HTTP MCP + OAuth + paginated discovery:** unlock hosted enterprise integrations; include prompts/resources and multimodal results, reconnect/schema invalidation, trust/allowlist policy, safe environment handling and redacted diagnostics.
5. **P1 — selectable execution backend:** Docker or remote SSH/cloud runtime behind an environment interface; document network/mount/secret boundaries. A current cwd restriction is not a substitute.
6. **P2 — execute_code:** persistent computation/RPC kernels for analysis and repeatable structured outputs, separately constrained from host shell.
7. **P2 — vision_analyze/video_analyze + image_generate:** multimodal workflows, with provider capability/credential checks, budgets and artifact delivery. Add video_generate/text_to_speech only where product demand justifies cost and dependencies.
8. **P2 — durable memory/session_search/todo_list/cronjob_manage:** these literal Hermes core names offer continuity, retrieval and scheduled work. A UI history or process manager is not equivalent to a model-callable scheduler or memory tool.
9. **P3 — governed plugin host:** provider/environment interfaces, package lifecycle, provenance and explicit trust boundaries; do not treat a large integration catalog as executable feature parity.

## Verification limits
No runtime tool import/count was attempted because Hermes discovery can initialize plugins, caches or integration state and would violate the intended read-only audit. Tests visible in A include MCP real-child roundtrip/alias tests (mcp.rs:406 onward), shell cancellation/streaming tests (tools.rs:1764-1838), and path escape tests (:1673-1712); they were inspected, not executed. This report does not certify either packaged EXE, deployed OAuth flows, browser drivers, actual sandbox isolation or installation completeness.
