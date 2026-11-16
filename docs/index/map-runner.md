# Module Guide: `src-tauri/src/agent/runner.rs`

The `runner` module implements the central autonomous execution loop for AetherAgent, managing conversation turns, model streaming, tool execution cycles, and subagent delegation.

## Core Responsibilities

1. **Agent Execution Loop (`AgentRunner::run`)**:
   - Assembles system prompt, workspace memory, active skills, and conversation history.
   - Dispatches prompts to the active provider and streams responses to the frontend.
   - Detects tool calls emitted by the model, validates arguments, and executes them against `tools.rs`.
   - Feeds tool execution results back to the model until it finishes with a final text response.
2. **Subagent Delegation (`run_subagent`)**:
   - Manages nested agent runs spawned by `invoke_subagent`.
   - Enforces execution limits (maximum turns, timeout boundaries, scoped toolsets).
   - Groups subagent activity cards in the UI and synthesizes child results back to the parent task.
3. **Context Optimization (`apply_micro_compaction`)**:
   - Evaluates context token estimates against model windows.
   - Truncates verbose older tool outputs while preserving recent tool turns and user instructions.
4. **Cancellation and Safety**:
   - Tracks active run IDs and cancellation permits (`UpdatePermit`).
   - Aborts in-flight streaming connections and marks pending tool calls as cancelled when interrupted.

## Key Types and Functions

- `AgentRunner`: Main execution struct holding references to storage, configuration, and event sinks.
- `UpdatePermit`: Concurrency gate that blocks updater installations while an agent run is active.
- `run`: Main entry point for single-model execution turns.
- `run_council`: Multi-model deliberation mode running two provider candidates in parallel.
- `load_selected_skill_context`: Injects user-selected skill files into the current prompt prefix.
