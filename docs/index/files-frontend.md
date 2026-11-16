# Frontend Files (`src/`)

This index lists the React components, custom hooks, and utilities that make up the AetherAgent webview interface.

## Core Shell & Layout

- **`src/App.tsx`**: Top-level shell hosting the sidebar, active canvas, modal overlays, and global keyboard shortcuts.
- **`src/components/ChatCanvas.tsx`**: Main conversation view, composer input, slash command autocompletion, and prompt queue.
- **`src/components/ChatHeader.tsx`**: Header bar displaying current model, reasoning effort, token context meter, and quick controls.
- **`src/components/VirtualizedMessageList.tsx`**: Virtualized message list handling smooth scrolling and anchor stabilization.
- **`src/components/ContextMenu.tsx`**: Right-click actions for copying code, editing turns, undoing, and rerunning prompts.

## Agent Cards & Rich Content

- **`src/components/ActivityTray.tsx`**: Collapsible tray summarizing active tools, skills, and background tasks.
- **`src/components/ExecutionTrace.tsx`**: Detailed expandable trace of tool calls, inputs, and outputs.
- **`src/components/SubagentTreeCard.tsx`**: Hierarchical card showing subagent progress, role, and completed turns.
- **`src/components/AskQuestionCard.tsx`**: Interactive multi-choice prompt card for the `ask_question` tool.
- **`src/components/InteractivePlanCard.tsx`**: Multi-step plan checklist tracking task progress.
- **`src/components/FilesChangedCard.tsx`**: Summary card showing files created, edited, or deleted during a turn.
- **`src/components/InteractiveTerminal.tsx`**: Terminal emulator component for interactive command sessions.

## Modals & Overlays

- **`src/components/SettingsModal.tsx`**: Configuration modal for providers, model defaults, MCP servers, and reflection cadence.
- **`src/components/ModelPickerModal.tsx`**: Model search and selector overlay (`Ctrl+K`).
- **`src/components/SkillPickerModal.tsx`**: Skill search and attachment popup.
- **`src/components/ArtifactsLibraryModal.tsx`**: Browser for saved code snippets, diffs, and generated artifacts.
- **`src/components/SessionHistoryModal.tsx`**: Chat session search and timeline view.
- **`src/components/AboutModal.tsx`**: App version info, update status, and diagnostic logs.

## Custom Hooks (`src/hooks/`)

- **`useAgent.ts`**: Central application state hook managing active runs, streaming deltas, and session updates.
- **`useAgent.tasks.ts`**: Tracks long-running background tasks and processes.
- **`useAgent.reflection.ts`**: Coordinates self-improvement review proposals and approval badges.
- **`useAgent.pagination.ts`**: Manages lazy loading of older turns and cursor positions.
- **`useHistoryPreview.ts`**: Provides read-only timeline preview when jumping to older turns.
- **`useSpeechRecognition.ts`**: Web Speech API integration for optional voice dictation.

## Types and Utilities

- **`src/types/agent.ts`**: TypeScript definitions for sessions, messages, tool calls, and configuration options.
- **`src/utils/sessionCacheBudget.ts`**: Memory budget calculations for cached session histories.
- **`src/utils/encoding.ts`**: UTF-8, base64, and string formatting utilities.
