//! Host integration of quire-syntax's `SyntaxDriver` with the editor: update before `show` with
//! the last visible lines, waker and `next_deadline` repaints, settled while jobs are pending,
//! tree-sitter folds in the gutter. (The same loop as `examples/playground.rs`.)

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::Harness;
use egui_quire::CodeEditor;
use quire_core::syntax::{InlineSpawner, TokenType};
use quire_core::{Document, EditorView};
use quire_syntax::{Lang, SyntaxDriver, TreeSitterHighlighter};

struct St {
    doc: Document,
    view: EditorView,
    driver: SyntaxDriver<TreeSitterHighlighter>,
    visible: Range<usize>,
    settled: bool,
}

fn app(ui: &mut egui::Ui, st: &mut St) {
    let ctx = ui.ctx().clone();
    let now = (ctx.input(|i| i.time) * 1000.0) as u64;
    st.driver
        .update(&mut st.doc, std::slice::from_ref(&st.visible), now);
    if let Some(at) = st.driver.next_deadline() {
        ctx.request_repaint_after(std::time::Duration::from_millis(at.saturating_sub(now)));
    }
    let out = CodeEditor::new(&mut st.doc, &mut st.view, "syn", "Source").show(ui);
    st.visible = out.visible_lines.clone();
    st.settled = out.settled;
}

const TS: &str = "function f() {\nconst a = 1;\nreturn a;\n}\nlet z = 0;\n";

fn harness(wakes: Arc<AtomicUsize>) -> Harness<'static, St> {
    let doc = Document::new(TS, Default::default());
    let view = EditorView::new(&doc);
    let mut driver = SyntaxDriver::new(
        TreeSitterHighlighter::new(Lang::TypeScript).unwrap(),
        Arc::new(InlineSpawner),
    )
    .with_debounce(50);
    driver.set_waker(Arc::new(move || {
        wakes.fetch_add(1, Ordering::Relaxed);
    }));
    Harness::builder()
        .with_size(Vec2::new(600.0, 300.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            app,
            St {
                doc,
                view,
                driver,
                visible: 0..50,
                settled: false,
            },
        )
}

fn first_type(doc: &Document, line: usize) -> TokenType {
    doc.tokens()
        .line(line)
        .first()
        .map_or(TokenType::Text, |t| t.style.ty())
}

#[test]
fn driver_tokens_folds_and_settled_through_the_editor() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let mut h = harness(wakes.clone());
    h.run();
    assert_eq!(first_type(&h.state().doc, 0), TokenType::Keyword);
    assert!(wakes.load(Ordering::Relaxed) >= 1, "the waker fired");
    assert!(h.state().settled);
    // Tree-sitter folds (no indentation here, so they can only come from the tree).
    assert!(
        h.state()
            .doc
            .fold_ranges()
            .iter()
            .any(|r| r.start_line == 0 && r.end_line >= 2)
    );

    // Type: the editor reports busy while the driver debounces, and asks for a frame then.
    let r = h.state().view.selection().primary();
    assert_eq!(r.head, 0);
    let p = egui::pos2(300.0, 290.0);
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Home);
    h.event(Event::Text("const q = 2;\n".into()));
    h.step();
    h.step();
    assert!(!h.state().settled, "a parse is pending");
    let delay = h
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|v| v.repaint_delay)
        .unwrap();
    assert!(delay <= std::time::Duration::from_millis(50), "{delay:?}");
    h.run_steps(6); // 100 ms
    assert!(h.state().settled);
    assert_eq!(first_type(&h.state().doc, 0), TokenType::Keyword);
    assert_eq!(first_type(&h.state().doc, 1), TokenType::Keyword);

    // The tree's fold folds the function body (line 1 now).
    let id = CodeEditor::id_for("syn");
    let k = egui_quire::Keymap::vscode_default()
        .keys_for(h.ctx.os(), "editor.foldAll")
        .unwrap();
    let mac = matches!(h.ctx.os(), egui::os::OperatingSystem::Mac);
    for c in std::iter::once(k.first).chain(k.second) {
        h.key_press_modifiers(c.modifiers(mac), c.key);
        h.run();
    }
    let line2 = CodeEditor::anchor_rect(&h.ctx, id, "line[2]").unwrap();
    assert_eq!(line2.height(), 0.0, "folded by the syntax range");
}
