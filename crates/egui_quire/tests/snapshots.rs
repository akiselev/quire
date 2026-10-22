//! Snapshot images of the editor, one per theme (light and dark), rendered through wgpu.
//!
//! macOS (Metal) only: other backends rasterise differently and the images would not compare.
//! Update with `UPDATE_SNAPSHOTS=1 scripts/cargo.sh test -p egui_quire --test snapshots`.
#![cfg(target_os = "macos")]

use egui::{Event, Key, Modifiers, PointerButton, Vec2, pos2};
use egui_kittest::Harness;
use egui_quire::CodeEditor;
use quire_core::{Diagnostic, Document, EditorView, Severity};

struct St {
    doc: Document,
    view: EditorView,
}

const SAMPLE: &str =
    "fn main() {\n\tlet greeting = \"hello\";\n\tlet n = 42;\n\tprintln!(\"{greeting} {n}\");\n}\n";

fn snapshot(dark: bool, name: &str) {
    let mut doc = Document::new(SAMPLE, Default::default());
    let v = doc.version();
    let at = SAMPLE.find("n = 42").unwrap();
    doc.set_diagnostics(
        "test".into(),
        v,
        vec![
            Diagnostic::new(at..at + 1, Severity::Warning, "unused n"),
            Diagnostic::new(3..7, Severity::Error, "demo error"),
        ],
    )
    .unwrap();
    let view = EditorView::new(&doc);
    let mut h = Harness::builder()
        .with_size(Vec2::new(420.0, 140.0))
        .build_ui_state(
            move |ui, st: &mut St| {
                ui.visuals_mut().text_cursor.blink = false;
                CodeEditor::new(&mut st.doc, &mut st.view, "snap", "Source").show(ui);
            },
            St { doc, view },
        );
    h.ctx.set_visuals(if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    });
    h.run();
    // Focus, then select "greeting" on line 2 (caret and selection visible).
    let p = pos2(200.0, 120.0);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Home);
    h.key_press(Key::ArrowDown);
    h.key_press(Key::End);
    h.key_press(Key::Home);
    h.key_press_modifiers(Modifiers::SHIFT, Key::End);
    h.run();
    h.snapshot(name);
}

#[test]
fn snapshot_dark() {
    snapshot(true, "editor_dark");
}

#[test]
fn snapshot_light() {
    snapshot(false, "editor_light");
}

/// Soft wrap (indented continuation rows, a selection across rows) and a long line cut to its
/// prefix with the marker.
#[test]
fn snapshot_wrap_and_prefix() {
    let text = format!(
        "fn main() {{\n    let s = \"{}\";\n    {}\n}}\n",
        "wrapped words ".repeat(8).trim_end(),
        "x".repeat(80)
    );
    let cfg = quire_core::EditorConfig {
        word_wrap: quire_core::WordWrap::On,
        stop_rendering_line_after: 60,
        ..Default::default()
    };
    let doc = Document::new(&text, cfg);
    let view = EditorView::new(&doc);
    let mut h = Harness::builder()
        .with_size(Vec2::new(420.0, 160.0))
        .build_ui_state(
            move |ui, st: &mut St| {
                ui.visuals_mut().text_cursor.blink = false;
                CodeEditor::new(&mut st.doc, &mut st.view, "snap", "Source").show(ui);
            },
            St { doc, view },
        );
    h.ctx.set_visuals(egui::Visuals::light());
    h.run();
    let at = text.find("words").unwrap();
    let end = text.find("\";").unwrap() - 10;
    {
        let st = h.state_mut();
        st.doc
            .execute(
                &mut st.view,
                &quire_core::EditorCommand::SetSelection {
                    selection: quire_core::Selection::single(at, end),
                },
                None,
            )
            .unwrap();
    }
    h.run();
    h.snapshot("editor_wrap_prefix");
}
