//! Playground: two views of one document side by side.
//!
//! `scripts/cargo.sh run -p egui_quire --example playground`

use egui_quire::{CodeEditor, EditorEvent};
use quire_core::{Document, EditorView};

const SAMPLE: &str = include_str!("../src/editor.rs");

struct App {
    doc: Document,
    left: EditorView,
    right: EditorView,
    log: Vec<String>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::bottom("log").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "version {:?}  dirty {}",
                    self.doc.version(),
                    self.doc.is_dirty()
                ));
            });
            for l in self.log.iter().rev().take(4) {
                ui.label(l);
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.columns(2, |cols| {
                for (i, (col, view)) in cols
                    .iter_mut()
                    .zip([&mut self.left, &mut self.right])
                    .enumerate()
                {
                    let out =
                        CodeEditor::new(&mut self.doc, view, ("playground", i), "Source").show(col);
                    for e in out.events {
                        if e == EditorEvent::SaveRequested {
                            let v = self.doc.version();
                            self.doc.mark_saved(v);
                        }
                        self.log.push(format!("view {i}: {e:?}"));
                    }
                }
            });
        });
    }
}

fn main() -> eframe::Result {
    let doc = Document::new(SAMPLE, Default::default());
    let left = EditorView::new(&doc);
    let right = EditorView::new(&doc);
    eframe::run_native(
        "quire playground",
        eframe::NativeOptions::default(),
        Box::new(|_| {
            Ok(Box::new(App {
                doc,
                left,
                right,
                log: Vec::new(),
            }))
        }),
    )
}
