use super::*;
use rune::{Cx, Env, Runtime};
use std::cell::RefCell;
use std::time::{Duration, Instant};

#[test]
fn four_math_delimiters_keep_exact_source() {
    let source = r"Greek alpha $x^2$ and \(y_1\), $$\frac{1}{2}$$ and \[\sqrt{x}\].";
    let doc = Document::parse(source);
    assert_eq!(doc.formulas.len(), 4);
    assert_eq!(
        doc.formulas.iter().map(|f| f.display).collect::<Vec<_>>(),
        [false, false, true, true]
    );
    for formula in &doc.formulas {
        assert_eq!(&source[formula.range.clone()], formula.original);
    }
    assert_eq!(doc.original.as_ref(), source);
}

#[test]
fn code_links_images_html_and_definitions_are_protected() {
    let source = "```math\n$x$ \\(x\\)\n```\n\n`$x$`\n\n    $x$\n\n[label $x$](https://host.invalid/$x$)\n\n![alt $x$](file:///secret/$x$)\n\n<div>\\(x\\)</div>\n\n[ref]: https://host.invalid/$x$\n\nreal $y$";
    let doc = Document::parse(source);
    assert_eq!(doc.formulas.len(), 1, "{doc:?}");
    assert_eq!(doc.formulas[0].tex, "y");
    assert!(doc.markdown.contains("file:///secret/$x$"));
    assert!(doc.markdown.contains("```math\n$x$"));
}

#[test]
fn currency_escapes_and_incomplete_math_are_not_formulas() {
    let source = r"Cost $5 and $7; escaped \$x\$ and \\(z\\). Incomplete \[x";
    let doc = Document::parse(source);
    assert!(doc.formulas.is_empty(), "{:?}", doc.formulas);
    assert_eq!(doc.markdown, source);
    assert!(!doc.warnings.is_empty());
}

#[test]
fn tables_and_headings_preserve_their_markdown_structure() {
    let source = "# Heading $x$\n\n| left | right |\n| --- | --- |\n| $y$ | **$z$** |";
    let doc = Document::parse(source);
    assert_eq!(doc.formulas.len(), 3);
    assert!(doc.formulas.iter().all(|f| f.detached));
    assert!(
        rune_kit::markdown::parse(&doc.markdown)
            .iter()
            .any(|b| matches!(b, rune_kit::markdown::Block::Table(_)))
    );
}

#[test]
fn task_offsets_survive_formula_expansion_and_reject_stale_edits() {
    let mut source = "A $x$\n\n- [ ] Do work\n".to_string();
    let doc = Document::parse(&source);
    let rendered = doc.markdown.find("[ ]").unwrap();
    let original = doc.original_offset(rendered).unwrap();
    assert_eq!(original, source.find("[ ]").unwrap());
    let change = TaskChange {
        source: source.as_str().into(),
        block_key: "b1".into(),
        marker_offset: original,
        checked: true,
    };
    assert!(apply_task_change(&mut source, &change));
    assert!(source.contains("[x]"));
    assert!(!apply_task_change(&mut source, &change));
}

#[test]
fn real_engine_draws_academic_latex_as_paths() {
    for tex in [
        r"\frac{a_1^2}{\sqrt{x}}",
        r"\sum_{i=1}^{n} i^2",
        r"\int_0^1 x^2\,dx",
        r"\begin{aligned}T(n)&=2T(n/2)+n\\&=n\log n\end{aligned}",
        r"\begin{pmatrix}a&b\\c&d\end{pmatrix}",
        r"\begin{cases}x^2&x>0\\0&x\le 0\end{cases}",
    ] {
        let svg = render_formula_svg(tex, true).unwrap_or_else(|e| panic!("{tex}: {e}"));
        assert!(svg.contains("<path"), "{tex}");
        assert!(!svg.contains("<foreignObject"));
        assert!(!svg.contains("<script"));
        assert!(!svg.contains("<text"));
    }
}

#[test]
fn unsupported_and_resource_limited_math_are_explicit_errors() {
    for tex in [
        r"\input{/secret}",
        r"\href{javascript:evil}{x}",
        r"\unknownAetherCommand{x}",
        "{",
    ] {
        assert!(render_formula_svg(tex, true).is_err(), "{tex}");
    }
    assert!(render_formula_svg(&"x".repeat(MAX_TEX + 1), false).is_err());
    assert!(
        render_formula_svg(&format!("{}x{}", "{".repeat(100), "}".repeat(100)), false).is_err()
    );
}

#[test]
fn caches_do_not_repeat_document_parse_or_math_layout() {
    let mut rich = RichContent::new(Arc::new(|| {}));
    let first = rich.document("A $x$ ");
    let again = rich.document("A $x$ ");
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(rich.document_parses, 1);
    let a = rich.request(&first.formulas[0], [240, 240, 240]);
    let b = rich.request(&again.formulas[0], [240, 240, 240]);
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(rich.math_requests, 1);
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let status = a.lock().unwrap().clone();
        match status {
            MathStatus::Ready { image, .. } => {
                let [w, h] = image.wait().expect("real SVG rasterization");
                assert!(w > 1 && h > 1);
                break;
            }
            MathStatus::Unsupported(e) => panic!("{e}"),
            MathStatus::Pending => {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    for i in 0..100 {
        rich.document(&format!("plain {i}"));
    }
    assert!(rich.documents.len() <= DOCUMENTS);
    assert!(rich.documents.iter().map(|d| d.bytes()).sum::<usize>() <= DOCUMENT_BYTES);
}

struct State {
    rich: RefCell<RichContent>,
    source: String,
    changed: bool,
}
fn view(s: &State, _: &mut Cx<State>) -> El {
    s.rich.borrow_mut().render_with_tasks(
        &s.source,
        "message",
        &Kit::neutral(true),
        |state: &mut State, change| {
            state.changed = apply_task_change(&mut state.source, &change);
        },
    )
}
fn runtime(source: &str) -> Runtime<State> {
    let mut rt = Runtime::new(
        State {
            rich: RefCell::new(RichContent::new(Arc::new(|| {}))),
            source: source.into(),
            changed: false,
        },
        view,
        (600.0, 1800.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    rt
}

#[test]
fn actual_rune_runtime_builds_gfm_code_diff_and_interactive_tasks() {
    let mut rt = runtime(
        "| a | b |\n|---|---|\n|1|2|\n\n```rust\nfn main() {}\n```\n\n```diff\n+new\n-old\n```\n\n- [ ] Native task\n",
    );
    let elements = rt.elements();
    let texts = elements
        .iter()
        .filter_map(|e| e.text.as_deref())
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|t| t.contains("fn main()")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("new")), "{texts:?}");
    let checkbox = elements
        .iter()
        .find(|e| e.role == "checkbox")
        .expect("native task checkbox");
    let [x, y, w, h] = checkbox.rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.frame(0.016);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.016);
    assert!(rt.state.changed);
    assert!(rt.state.source.contains("[x]"));
}

#[test]
fn unsafe_images_never_resolve_and_unsupported_math_is_visible() {
    let mut rt = runtime("![unsafe](file:///secret.png)\n\n$\\unknownAetherCommand{x}$");
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        rt.invalidate(); // A real shell invalidates the view when the setup waker fires.
        rt.frame(0.016);
        let texts = rt
            .elements()
            .into_iter()
            .filter_map(|e| e.text)
            .collect::<Vec<_>>();
        if texts.iter().any(|t| t.contains("unsupported native math")) {
            assert!(texts.iter().any(|t| t.contains("unknownAetherCommand")));
            break;
        }
        assert!(
            Instant::now() < until,
            "texts={texts:?}; statuses={:?}",
            rt.state
                .rich
                .borrow()
                .formulas
                .iter()
                .map(|e| e.status.lock().unwrap().clone())
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn whole_message_selection_copies_original_not_synthetic_image_urls() {
    let source = "Before **bold** $x^2$ after.\n\n- [ ] Task";
    let mut rt = runtime(source);
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    rt.update(|_, tx| tx.select_all_text("message"));
    rt.frame(0.016);
    rt.key(rune::KeyEvent {
        key: rune::Key::Command('c'),
        mods: rune::Mods {
            cmd: true,
            ..Default::default()
        },
    });
    rt.frame(0.016);
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some(source)
    );
}

#[test]
fn rune_runtime_mounts_real_formula_pixels_with_selectable_tex() {
    let mut rt = runtime("Result $\\frac{1}{2}$.");
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        rt.invalidate();
        rt.frame(0.016);
        let elements = rt.elements();
        let tex = elements
            .iter()
            .find(|e| e.text.as_deref().is_some_and(|t| t.starts_with("TeX: ")));
        if tex.is_some() {
            assert!(
                elements
                    .iter()
                    .any(|e| e.role == "image" && e.rect[2] > 1.0 && e.rect[3] > 1.0)
            );
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(rt.state.rich.borrow().math_requests, 1);
}

#[test]
fn formula_image_scheme_cannot_be_spoofed_by_untrusted_markdown() {
    let source = "![untrusted](aether-native-math:0)\n\n$x$";
    let doc = Document::parse(source);
    assert_ne!(doc.formulas[0].image_source, "aether-native-math:0");
    assert!(doc.markdown.contains("![untrusted](aether-native-math:0)"));
}

#[test]
fn actual_task_callback_maps_offset_after_math() {
    let mut rt = runtime("Before $x^2$ after.\n\n- [ ] Native task\n");
    let checkbox = rt
        .elements()
        .into_iter()
        .find(|e| e.role == "checkbox")
        .unwrap();
    let [x, y, w, h] = checkbox.rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.frame(0.016);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.016);
    assert!(rt.state.changed);
    assert_eq!(
        rt.state.source,
        "Before $x^2$ after.\n\n- [x] Native task\n"
    );
}

#[test]
fn whole_source_copy_after_formula_pixels_are_ready() {
    let source = "Native $\\frac{1}{2}$ and **formatting**.";
    let mut rt = runtime(source);
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        rt.invalidate();
        rt.frame(0.016);
        if rt
            .elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.starts_with("TeX: ")))
        {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    rt.update(|_, tx| tx.select_all_text("message"));
    rt.frame(0.016);
    rt.key(rune::KeyEvent {
        key: rune::Key::Command('c'),
        mods: rune::Mods {
            cmd: true,
            ..Default::default()
        },
    });
    rt.frame(0.016);
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some(source)
    );
}
