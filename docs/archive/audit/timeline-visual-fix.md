# Right-hand timeline visual fix

## User evidence and scope

Inspected the supplied 18.33-second MP4 through extracted frames and Windows OCR. It shows repeated navigation through the right-hand conversation timeline, not Council selection. Original media remains untouched.

The reproduction uses production React/TanStack/KaTeX with 50 long, variable-height messages and 25 user turns. Browser IPC is fixture-based; no paid provider or user session was modified.

## Confirmed causes

1. The virtual list begins 52 px into the scroll content, after padding and the history header. Timeline scrollToIndex used list-local coordinates without the application-level DOM correction already used for search.
2. Timeline active selection rounded physical scroll percentage into a turn number. That assumes equally tall turns and selects the wrong turn when long responses, code, or math vary in height.
3. After a large manual wheel movement, onScroll can run before TanStack mounts the newly visible row. Sampling only during that event retained the previous marker until another scroll event.

## Fix

- Generalized measured jump settling: timeline targets align at the actual viewport start; search retains center alignment. Uses at most three RAF passes plus target/viewport-only ResizeObserver settling for 800 ms.
- Explicit jumps cancel old prepend/viewport anchors. User input, newer jumps, navigation, loading and unmount cancel the observer/timer/frame. Normal prepend anchoring and timeline end clamping remain intact.
- Timeline selection receives the first visible source message index from ChatCanvas and resolves its owning user turn. The thumb still shows physical scroll percentage independently.
- After virtual-row layout commits, resample actual intersecting mounted rows; only update state if the index changed. No per-frame polling loop or complete-history DOM scan.
- Timeline ticks and popover rows are keyboard-operable buttons with aria-current. Stable tick geometry replaces hover scaling. Focus keeps the popover available; Escape closes it and returns focus, and hide timers are cleaned up.

## Verification

- Before: settled target offsets consistently +52 px; selected turn differed from the clicked/visible source.
- After: all 28 settled navigation scenarios measured exactly 0 px target offset, including cold, repeated and rapid clicks. Forty-four navigation clicks were exercised.
- All six manual wheel cases selected the actual visible owning turn in both tick and popover, upward and downward.
- Timeline production harness: **167 passed, 0 failed**.
- Full roadmap production browser regression: **31 passed, 0 failed**; search center error remained 0.25 px.
- Zero sampled blank viewports, header shifts, runtime exceptions, console errors, failed requests, HTTP errors or unknown IPC.
- Controlled geometry/unit integration: **108 passed across 10 focused files**; TypeScript and production build passed.
- Full frontend final result: **59 files, 574 passed, 0 failed**, recorded in `verification/timeline-final-frontend.log`.
- An unrelated jsdom-only 5 MiB preview-cache fixture exceeded its 5-second timeout in the initial full runs. Its cache logic and payload size remain real; browser Blob URL APIs and the exact fixture codec/byte conversion are scoped test doubles. The separate full-size real-decoder test remains unchanged. No production preview change, timeout increase, skip or assertion removal was made; the final fixture passed three focused repetitions and the whole preview file.

Reports: `verification/timeline-final-browser/report.md`, `report.json`, and `verification/timeline-final-roadmap/report.md`. Build: `verification/timeline-final-build.log`. Harness: `verification/timeline-browser.mjs`.

Five unique timeline screenshots are in `verification/timeline-final-browser/`; the report repeats one screenshot path across scenarios, so screenshot entries are not distinct file counts.

## Limits

No native/backend code changed; native Rust tests were not rerun for this frontend-only fix. Browser RAF sampling does not establish frame-by-frame absence of OS compositor glitches, and native WebView playback was not tested. Installer/executable from the preceding 0.3.0 build has not been rebuilt for this fix. No installation, commit, push or user data deletion was performed.
