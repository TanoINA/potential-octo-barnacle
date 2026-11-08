---
category: react-ui-state
description: Use when implementing expandable execution traces that should open during active work, collapse on completion, and still respect manual toggles within each lifecycle phase.
metadata:
  origin: aether-reflection
name: phase-scoped-execution-trace-controls
---
# Phase-scoped execution trace controls

- Represent the manual collapse preference as `boolean | null`; use `null` to select the lifecycle default.
- Derive the default from activity, not failure: use `manualCollapsed ?? !hasRunning` when active traces should open and finished traces should collapse even after errors.
- Reset the manual preference when `hasRunning` changes. Preserve it across updates within the same phase so reopening completed details is not undone by later result updates.
- Keep failure visibility independent of expansion. Color the collapsed summary when failures exist rather than forcing failed traces to remain expanded.
- Bound expanded content with a maximum height, internal vertical scrolling, and contained overscroll so long traces do not dominate the surrounding page.
- Align status types and aggregate counts with emitted statuses. Count both `error` and `failed` when supported, and keep cancellations distinct from failures.

## Regression coverage

Test active-to-completed transitions after manual expansion, completion with an error, reopening completed details, and updates that remain in the completed phase. Verify failure summary styling, cancellation-only summaries, mixed failure-status counts, and a long expanded list with bounded internal scrolling. Run component tests and type checking after changing status unions.
