# UI09 — execution trace truthfulness and accessible details

## Changes

- `src/components/ExecutionTrace.tsx`: error/nonzero-exit rows explicitly say **Failed**; interrupted rows say **Cancelled** instead of past-tense success. Cancellation takes precedence over an interrupt exit code.
- Mixed/incomplete/failed traces use a neutral step-count summary rather than claiming skills were loaded or commands completed.
- Every tool now has expandable argument/result details. Failed details open by default. Raw argument strings (including incomplete JSON) and results render as inert React text in bounded, selectable `<pre>` elements.
- Rows are native `type="button"` controls with visible focus styling. Disclosure controls expose `aria-expanded` and `aria-controls`. Native button semantics provide keyboard activation without custom key handlers.
- Existing nonterminal artifact-row clicks still select the artifact; a separate sibling Details button controls its inline details without selecting it. Terminal rows retain terminal expansion, output, exit badges, and copy behavior.
- Added `src/components/ExecutionTrace.test.tsx` (10 regression cases).

## Verification

- RED: initial focused test execution produced 3 expected failures for `write_file`, `use_skill`, and MCP nonterminal tools: no native failed disclosure and hidden error/results.
- GREEN: `npm test -- src/components/ExecutionTrace.test.tsx src/components/TerminalOutput.test.tsx` — **2 files passed, 11 tests passed**.
- Covers failed write/skill/MCP calls, inert HTML-like result/argument strings, collapse/reopen, native button semantics and focusability, three cancelled tool types, artifact selection separation, successful command output/exit/copy controls, raw incomplete arguments, and nonzero exit with `done` status.
- `git diff --check -- src/components/ExecutionTrace.tsx src/components/ExecutionTrace.test.tsx` — passed (Git emitted only an LF-to-CRLF working-copy warning).
- Full frontend gates remain with the parent agent as delegated. No browser-level keyboard or screen-reader run was performed; jsdom verifies native semantics/focus and activation through click, not browser-generated Enter/Space clicks.

## Scope and review

Only the component, its new test, and this audit note were edited. No commit was created; unrelated concurrent changes were left untouched.

Self-review: accuracy/completeness are supported by focused regression execution, with browser-level keyboard behavior and full frontend integration left explicitly unverified here. The implementation preserves existing terminal and artifact interactions without a visual redesign.
