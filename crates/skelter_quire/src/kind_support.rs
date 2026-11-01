//! The mechanics of a `code-editor` skelter-core kind, shared by [`crate::kind::CodeEditorKind`]
//! and hosts that keep their own kind (cadatomic's `<code-editor>`, which owns its documents,
//! props and methods but draws and edits through a [`SkelterEditor`] as this crate's kind does).
//!
//! - [`record_scene`]: the editor's four display lists composed into the node's one list.
//! - [`route_event`]: a skelter-core [`Event`] to the editor's input methods.
//! - [`input_effects`], [`schedule_wake`]: what the kind asks of its [`EventCx`] after an event.
//! - [`a11y_action`]: AccessKit `SetValue` and `ReplaceSelectedText`.
//! - [`ImeHost`] and [`text_input_client!`](crate::text_input_client): the kind as the platform
//!   IME's `TextInputClient`, through an [`ImeTarget`].

use std::sync::Arc;

use quire_core::{Document, EditorView};
use skelter_base::display::DisplayList;
use skelter_base::geom::{Point, Rect, RoundedRect, Vec2};
use skelter_base::ids::RunId;
use skelter_base::text::GlyphRun;
use skelter_base::time::Time;
use skelter_core::{Event, EventCx, FocusReason, RecordCx};

use crate::editor::SkelterEditor;
use crate::ime::ImeTarget;
use crate::layers::translate_prim;
use crate::scene::RunSink;

/// [`RunSink`] over a node's [`RecordCx`].
pub struct CxSink<'c, 'a>(pub &'c mut RecordCx<'a>);

impl std::fmt::Debug for CxSink<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CxSink").finish_non_exhaustive()
    }
}

impl RunSink for CxSink<'_, '_> {
    fn add_run(&mut self, run: Arc<GlyphRun>) -> RunId {
        self.0.add_run(run)
    }
}

/// Records the editor into the node's display list: the background, the content under the text
/// clip translated by the scroll offset, the primary caret in its blink phase at `now`, the gutter
/// by its y offset under its own clip, and the overlay.
pub fn record_scene(
    cx: &mut RecordCx,
    editor: &mut SkelterEditor,
    doc: &Document,
    view: &mut EditorView,
    now: Time,
) {
    let sc = editor.record(doc, view, &mut CxSink(cx), None);
    let caret_on = editor.caret_visible(now);
    let put = |cx: &mut RecordCx, list: &DisplayList, d: Vec2| {
        for p in &list.items {
            let (q, ink) = translate_prim(p, d);
            cx.push(q, ink);
        }
    };
    put(cx, &sc.background, Vec2::ZERO);
    let content_d = Vec2::new(
        sc.text_origin.x - sc.scroll.x,
        sc.text_origin.y - sc.scroll.y,
    );
    cx.push_clip(RoundedRect {
        rect: sc.text_clip,
        radii: Default::default(),
    });
    put(cx, &sc.content, content_d);
    if caret_on && let Some((p, _)) = sc.caret_prim() {
        let (q, ink) = translate_prim(&p, content_d);
        cx.push(q, ink);
    }
    cx.pop_clip();
    if sc.gutter_width > 0.0 {
        cx.push_clip(RoundedRect {
            rect: Rect::new(0.0, 0.0, sc.gutter_width, sc.text_clip.max.y),
            radii: Default::default(),
        });
        put(
            cx,
            &sc.gutter,
            Vec2::new(0.0, sc.text_origin.y - sc.scroll.y),
        );
        cx.pop_clip();
    }
    put(cx, &sc.overlay, Vec2::ZERO);
}

/// Hands `ev` to the editor at `now` (after `set_now`); true when the editor used it. Raw motion,
/// gestures, clicks, hover entry and per-child AccessKit requests are not the editor's.
pub fn route_event(
    e: &mut SkelterEditor,
    doc: &mut Document,
    view: &mut EditorView,
    ev: &Event,
    now: Time,
) -> bool {
    e.set_now(now);
    match ev {
        Event::PointerDown(p) => {
            e.focus(doc, view, true);
            e.pointer_down(doc, view, p)
        }
        Event::PointerMove(p) => e.pointer_move(doc, view, p),
        Event::PointerUp(p) => e.pointer_up(doc, view, p),
        Event::PointerCancel => e.pointer_up(doc, view, &cancel_input()),
        Event::HoverLeave => {
            e.pointer_leave(doc, view);
            false
        }
        Event::Wheel(w) => e.wheel(doc, view, w.delta, w.modifiers),
        Event::Key(k) => e.key(doc, view, k),
        Event::Ime(m) => {
            e.ime(doc, view, m);
            true
        }
        Event::FocusIn(_) => {
            e.focus(doc, view, true);
            false
        }
        Event::FocusOut => {
            e.focus(doc, view, false);
            false
        }
        Event::Timer => {
            e.tick(doc, view, now);
            true
        }
        Event::A11y(req) => a11y_action(e, doc, view, req),
        Event::Click(_)
        | Event::HoverEnter
        | Event::RawMotion(_)
        | Event::Gesture(_)
        | Event::A11yChild(..) => false,
    }
}

/// What a code editor asks of its node after an event: focus and pointer capture on press,
/// release on lift or cancel, the text cursor while the pointer is over it.
pub fn input_effects(cx: &mut EventCx, ev: &Event) {
    match ev {
        Event::PointerDown(_) => {
            cx.focus(FocusReason::Pointer);
            cx.capture_pointer();
        }
        Event::PointerUp(_) | Event::PointerCancel => cx.release_pointer(),
        Event::PointerMove(_) | Event::HoverEnter => {
            cx.set_cursor(skelter_base::cursor::CursorIcon::Text)
        }
        _ => {}
    }
}

/// Wakes the node at the editor's next deadline (hover and hint timers, the caret's next blink;
/// [`SkelterEditor::tick`]'s answer), or cancels its wake-up when there is none.
pub fn schedule_wake(cx: &mut EventCx, next: Option<Time>) {
    match next {
        Some(t) => cx.wake_at(t),
        None => cx.cancel_wake(),
    }
}

/// The pointer lift a cancel stands for: no button, no position.
fn cancel_input() -> skelter_core::PointerInput {
    skelter_core::PointerInput {
        pointer: 0,
        kind: skelter_base::event::PointerType::Mouse,
        button: None,
        buttons: 0,
        pos: Point::ZERO,
        local: Point::ZERO,
        modifiers: Default::default(),
        pressure: 0.0,
        count: 0,
        history: Vec::new(),
    }
}

/// `SetValue` replaces the text; `ReplaceSelectedText` the selection (egui_quire's
/// `handle_accesskit_actions`). Refused when the editor is read-only.
pub fn a11y_action(
    e: &mut SkelterEditor,
    doc: &mut Document,
    view: &mut EditorView,
    req: &skelter_base::accesskit::ActionRequest,
) -> bool {
    use skelter_base::accesskit::{Action, ActionData};
    if e.options.read_only {
        return false;
    }
    let Some(ActionData::Value(v)) = &req.data else {
        return false;
    };
    let len = doc.text().len_bytes();
    let range = match req.action {
        Action::SetValue => 0..len,
        Action::ReplaceSelectedText => view.selection().primary().range(),
        _ => return false,
    };
    let author = e.options.author;
    if let Some(a) = crate::editor::replace_range(doc, view, range, v, false, author) {
        e.note_applied(a);
    }
    true
}

/// A kind that lends an [`ImeTarget`] (its editor, document, view and window transform) while it
/// has one. [`text_input_client!`](crate::text_input_client) implements `TextInputClient` over it.
pub trait ImeHost {
    /// Runs `f` on the target, or returns `None` before the editor exists.
    fn with_ime<R>(&self, f: impl FnOnce(&mut ImeTarget<'_>) -> R) -> Option<R>;
}

/// Implements skelter's `TextInputClient` for a type that implements
/// [`ImeHost`](crate::kind_support::ImeHost), forwarding each call to its [`ImeTarget`]
/// (`InputPurpose::Code`).
#[macro_export]
macro_rules! text_input_client {
    ($t:ty) => {
        impl $crate::skelter_base::input::TextInputClient for $t {
            fn text_for_range(
                &self,
                range: $crate::skelter_base::input::Utf16Range,
            ) -> ::std::option::Option<::std::string::String> {
                $crate::kind_support::ImeHost::with_ime(self, |t| t.text_for_range(range)).flatten()
            }
            fn selected_range(
                &self,
            ) -> ::std::option::Option<$crate::skelter_base::input::Utf16Range> {
                $crate::kind_support::ImeHost::with_ime(self, |t| t.selected_range()).flatten()
            }
            fn marked_range(
                &self,
            ) -> ::std::option::Option<$crate::skelter_base::input::Utf16Range> {
                $crate::kind_support::ImeHost::with_ime(self, |t| t.marked_range()).flatten()
            }
            fn replace_text_in_range(
                &mut self,
                range: ::std::option::Option<$crate::skelter_base::input::Utf16Range>,
                text: &str,
            ) {
                $crate::kind_support::ImeHost::with_ime(self, |t| {
                    t.replace_text_in_range(range, text)
                });
            }
            fn replace_and_mark_text_in_range(
                &mut self,
                range: ::std::option::Option<$crate::skelter_base::input::Utf16Range>,
                text: &str,
                new_selected: ::std::option::Option<$crate::skelter_base::input::Utf16Range>,
            ) {
                $crate::kind_support::ImeHost::with_ime(self, |t| {
                    t.replace_and_mark_text_in_range(range, text, new_selected)
                });
            }
            fn unmark_text(&mut self) {
                $crate::kind_support::ImeHost::with_ime(self, |t| t.unmark_text());
            }
            fn bounds_for_range(
                &self,
                range: $crate::skelter_base::input::Utf16Range,
            ) -> ::std::option::Option<$crate::skelter_base::geom::Rect> {
                $crate::kind_support::ImeHost::with_ime(self, |t| t.bounds_for_range(range))
                    .flatten()
            }
            fn character_index_for_point(
                &self,
                p: $crate::skelter_base::geom::Point,
            ) -> ::std::option::Option<usize> {
                $crate::kind_support::ImeHost::with_ime(self, |t| t.character_index_for_point(p))
                    .flatten()
            }
            fn surrounding_text(
                &self,
                max_bytes: usize,
            ) -> ::std::option::Option<(::std::string::String, ::std::ops::Range<usize>)> {
                $crate::kind_support::ImeHost::with_ime(self, |t| t.surrounding_text(max_bytes))
                    .flatten()
            }
            fn purpose(&self) -> $crate::skelter_base::input::InputPurpose {
                $crate::skelter_base::input::InputPurpose::Code
            }
        }
    };
}
