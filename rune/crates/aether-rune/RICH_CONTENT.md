# Native rich content integration

Owned files: `src/rich_content.rs`, `src/rich_content_tests.rs`.

The main application manifest is intentionally not created or modified here.
Its owner must add:

```toml
# Existing Rune dependencies must enable these features:
# rune: features = ["svg"]
# rune-kit: features = ["markdown"]
latex-rust = "=3.0.2"
pulldown-cmark = { version = "0.13", default-features = false }
```

`latex-rust` requires **Rust 1.99**. This host has 1.99.1. It embeds STIX Two
Math; preserve dependency font/license notices when packaging. No Typst,
MiTeX, JavaScript runtime, WebView, HTML renderer, TeX install, or network is
required. `math-core` was inspected but rejected because it emits MathML,
not laid-out drawing primitives.

```rust
mod rich_content;
// Application state:
// rich: RefCell<rich_content::RichContent>
// In App setup, use the shell-provided waker, not a no-op:
// rich: RefCell::new(RichContent::new(wake.clone()))
// In the view:
// s.rich.borrow_mut().scale = cx.env.scale; // use your actual Env reference
// s.rich.borrow_mut().render(&message.text, &message.id, kit)
```

`render` is read-only. `render_with_tasks::<App>(..., callback)` routes GFM
checkbox interactions back to application state as `TaskChange` with original
source snapshot, byte offset, Kit block key, and checked state. Call
`apply_task_change(&mut message.text, &change)` to reject stale events safely.
The parent must persist its own edits. The standalone `markdown::<App>` helper
uses Kit directly with selectable/source-copy/wide-scroll enabled.

## Verified behavior

- Rune Kit provides GFM tables, highlighted fenced/indented code, diff fences,
  selectable grouped text and native task checkboxes. Its pinned API has an
  internal content-addressed parse cache; no public `Document` handle exists.
- `$...$`, `$$...$$`, `\(...\)`, and `\[...\]` formulas are extracted outside
  code, links, images, HTML ranges and reference definitions. The original
  document and formula delimiters remain available verbatim.
- Actual fractions, subscripts/superscripts, radicals, sums, integrals, aligned,
  matrices and cases produce native glyph-path SVG and rasterize in Rune.
- Unknown commands, malformed input, decode errors and limits are visible,
  with original TeX. Source text under a loaded formula is selectable.
- Whole-message source copy uses original input, never generated image URLs.
- External Markdown images do not load. Only generated formula handles are
  resolved, and a user-supplied internal-looking URL cannot spoof a formula.
- One math worker, bounded nonblocking queue, content/theme/scale cache keys,
  bounded document cache, bounded formula source/depth/grid/dimensions/output
  and explicit errors. Image decoding occurs on Rune image workers.

## Explicit limitations / remaining parent work

- Formula cards are NOT baseline-aligned inline math. Ordinary paragraph/list
  formulas split the text flow. Formulas inside a heading, emphasis or table
  retain a numbered reference with the equation card appended below the document.
- This is math-mode LaTeX, not arbitrary TeX documents, packages, TikZ or chemistry.
  Unknown commands are rejected, not silently rendered as raw-TeX success.
- GFM tasks are interactive via the callback. Special `plan` fences, three-state
  React plan cards, execution progress/persistence and other application-specific
  actions remain parent-panel work. No implicit app mutation or link opening.
- Partial selections copy the drawn selection, which may contain a generated
  equation reference. Select-all copies exact source; formula TeX is separately
  selectable. The module does not claim semantic MathML accessibility or one
  continuous native text run through formula glyphs.
- Limits: 1 MiB source; 64 documents / 4 MiB cached normalization; 96 formula
  occurrences per document; 4096-byte formula, parser depth 64, 128 ampersands,
  64 row breaks; 4096 x 2048 pt formula bounds; 1 MiB SVG; 1024-square raster;
  32 MiB formula cache including conservative decoded-mip estimates. Mounted
  Rune elements can retain image handles beyond cache eviction. A high-load
  document larger than the image budget is not an unlimited formula viewer.
- This module does not virtualize parent transcripts. Parent should render only
  mounted messages and use memoization. Rune Kit parse caching avoids reparse;
  its full built-subtree cache is bypassed for math image resolvers and callbacks.
- Tests use actual headless Rune Runtime and its memory clipboard, not a desktop
  window/GPU screenshot. Application integration/build is owned by the parent.

## Reproduce

`cargo test --manifest-path verification/native-rich-content/Cargo.toml --lib`

The isolated harness imports the actual owned source and local pinned Rune source.
`verification/native-rich-content/green.log` records the final regression result;
`red.log` records the initial extraction failure. `clippy.log` and
`rune-revision.txt` record lint output and the inspected Rune revision.
