//! Playground: two views of one Svelte document side by side, highlighted by quire-syntax's
//! `SyntaxDriver` on background threads.
//!
//! `scripts/cargo.sh run -p egui_quire --example playground`
//!
//! The highlighting is host code: the widget reads tokens and folds from the `Document`; the host
//! runs the driver around `show` (see [`Highlighting`]).

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use egui_quire::{CodeEditor, EditorEvent, EditorOutput};
use quire_core::syntax::Spawner;
use quire_core::{Document, EditorView};
use quire_syntax::{Lang, SyntaxDriver, TreeSitterHighlighter};

const SAMPLE: &str = include_str!("../../quire-syntax/tests/fixtures/component.svelte");

/// Runs each job on its own thread (the host's choice; quire's crates never spawn threads).
struct ThreadSpawner;

impl Spawner for ThreadSpawner {
    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(job);
    }
}

/// The host side of syntax highlighting for one document shown in several views.
///
/// - Before the views show: `SyntaxDriver::update` with the lines every view showed last frame
///   (one range per view); results
///   that arrived since are applied (tokens via `Document::set_tokens`, folds via
///   `Document::set_fold_ranges`).
/// - The driver's waker is `ctx.request_repaint`, so a finished background job brings a frame;
///   a pending debounce asks for a frame at `next_deadline`.
/// - While a debounce or job is pending the driver holds `PendingWork` in the document, so the
///   editor's `EditorOutput::settled` (and `Document::settled_probe`) report busy.
struct Highlighting {
    driver: SyntaxDriver<TreeSitterHighlighter>,
    /// Per view: the lines it showed last frame.
    visible: [Range<usize>; 2],
}

impl Highlighting {
    fn new(ctx: &egui::Context, lang: Lang) -> Self {
        let mut driver = SyntaxDriver::new(
            TreeSitterHighlighter::new(lang).expect("grammar enabled"),
            Arc::new(ThreadSpawner),
        );
        let c = ctx.clone();
        driver.set_waker(Arc::new(move || c.request_repaint()));
        Self {
            driver,
            visible: [0..100, 0..100],
        }
    }

    fn before_show(&mut self, ctx: &egui::Context, doc: &mut Document) {
        let now = (ctx.input(|i| i.time) * 1000.0) as u64;
        self.driver.update(doc, &self.visible, now);
        if let Some(at) = self.driver.next_deadline() {
            ctx.request_repaint_after(Duration::from_millis(at.saturating_sub(now)));
        }
    }

    fn after_show(&mut self, view: usize, out: &EditorOutput) {
        self.visible[view] = out.visible_lines.clone();
    }
}

struct App {
    doc: Document,
    views: [EditorView; 2],
    syntax: Highlighting,
    settled: bool,
    log: Vec<String>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::bottom("log").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "version {:?}  dirty {}  settled {}",
                    self.doc.version(),
                    self.doc.is_dirty(),
                    self.settled,
                ));
            });
            for l in self.log.iter().rev().take(4) {
                ui.label(l);
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.columns(2, |cols| {
                for (i, (col, view)) in cols.iter_mut().zip(&mut self.views).enumerate() {
                    if i == 0 {
                        self.syntax.before_show(col.ctx(), &mut self.doc);
                    }
                    let out =
                        CodeEditor::new(&mut self.doc, view, ("playground", i), "Source").show(col);
                    self.syntax.after_show(i, &out);
                    if i == 0 {
                        self.settled = out.settled;
                    }
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
    let views = [EditorView::new(&doc), EditorView::new(&doc)];
    eframe::run_native(
        "quire playground",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            Ok(Box::new(App {
                syntax: Highlighting::new(&cc.egui_ctx, Lang::Svelte),
                doc,
                views,
                settled: true,
                log: Vec::new(),
            }))
        }),
    )
}
