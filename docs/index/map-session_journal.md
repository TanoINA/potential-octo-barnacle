# Module Guide: `src-tauri/src/agent/session_journal.rs`

The `session_journal` module provides append-only transaction logging and delta tracking for active conversation sessions, ensuring integrity and crash recovery without rewriting multi-megabyte JSON transcripts during rapid streaming turns.

## Core Responsibilities

1. **Incremental Delta Logging**:
   - Appends message increments, tool call updates, and artifact mutations as journal records.
   - Decouples in-memory live streaming from expensive canonical file rewrites.
2. **Crash Recovery and Baseline Replay**:
   - Reconstructs active session state from the last committed baseline and uncommitted journal entries if the app exits unexpectedly.
   - Validates message sequence numbers and cryptographic hashes to detect corruption or incomplete writes.
3. **Artifact Growth Tracking**:
   - Tracks incremental artifact data (e.g. streaming terminal output or code diffs) to prevent data loss between flush intervals.
4. **Checkpoint Synchronization**:
   - Coordinates with `storage.rs` to periodically materialize journal deltas into canonical `.checkpoint` sidecars.

## Key Types and Functions

- `Journal`: In-memory handle representing the active append log for an open session.
- `Baseline`: Committed disk snapshot used as the starting point for incremental journal deltas.
- `validate_artifacts`: Checks stored artifact references and payloads against session records.
- `checkpoint_counter`: Generates monotonically increasing revision IDs for save conflict detection.
