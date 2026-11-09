//! Native Markdown and LaTeX. Requires rune/svg, rune-kit/markdown,
//! pulldown-cmark 0.13 (default-features=false), and latex-rust =3.0.2 (Rust 1.99).
//!
//! Construct once with the setup waker; keep in RefCell<RichContent> when a view
//! only borrows application state. No network, filesystem image resolver, HTML,
//! browser, JavaScript, or TeX subprocess is used. Kit owns Markdown's parse cache.
//! This module separately bounds document normalization, math jobs and live images.
//!
//! Limitations: formulas are native image cards, not baseline-aligned inline runs.
//! Formulas in tables/headings/emphasis get a numbered reference and a card below
//! the document. Math-only LaTeX is supported, not packages or document typesetting.
//! Unknown commands/errors remain visible with selectable original TeX.

use std::collections::VecDeque;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use pulldown_cmark::{Event, Options, Parser, Tag};
use rune::{El, Image, ImageCache, Waker, col, text};
use rune_kit::{Kit, Markdown};

const MAX_SOURCE: usize = 1024 * 1024;
const DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
const DOCUMENTS: usize = 64;
const MAX_TEX: usize = 4096;
const MAX_FORMULAS: usize = 96;
const MATH_BYTES: usize = 32 * 1024 * 1024;
const MAX_SVG: usize = 1024 * 1024;
const JOBS: usize = 8;

/// Read-only Markdown, including GFM tables, highlighted code, diffs and task lists.
/// The unused state parameter makes this usable alongside state-typed renderers.
/// Use RichContent::render for math and render_with_tasks for interactive plans.
pub fn markdown<S: 'static>(source: &str, key: &str, kit: &Kit) -> El {
    Markdown::new(source)
        .selectable(true)
        .copy_source(true)
        .scroll_wide(true)
        .build(kit)
        .key(key)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formula {
    pub tex: String,
    /// Delimiters included; never normalized or rewritten for copying.
    pub original: String,
    pub range: Range<usize>,
    pub display: bool,
    /// Inside a table/heading/styled run: rendered after the document instead.
    pub detached: bool,
    pub image_source: String,
}

#[derive(Clone, Debug)]
struct Edit {
    original: Range<usize>,
    rendered: Range<usize>,
}

/// Cached normalization result. The original is authoritative for copy and edits.
#[derive(Clone, Debug)]
pub struct Document {
    pub original: Arc<str>,
    pub markdown: String,
    pub formulas: Vec<Formula>,
    pub warnings: Vec<String>,
    edits: Vec<Edit>,
}

impl Document {
    pub fn parse(source: &str) -> Self {
        let mut doc = Self {
            original: source.into(),
            markdown: String::new(),
            formulas: vec![],
            warnings: vec![],
            edits: vec![],
        };
        if source.len() > MAX_SOURCE {
            doc.markdown = "Native rich content exceeds the 1 MiB rendering limit. Use the original source view.".into();
            doc.warnings
                .push("Rendering limit exceeded; original source retained for copying.".into());
            return doc;
        }
        if !source.contains('$') && !source.contains("\\(") && !source.contains("\\[") {
            doc.markdown = source.into();
            return doc;
        }
        // Parse only to identify protected byte ranges, never strip math out of code,
        // link destinations/labels, image alt text, HTML or reference definitions.
        let options =
            Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
        let parser = Parser::new_ext(source, options);
        let mut protected: Vec<Range<usize>> = parser
            .reference_definitions()
            .iter()
            .map(|(_, d)| d.span.clone())
            .collect();
        let mut detached = Vec::new();
        for (event, range) in parser.into_offset_iter() {
            match event {
                Event::Start(
                    Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. } | Tag::HtmlBlock,
                )
                | Event::Code(_)
                | Event::Html(_)
                | Event::InlineHtml(_) => protected.push(range),
                Event::Start(
                    Tag::Table(_)
                    | Tag::Heading { .. }
                    | Tag::Emphasis
                    | Tag::Strong
                    | Tag::Strikethrough,
                ) => detached.push(range),
                _ => {}
            }
        }
        protected.sort_by_key(|r| r.start);
        let mut cursor = 0;
        for range in protected {
            let start = range.start.max(cursor);
            if start > cursor {
                doc.scan(source, cursor..start, &detached);
            }
            if range.end > start {
                doc.markdown.push_str(&source[start..range.end]);
            }
            cursor = cursor.max(range.end);
        }
        if cursor < source.len() {
            doc.scan(source, cursor..source.len(), &detached);
        }
        for (i, formula) in doc.formulas.iter().enumerate().filter(|(_, f)| f.detached) {
            doc.markdown.push_str(&format!(
                "\n\n![Equation {}]({})\n",
                i + 1,
                formula.image_source
            ));
        }
        doc
    }

    fn scan(&mut self, source: &str, range: Range<usize>, detached: &[Range<usize>]) {
        let mut i = range.start;
        let mut copied = i;
        while i < range.end {
            let rest = &source[i..range.end];
            let opener = if !escaped(source, i) && rest.starts_with("\\[") {
                Some(("\\[", "\\]", true))
            } else if !escaped(source, i) && rest.starts_with("\\(") {
                Some(("\\(", "\\)", false))
            } else if !escaped(source, i) && rest.starts_with("$$") {
                Some(("$$", "$$", true))
            } else if !escaped(source, i)
                && rest.starts_with('$')
                && rest[1..].chars().next().is_some_and(|c| !c.is_whitespace())
            {
                Some(("$", "$", false))
            } else {
                None
            };
            let Some((open, close, display)) = opener else {
                i += source[i..].chars().next().unwrap().len_utf8();
                continue;
            };
            let body = i + open.len();
            let mut end = body;
            let mut found = None;
            while end < range.end {
                if !display && source.as_bytes()[end] == b'\n' {
                    break;
                }
                if source[end..range.end].starts_with(close) && !escaped(source, end) {
                    let dollar_ok = close != "$"
                        || (end > body
                            && !source[..end].chars().next_back().unwrap().is_whitespace()
                            && !source[end + 1..range.end]
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_digit()));
                    if dollar_ok {
                        found = Some(end);
                        break;
                    }
                }
                end += source[end..].chars().next().unwrap().len_utf8();
            }
            let Some(end) = found else {
                if open != "$" && self.warnings.len() < 16 {
                    self.warnings.push("Incomplete math delimiter; original text is shown until the formula is complete.".into());
                }
                i += open.len();
                continue;
            };
            if self.formulas.len() >= MAX_FORMULAS {
                if !self.warnings.iter().any(|w| w.starts_with("Math limit")) {
                    self.warnings.push("Math limit: at most 96 formulas per document; remaining source is shown unchanged.".into());
                }
                break;
            }
            let finish = end + close.len();
            let index = self.formulas.len();
            // Avoid accepting a user-written image as a generated formula, even if
            // the user guesses this module's private scheme and document indices.
            let mut image_source = format!("aether-native-math:{index}");
            while source.contains(&image_source) {
                image_source.push('x');
            }
            let is_detached = detached.iter().any(|r| r.start <= i && i < r.end);
            self.markdown.push_str(&source[copied..i]);
            let rendered_start = self.markdown.len();
            if is_detached {
                self.markdown
                    .push_str(&format!("[Equation {} - native formula below]", index + 1));
            } else {
                self.markdown.push_str(&format!(
                    "![Equation {} - native math]({image_source})",
                    index + 1
                ));
            }
            self.edits.push(Edit {
                original: i..finish,
                rendered: rendered_start..self.markdown.len(),
            });
            self.formulas.push(Formula {
                tex: source[body..end].into(),
                original: source[i..finish].into(),
                range: i..finish,
                display,
                detached: is_detached,
                image_source,
            });
            i = finish;
            copied = finish;
        }
        self.markdown.push_str(&source[copied..range.end]);
    }

    /// Map Kit's rewritten-source task marker back to the authoritative original.
    pub fn original_offset(&self, offset: usize) -> Option<usize> {
        let mut delta = 0isize;
        for edit in &self.edits {
            if offset < edit.rendered.start {
                break;
            }
            if offset < edit.rendered.end {
                return None;
            }
            delta += edit.original.len() as isize - edit.rendered.len() as isize;
        }
        let original = offset.checked_add_signed(delta)?;
        (original < self.original.len()).then_some(original)
    }

    fn bytes(&self) -> usize {
        self.original.len()
            + self.markdown.len()
            + self
                .formulas
                .iter()
                .map(|f| f.tex.len() + f.original.len() + f.image_source.len() + 128)
                .sum::<usize>()
    }
}

fn escaped(source: &str, at: usize) -> bool {
    source.as_bytes()[..at]
        .iter()
        .rev()
        .take_while(|b| **b == b'\\')
        .count()
        % 2
        == 1
}

#[derive(Clone, Debug)]
pub struct TaskChange {
    pub source: Arc<str>,
    /// Kit's generated block key; source offset is the authoritative identity.
    pub block_key: String,
    pub marker_offset: usize,
    pub checked: bool,
}

/// Safely apply a task callback only to the exact snapshot it was rendered from.
pub fn apply_task_change(source: &mut String, change: &TaskChange) -> bool {
    if source.as_str() != change.source.as_ref() {
        return false;
    }
    let at = change.marker_offset;
    if !matches!(
        source.as_bytes().get(at..at.saturating_add(3)),
        Some(b"[ ]" | b"[x]" | b"[X]")
    ) {
        return false;
    }
    source.replace_range(at + 1..at + 2, if change.checked { "x" } else { " " });
    true
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MathKey {
    tex: String,
    display: bool,
    color: [u8; 3],
    scale: u16,
}

#[derive(Clone, Debug)]
pub enum MathStatus {
    Pending,
    Ready {
        image: Image,
        width: f32,
        height: f32,
        svg_bytes: usize,
        raster_pixels: usize,
    },
    Unsupported(String),
}
impl MathStatus {
    fn bytes(&self) -> usize {
        match self {
            Self::Ready {
                svg_bytes,
                raster_pixels,
                ..
            } => svg_bytes + raster_pixels * 6,
            Self::Unsupported(s) => s.len(),
            Self::Pending => MAX_SVG + 1024 * 1024 * 6,
        }
    }
}
struct MathEntry {
    key: MathKey,
    status: Arc<Mutex<MathStatus>>,
}
struct Job {
    key: MathKey,
    status: Arc<Mutex<MathStatus>>,
}

type TaskHandler<S> = Rc<dyn Fn(&mut S, TaskChange)>;

pub struct RichContent {
    changed: Arc<AtomicBool>,
    documents: VecDeque<Arc<Document>>,
    formulas: VecDeque<MathEntry>,
    jobs: mpsc::SyncSender<Job>,
    /// Retains the decoders for this renderer's lifetime, including queued math.
    _images: Arc<ImageCache>,
    /// Device pixels per point; clamped to 1..=3. Formula rasters cap at 1024 squared.
    pub scale: f32,
    pub document_parses: usize,
    pub math_requests: usize,
}

impl RichContent {
    pub fn new(wake: Waker) -> Self {
        let changed = Arc::new(AtomicBool::new(false));
        let flag = changed.clone();
        let wake: Waker = Arc::new(move || {
            flag.store(true, Ordering::Release);
            wake();
        });
        let images = Arc::new(ImageCache::new(wake.clone()));
        let (jobs, receiver) = mpsc::sync_channel::<Job>(JOBS);
        let worker_images = images.clone();
        std::thread::Builder::new()
            .name("aether-native-math".into())
            .spawn(move || {
                let font = latex_rust::MathFont::stix_two_math().map_err(|e| e.to_string());
                while let Ok(job) = receiver.recv() {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let font = font.as_ref().map_err(Clone::clone)?;
                        let (svg, width, height) =
                            formula_svg(&job.key.tex, job.key.display, job.key.color, font)?;
                        let size = width.max(height).min(1024.0);
                        let scale = job.key.scale as f32 / 100.0;
                        let raster_side = (size * scale).ceil().clamp(1.0, 1024.0) as usize;
                        let image =
                            worker_images.svg(Arc::<[u8]>::from(svg.as_bytes()), size, scale);
                        Ok::<_, String>(MathStatus::Ready {
                            image,
                            width,
                            height,
                            svg_bytes: svg.len(),
                            raster_pixels: raster_side * raster_side,
                        })
                    }))
                    .unwrap_or_else(|_| {
                        Err("Native math renderer failed; original TeX is retained.".into())
                    });
                    *job.status.lock().unwrap_or_else(|e| e.into_inner()) =
                        result.unwrap_or_else(MathStatus::Unsupported);
                    wake();
                }
            })
            .expect("native math worker");
        Self {
            changed,
            documents: VecDeque::new(),
            formulas: VecDeque::new(),
            jobs,
            _images: images,
            scale: 1.0,
            document_parses: 0,
            math_requests: 0,
        }
    }

    /// Drain worker/decode invalidations even when the controller has no new events.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }

    pub fn document(&mut self, source: &str) -> Arc<Document> {
        if let Some(index) = self
            .documents
            .iter()
            .position(|d| d.original.as_ref() == source)
        {
            let doc = self.documents.remove(index).unwrap();
            self.documents.push_back(doc.clone());
            return doc;
        }
        self.document_parses += 1;
        let doc = Arc::new(Document::parse(source));
        let bytes = doc.bytes();
        if bytes <= DOCUMENT_BYTES {
            while self.documents.len() >= DOCUMENTS
                || self.documents.iter().map(|d| d.bytes()).sum::<usize>() + bytes > DOCUMENT_BYTES
            {
                self.documents.pop_front();
            }
            self.documents.push_back(doc.clone());
        }
        doc
    }

    fn request(&mut self, formula: &Formula, color: [u8; 3]) -> Arc<Mutex<MathStatus>> {
        if formula.tex.len() > MAX_TEX {
            return Arc::new(Mutex::new(MathStatus::Unsupported(
                "formula exceeds the 4096-byte limit".into(),
            )));
        }
        let key = MathKey {
            tex: formula.tex.clone(),
            display: formula.display,
            color,
            scale: (if self.scale.is_finite() {
                self.scale.clamp(1.0, 3.0)
            } else {
                1.0
            } * 100.0)
                .round() as u16,
        };
        if let Some(index) = self.formulas.iter().position(|e| e.key == key) {
            let entry = self.formulas.remove(index).unwrap();
            let status = entry.status.clone();
            self.formulas.push_back(entry);
            return status;
        }
        let bytes = |entries: &VecDeque<MathEntry>| {
            entries
                .iter()
                .map(|e| {
                    e.key.tex.len() + e.status.lock().unwrap_or_else(|e| e.into_inner()).bytes()
                })
                .sum::<usize>()
        };
        // Reserve the largest possible SVG+decoded mip chain before enqueueing,
        // so many simultaneously pending jobs cannot defeat the memory budget.
        let reservation = MAX_SVG + 1024 * 1024 * 6 + MAX_TEX;
        while !self.formulas.is_empty()
            && (self.formulas.len() >= MAX_FORMULAS
                || bytes(&self.formulas) + reservation > MATH_BYTES)
        {
            let removable = self.formulas.iter().position(|entry| {
                match &*entry.status.lock().unwrap_or_else(|e| e.into_inner()) {
                    MathStatus::Pending => false,
                    MathStatus::Ready { image, .. } => image.is_ready(),
                    MathStatus::Unsupported(_) => true,
                }
            });
            let Some(index) = removable else {
                return Arc::new(Mutex::new(MathStatus::Unsupported(
                    "Native math memory budget busy; retrying after pending formulas finish."
                        .into(),
                )));
            };
            self.formulas.remove(index);
        }
        let status = Arc::new(Mutex::new(MathStatus::Pending));
        let job = Job {
            key: key.clone(),
            status: status.clone(),
        };
        match self.jobs.try_send(job) {
            Ok(()) => {
                self.math_requests += 1;
                self.formulas.push_back(MathEntry {
                    key,
                    status: status.clone(),
                });
            }
            Err(mpsc::TrySendError::Full(_)) => {
                // Transient, deliberately not cached; completing jobs wake the view
                // and let it retry instead of permanently rejecting a large message.
                *status.lock().unwrap() = MathStatus::Unsupported(
                    "Native math queue busy; retrying after pending formulas finish.".into(),
                );
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                *status.lock().unwrap() =
                    MathStatus::Unsupported("Native math worker stopped.".into());
            }
        }
        status
    }

    pub fn render(&mut self, source: &str, key: &str, kit: &Kit) -> El {
        self.render_inner::<()>(source, key, kit, None)
    }

    pub fn render_with_tasks<S: 'static>(
        &mut self,
        source: &str,
        key: &str,
        kit: &Kit,
        on_task: impl Fn(&mut S, TaskChange) + 'static,
    ) -> El {
        self.render_inner(source, key, kit, Some(Rc::new(on_task)))
    }

    fn render_inner<S: 'static>(
        &mut self,
        source: &str,
        key: &str,
        kit: &Kit,
        on_task: Option<TaskHandler<S>>,
    ) -> El {
        let doc = self.document(source);
        if doc.formulas.is_empty() && doc.warnings.is_empty() && on_task.is_none() {
            return markdown::<S>(source, key, kit);
        }
        let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let color = [
            channel(kit.c.text_base.r),
            channel(kit.c.text_base.g),
            channel(kit.c.text_base.b),
        ];
        let mut pictures = Vec::new();
        let mut notices = doc.warnings.clone();
        for (index, formula) in doc.formulas.iter().enumerate() {
            let state = self.request(formula, color);
            let status = state.lock().unwrap_or_else(|e| e.into_inner()).clone();
            match status {
                MathStatus::Ready {
                    image,
                    width,
                    height,
                    ..
                } => {
                    if let Some(error) = image.error() {
                        notices.push(format!(
                            "Equation {}: native SVG decode failed: {error}\n{}",
                            index + 1,
                            formula.original
                        ));
                    }
                    pictures.push((
                        formula.image_source.clone(),
                        image,
                        width,
                        height,
                        formula.original.clone(),
                    ));
                }
                MathStatus::Unsupported(reason) => notices.push(format!(
                    "Equation {} - unsupported native math: {reason}\n{}",
                    index + 1,
                    formula.original
                )),
                MathStatus::Pending => {}
            }
        }
        let pictures = Rc::new(pictures);
        let resolve = pictures.clone();
        let frame = pictures.clone();
        let mut md = Markdown::new(doc.markdown.clone())
            .selectable(true)
            .scroll_wide(true)
            // Resolve ONLY generated equations; untrusted markdown cannot read a local
            // file, trigger HTTP, or submit arbitrary SVG to the image decoder.
            .images(move |src| resolve.iter().find(|p| p.0 == src).map(|p| p.1.clone()))
            .image_frame(move |image, info| {
                let Some((_, _, width, height, original)) = frame.iter().find(|p| p.0 == info.src)
                else {
                    return image;
                };
                let ink = image
                    .w(*width)
                    .h(*height)
                    .max_w(*width)
                    .label(format!("LaTeX equation: {original}"));
                col()
                    .w_full()
                    .gap(4.0)
                    .child(col().w_full().scroll_x().child(ink))
                    .child(
                        text(format!("TeX: {original}"))
                            .font(rune::TextStyle::new(11.0, 400.0))
                            .selectable(true),
                    )
            });
        if let Some(on_task) = on_task {
            let original = doc.clone();
            md = md.on_task(move |state: &mut S, block, offset, checked| {
                let Some(marker_offset) = original.original_offset(offset) else {
                    return;
                };
                on_task(
                    state,
                    TaskChange {
                        source: original.original.clone(),
                        block_key: block.into(),
                        marker_offset,
                        checked,
                    },
                );
            });
        }
        let original = doc.original.clone();
        // Do not set Kit copy_source on rewritten Markdown: it would copy synthetic
        // image URLs. A whole selection instead copies the exact original message.
        let mut result = col()
            .key(key)
            .w_full()
            .gap(8.0)
            .selection_group(true)
            .selection_copy(move |selection| {
                selection
                    .texts()
                    .iter()
                    .all(|t| t.is_whole())
                    .then(|| original.to_string())
            })
            .child(md.build(kit).selection_group(false));
        for notice in notices {
            result = result.child(
                text(notice)
                    .color(kit.c.text_muted)
                    .font(rune::TextStyle::new(12.0, 400.0))
                    .selectable(true)
                    .role("status"),
            );
        }
        result
    }
}

/// Synchronous engine boundary for worker and golden tests. It emits actual glyph
/// paths, not MathML/HTML or a raw-TeX image. Source, depth and output are bounded.
pub fn render_formula_svg(tex: &str, display: bool) -> Result<String, String> {
    let font = latex_rust::MathFont::stix_two_math().map_err(|e| e.to_string())?;
    formula_svg(tex, display, [24, 24, 24], &font).map(|v| v.0)
}

fn formula_svg(
    tex: &str,
    display: bool,
    color: [u8; 3],
    font: &latex_rust::MathFont,
) -> Result<(String, f32, f32), String> {
    use latex_rust::{Color, Dim, MathStyle, ParseOptions, SvgOptions};
    if tex.len() > MAX_TEX {
        return Err("formula exceeds the 4096-byte limit".into());
    }
    if tex.matches('&').count() > 128 || tex.matches("\\\\").count() > 64 {
        return Err("formula exceeds the matrix/alignment limit".into());
    }
    let mut options = ParseOptions::new();
    options.max_depth = 64;
    let (ast, _) = latex_rust::parse_with_options(tex, &options).map_err(|e| e.to_string())?;
    let mut svg_options = SvgOptions::new();
    svg_options.display = display;
    svg_options.font_size_pt = Dim::from_i64(if display { 20 } else { 16 });
    svg_options.color = Color::rgb(color[0], color[1], color[2]);
    let tree = latex_rust::layout_with_em_size_pt(
        &ast,
        font,
        if display {
            MathStyle::Display
        } else {
            MathStyle::Text
        },
        &svg_options.font_size_pt,
    )
    .map_err(|e| e.to_string())?;
    let width = (&tree.width * &svg_options.font_size_pt)
        .to_svg_string()
        .parse::<f32>()
        .map_err(|e| e.to_string())?;
    let height = ((&tree.height + &tree.depth) * &svg_options.font_size_pt)
        .to_svg_string()
        .parse::<f32>()
        .map_err(|e| e.to_string())?;
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || width > 4096.0
        || height > 2048.0
    {
        return Err("formula exceeds native layout dimensions (4096 - 2048 points)".into());
    }
    let svg = latex_rust::render_svg(&tree, font, &svg_options).map_err(|e| e.to_string())?;
    if svg.len() > MAX_SVG {
        return Err("formula SVG exceeds the 1 MiB limit".into());
    }
    Ok((svg, width, height))
}

#[cfg(test)]
#[path = "rich_content_tests.rs"]
mod tests;
