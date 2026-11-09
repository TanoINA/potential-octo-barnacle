# Academic math formatting fix

## Cause

The chat used CommonMark/GFM without a math extension. CommonMark removes the punctuation escapes in `\(...\)` and `\[...\]`, leaving raw TeX commands such as `\begin{aligned}` visible instead of a typeset equation. Changing the model's answer or weakening the binary-file tools would not fix this rendering issue.

## Changes

- Added remark-math and rehype-katex with local KaTeX styles/fonts. Supports inline `$...$` / `\(...\)` and display `$$...$$` / `\[...\]`.
- Added source-range-aware delimiter normalization, protecting inline, fenced, indented code, links, images, definitions and HTML. A literal unmatched opener in code cannot consume a later valid formula.
- Original `math`-labelled fences remain copyable code; only actual math nodes are rendered as equations.
- Preserved module-level plugin identities and Markdown memoization. Ordinary content without backslash delimiters skips the normalization parse.
- Display equations scroll horizontally within their bubble; they do not widen the chat on narrow screens. MathML and original TeX annotations are retained.
- Incomplete streaming formulas remain readable, and malformed formulas fall back without crashing the message.
- KaTeX uses `trust: false`, bounded expansion/size, and no raw-HTML plugin.

## Dependencies

KaTeX is pinned through an npm override to the direct supported version range (`^0.19.1`) so remark/rehype do not load separate vulnerable 0.16.x instances or mismatched styles. `npm ls katex` verifies deduplication. `npm audit --omit=dev` reports zero vulnerabilities after the override.

KaTeX adds local font assets and JS/CSS. The production build currently emits an advisory for an entry chunk over 500 kB; build succeeds. This small rendering fix does not claim zero bundle-size cost or implement an additional lazy math loader.

## Verification

- Screenshot-style aligned T(n) calculation, inline complexity, fractions, matrices and table cells.
- Code fences (including language math), inline/indented code and literal openers are protected.
- Invalid/incomplete streaming TeX and untrusted link/HTML commands.
- Existing Markdown, InteractivePlanCard and memoized rendering regressions.
- Production Chromium: 11/11 checks; three MathML/KaTeX formulas rendered; local WOFF2 fonts HTTP 200; no console, runtime or network errors. At 390px, a 831px equation scrolls inside its 316px container while document/body remain 390px wide.

Browser evidence: `verification/math-browser-evidence/report.md`, `report.json`, `math-1280x900.png`, `math-390x900.png`. The browser uses actual production React with mocked Tauri IPC, not a native desktop install or paid provider.

Final gates: **46 frontend test files, 440 passed, 0 failed**; production build/TypeScript and whitespace check pass. No Rust changes were made for this fix, so the native suite was not rerun.

Final frontend logs: `verification/math-frontend.log`, `verification/math-focused.log`, `verification/math-build.log`, `verification/math-dependencies-audit.json`.

No Rust/backend behavior changed; no session contents were rewritten. This does not rebuild the executable/installers from the previous release. Properly delimited formulas in existing history render when displayed by the updated frontend; unmarked plain-text brackets are not guessed as math.
