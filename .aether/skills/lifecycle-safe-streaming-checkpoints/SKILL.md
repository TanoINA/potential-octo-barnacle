---
category: react-async-persistence
description: Use when adding periodic persistence to streaming React sessions, especially when background windows suspend animation frames or asynchronous saves can finish after cleanup.
metadata:
  origin: aether-reflection
name: lifecycle-safe-streaming-checkpoints
---
# Lifecycle-safe streaming checkpoints

- Schedule persistence with a timer independent of `requestAnimationFrame`; flush pending streaming buffers before taking the snapshot so hidden windows do not depend on rendering to reach disk.
- Keep checkpoint state per live session: timer handle, busy flag, and lifecycle generation. Allow at most one delayed timer and one queued or in-flight checkpoint per session rather than accumulating snapshots behind a slow disk.
- Capture the generation when scheduling. Check it before arming a timer and again inside its callback. Increment the generation during cleanup and clear pending timers, preventing an in-flight promise's completion handler from resurrecting a timer after unmount.
- After a save settles, rearm only if the session is still running and its data changed since the saved immutable snapshot. Route rearming through the generation-checked scheduler.
- Serialize checkpoint writes and final saves through the same persistence queue. On completion, cancel the delayed checkpoint, flush pending buffers, and enqueue the final save so an older checkpoint cannot overwrite final state.
- Reuse the native atomic replacement and synchronization path. Keep expensive indexing out of frequent checkpoint writes and reconcile it on final save when appropriate.

## Regression coverage

Test suspended animation frames, updates to offscreen sessions, slow persistence, checkpoint/final-save ordering, and cleanup while a checkpoint promise remains unresolved. Resolve that promise after unmount and advance timers to verify that no checkpoint is rearmed.

Treat periodic snapshots as bounded recovery, not an event journal or a guaranteed recovery interval: timer throttling and storage latency can extend the window of lost output.
