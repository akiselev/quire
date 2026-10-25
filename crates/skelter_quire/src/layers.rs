//! The editor as compositor layers, for hosts that own their part of the layer tree (a canvas-like
//! pane, a test harness, svelters' `<code-editor>` once skelter-core lets a kind own layers).
//!
//! Four layers under the host's parent layer, in paint order:
//!
//! | Layer | Kind | Transform | Holds |
//! |---|---|---|---|
//! | background | `Content` | the editor's box | [`EditorScene::background`] |
//! | content | `Scroll` | the text origin, translated by the content scroll node (x and y) | [`EditorScene::content`] |
//! | gutter | `Scroll` | the editor's box, translated by the gutter scroll node (y only) | [`EditorScene::gutter`] |
//! | overlay | `Overlay` | the editor's box | [`EditorScene::overlay`] |
//!
//! Scrolling is a compositor offset: [`EditorLayers::scroll_commits`] are the two
//! [`FastCommit::ScrollOffset`]s of a scroll the recorded lines cover ([`SkelterEditor::covers`]);
//! only the overlay (scrollbars, popups) is re-recorded then. The caret is a [`FastCommit::Caret`]
//! the compositor blinks ([`EditorLayers::caret_commit`]).
//!
//! The host allocates every id (layers, property nodes, the four list nodes), since they share id
//! spaces with the rest of its tree.

use std::sync::Arc;

use quire_core::{Document, EditorView};
use skelter_base::commit::{Commit, FastCommit};
use skelter_base::display::DisplayList;
use skelter_base::geom::{Affine, Rect, Vec2};
use skelter_base::ids::{InputSeq, LayerId, NodeId, PropNodeId, Revision, RunId, WindowId};
use skelter_base::layer::{ItemRef, LayerDelta, LayerDesc, LayerKind};
use skelter_base::props::{
    ClipNode, ClipShape, OverscrollBehavior, PropOp, ScrollNode, TransformNode,
};
use skelter_base::text::GlyphRun;

use crate::editor::SkelterEditor;
use crate::scene::{EditorScene, RunSink};

/// Ids the host allocates for one editor's layers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorLayerIds {
    pub window: WindowId,
    /// The layer the editor's layers go under, and the transform space of the editor's box.
    pub parent_layer: Option<LayerId>,
    pub parent_space: Option<PropNodeId>,
    pub background: LayerId,
    pub content: LayerId,
    pub gutter: LayerId,
    pub overlay: LayerId,
    /// The editor's box (its offset in the parent space).
    pub box_space: PropNodeId,
    /// The text area clip (in the box space).
    pub text_clip: PropNodeId,
    /// The gutter clip (in the box space).
    pub gutter_clip: PropNodeId,
    pub content_scroll: PropNodeId,
    pub content_space: PropNodeId,
    pub gutter_scroll: PropNodeId,
    pub gutter_space: PropNodeId,
    /// The nodes the four display lists are committed under.
    pub background_node: NodeId,
    pub content_node: NodeId,
    pub gutter_node: NodeId,
    pub overlay_node: NodeId,
}

/// Run ids for one family of lists: the runs of the current record and the previous one's, which
/// the next commit drops.
#[derive(Debug, Default)]
pub struct RunTable {
    next: u64,
    /// The high bits that keep this table's ids apart from the host's.
    base: u64,
    added: Vec<(RunId, Arc<GlyphRun>)>,
    live: Vec<RunId>,
}

impl RunTable {
    /// Ids from `base` (e.g. `0xQ000_0000_0000_0000` per editor) upwards.
    pub fn new(base: u64) -> Self {
        RunTable {
            next: 0,
            base,
            added: Vec::new(),
            live: Vec::new(),
        }
    }

    /// Starts a new record: returns the previous record's runs (to drop once the new lists are
    /// committed).
    pub fn begin(&mut self) -> Vec<RunId> {
        self.added.clear();
        std::mem::take(&mut self.live)
    }

    /// The runs added since [`Self::begin`].
    pub fn take_added(&mut self) -> Vec<(RunId, Arc<GlyphRun>)> {
        std::mem::take(&mut self.added)
    }
}

impl RunSink for RunTable {
    fn add_run(&mut self, run: Arc<GlyphRun>) -> RunId {
        self.next += 1;
        let id = RunId(self.base | self.next);
        self.added.push((id, run));
        self.live.push(id);
        id
    }
}

/// One editor's layers and what the compositor holds of them.
#[derive(Debug)]
pub struct EditorLayers {
    pub ids: EditorLayerIds,
    main_runs: RunTable,
    overlay_runs: RunTable,
    added: bool,
    /// What the compositor has: (box transform, size bits, content rev) of the last full commit.
    committed: Option<(Affine, (u32, u32), u64)>,
    last_scroll: Vec2,
}

impl EditorLayers {
    /// `run_base`: high bits for this editor's run ids (two tables: `run_base` and
    /// `run_base | 1 << 62`).
    pub fn new(ids: EditorLayerIds, run_base: u64) -> Self {
        EditorLayers {
            ids,
            main_runs: RunTable::new(run_base),
            overlay_runs: RunTable::new(run_base | 1 << 62),
            added: false,
            committed: None,
            last_scroll: Vec2::ZERO,
        }
    }

    /// The commit for this frame (after [`SkelterEditor::layout`]): everything when the content
    /// changed or a scroll left the recorded lines; else the overlay and the scroll offsets.
    pub fn commit(
        &mut self,
        editor: &mut SkelterEditor,
        doc: &Document,
        view: &mut EditorView,
        box_transform: Affine,
        revision: Revision,
        input_seq: InputSeq,
    ) -> Commit {
        let ids = self.ids;
        let mut c = Commit {
            window: ids.window,
            revision,
            input_seq,
            ..Commit::default()
        };
        let size = editor.geom().size;
        let key = (
            box_transform,
            (size.w.to_bits(), size.h.to_bits()),
            editor.content_rev,
        );
        let full =
            self.committed != Some(key) || !self.added || self.last_scroll_left_cover(editor);
        if full {
            let dropped_main = self.main_runs.begin();
            let dropped_overlay = self.overlay_runs.begin();
            let sc = editor.record(doc, view, &mut self.main_runs, Some(&mut self.overlay_runs));
            self.structure(&mut c, &sc, box_transform);
            for (node, list) in [
                (ids.background_node, &sc.background),
                (ids.content_node, &sc.content),
                (ids.gutter_node, &sc.gutter),
                (ids.overlay_node, &sc.overlay),
            ] {
                c.lists.push((node, Arc::new(list.clone())));
            }
            c.runs.extend(self.main_runs.take_added());
            c.runs.extend(self.overlay_runs.take_added());
            c.dropped_runs.extend(dropped_main);
            c.dropped_runs.extend(dropped_overlay);
            c.ime_area = sc.ime_rect.map(|r| box_transform.apply_rect(&r));
            self.committed = Some(key);
            self.added = true;
            editor.recorded_at = Some(editor.scroll());
        } else {
            let dropped = self.overlay_runs.begin();
            let sc = editor.record_overlay_only(doc, view, &mut self.overlay_runs);
            c.lists
                .push((ids.overlay_node, Arc::new(sc.overlay.clone())));
            c.runs.extend(self.overlay_runs.take_added());
            c.dropped_runs.extend(dropped);
            Self::scroll_ops(&mut c, &ids, &sc);
            c.ime_area = sc.ime_rect.map(|r| box_transform.apply_rect(&r));
        }
        self.last_scroll = editor.scroll();
        c
    }

    fn last_scroll_left_cover(&self, editor: &SkelterEditor) -> bool {
        !editor.covers(editor.scroll())
    }

    fn scroll_ops(c: &mut Commit, ids: &EditorLayerIds, sc: &EditorScene) {
        let text = sc.text_clip;
        let content = ScrollNode {
            space: ids.box_space,
            offset: sc.scroll,
            content: sc.content_size,
            viewport: text,
            scroll_x: true,
            scroll_y: true,
            overscroll: OverscrollBehavior::None,
            passive: true,
        };
        let gutter = ScrollNode {
            offset: Vec2::new(0.0, sc.scroll.y),
            viewport: Rect::new(0.0, 0.0, sc.gutter_width, text.height()),
            scroll_x: false,
            ..content
        };
        c.props
            .ops
            .push(PropOp::Scroll(ids.content_scroll, content));
        c.props.ops.push(PropOp::Scroll(ids.gutter_scroll, gutter));
    }

    fn structure(&self, c: &mut Commit, sc: &EditorScene, box_transform: Affine) {
        let ids = self.ids;
        let ops = &mut c.props.ops;
        ops.push(PropOp::Transform(
            ids.box_space,
            TransformNode {
                parent: ids.parent_space,
                local: box_transform,
                scroll: None,
            },
        ));
        ops.push(PropOp::Clip(
            ids.text_clip,
            ClipNode {
                parent: None,
                space: ids.box_space,
                shape: ClipShape::Rect(sc.text_clip),
            },
        ));
        let gutter_rect = Rect::new(0.0, 0.0, sc.gutter_width, sc.text_clip.height());
        ops.push(PropOp::Clip(
            ids.gutter_clip,
            ClipNode {
                parent: None,
                space: ids.box_space,
                shape: ClipShape::Rect(gutter_rect),
            },
        ));
        let o = sc.text_origin;
        ops.push(PropOp::Transform(
            ids.content_space,
            TransformNode {
                parent: Some(ids.box_space),
                local: Affine::translate(o.x, o.y),
                scroll: Some(ids.content_scroll),
            },
        ));
        ops.push(PropOp::Transform(
            ids.gutter_space,
            TransformNode {
                parent: Some(ids.box_space),
                local: Affine::translate(0.0, o.y),
                scroll: Some(ids.gutter_scroll),
            },
        ));
        Self::scroll_ops(c, &ids, sc);
        let size = sc.text_clip.max;
        let full = Rect::new(0.0, 0.0, size.x, size.y);
        let desc = |id, kind, transform, clip, z, bounds| LayerDesc {
            id,
            parent: ids.parent_layer,
            kind,
            transform,
            clip,
            effect: None,
            z,
            bounds,
            opaque: false,
        };
        let layers = [
            desc(
                ids.background,
                LayerKind::Content,
                ids.box_space,
                None,
                0,
                full,
            ),
            desc(
                ids.content,
                LayerKind::Scroll { tiles: false },
                ids.content_space,
                Some(ids.text_clip),
                1,
                Rect::from_origin_size(skelter_base::geom::Point::ZERO, sc.content_size),
            ),
            desc(
                ids.gutter,
                LayerKind::Scroll { tiles: false },
                ids.gutter_space,
                Some(ids.gutter_clip),
                2,
                Rect::new(0.0, 0.0, sc.gutter_width, sc.content_size.h),
            ),
            desc(
                ids.overlay,
                LayerKind::Overlay,
                ids.box_space,
                None,
                3,
                full,
            ),
        ];
        let first = !self.added;
        for (d, node) in layers.into_iter().zip([
            ids.background_node,
            ids.content_node,
            ids.gutter_node,
            ids.overlay_node,
        ]) {
            c.layers.push(if first {
                LayerDelta::Add(d)
            } else {
                LayerDelta::Update(d)
            });
            if first {
                c.layers.push(LayerDelta::Items {
                    layer: d.id,
                    items: vec![ItemRef {
                        node,
                        transform: d.transform,
                        clip: d.clip,
                        effect: None,
                    }],
                });
            }
        }
    }

    /// The fast commits of a scroll to the editor's current offset (content x/y, gutter y).
    pub fn scroll_commits(
        &self,
        editor: &SkelterEditor,
        revision: Revision,
        input_seq: InputSeq,
    ) -> [FastCommit; 2] {
        let s = editor.scroll();
        let w = self.ids.window;
        [
            FastCommit::ScrollOffset {
                window: w,
                node: self.ids.content_scroll,
                offset: s,
                revision,
                input_seq,
            },
            FastCommit::ScrollOffset {
                window: w,
                node: self.ids.gutter_scroll,
                offset: Vec2::new(0.0, s.y),
                revision,
                input_seq,
            },
        ]
    }

    /// The caret for the compositor to blink, in window space (`None` hides it).
    pub fn caret_commit(
        &self,
        scene_caret: Option<Rect>,
        editor: &SkelterEditor,
        box_transform: Affine,
        revision: Revision,
    ) -> FastCommit {
        let g = editor.geom();
        let s = editor.scroll();
        let rect = scene_caret.map(|r| {
            let local = r.translate(Vec2::new(g.text_left() - s.x, crate::editor::PAD_Y - s.y));
            box_transform.apply_rect(&local)
        });
        FastCommit::Caret {
            window: self.ids.window,
            rect,
            revision,
        }
    }

    /// Removes the editor's layers and runs (the editor went away).
    pub fn remove(&mut self, revision: Revision) -> Commit {
        let ids = self.ids;
        let mut c = Commit {
            window: ids.window,
            revision,
            ..Commit::default()
        };
        for l in [ids.background, ids.content, ids.gutter, ids.overlay] {
            c.layers.push(LayerDelta::Remove(l));
        }
        for p in [
            ids.box_space,
            ids.text_clip,
            ids.gutter_clip,
            ids.content_scroll,
            ids.content_space,
            ids.gutter_scroll,
            ids.gutter_space,
        ] {
            c.props.ops.push(PropOp::Remove(p));
        }
        c.removed.extend([
            ids.background_node,
            ids.content_node,
            ids.gutter_node,
            ids.overlay_node,
        ]);
        c.dropped_runs.extend(self.main_runs.begin());
        c.dropped_runs.extend(self.overlay_runs.begin());
        self.added = false;
        self.committed = None;
        c
    }
}

/// A display list's prims moved by `d` (hosts that place a content list in their own space).
pub fn translate_list(list: &DisplayList, d: Vec2) -> DisplayList {
    let mut out = DisplayList::new();
    for p in &list.items {
        let (q, ink) = translate_prim(p, d);
        out.push(q, ink);
    }
    out.bounds = list.bounds.translate(d);
    out
}

/// One prim moved by `d`, with its moved ink rect (clips have no ink).
pub fn translate_prim(
    p: &skelter_base::display::Prim,
    d: Vec2,
) -> (skelter_base::display::Prim, Rect) {
    use skelter_base::display::Prim;
    let mv = |pt: skelter_base::geom::Point| pt + d;
    match p {
        Prim::Quad(q) => {
            let mut q = q.clone();
            q.rect = q.rect.translate(d);
            let r = q.rect;
            (Prim::Quad(q), r)
        }
        Prim::Shadow(s) => {
            let mut s = *s;
            s.rect = s.rect.translate(d);
            (
                Prim::Shadow(s),
                s.rect.inflate(s.blur * 1.5 + s.spread.max(0.0)),
            )
        }
        Prim::Line(l) => {
            let mut l = *l;
            l.from = mv(l.from);
            l.to = mv(l.to);
            let ink = Rect::new(
                l.from.x.min(l.to.x),
                l.from.y.min(l.to.y),
                l.from.x.max(l.to.x),
                l.from.y.max(l.to.y),
            )
            .inflate(l.width * 2.0);
            (Prim::Line(l), ink)
        }
        Prim::Glyphs(g) => {
            let mut g = *g;
            g.origin = mv(g.origin);
            // the run's metrics are not at hand: a generous box around the origin
            let ink = Rect::new(
                g.origin.x,
                g.origin.y - 64.0,
                g.origin.x + 4096.0,
                g.origin.y + 32.0,
            );
            (Prim::Glyphs(g), ink)
        }
        Prim::Icon(i) => {
            let mut i = *i;
            i.rect = i.rect.translate(d);
            (Prim::Icon(i), i.rect)
        }
        Prim::Image(i) => {
            let mut i = *i;
            i.rect = i.rect.translate(d);
            (Prim::Image(i), i.rect)
        }
        Prim::Path(p) => {
            let mut p = *p;
            p.transform = Affine::translate(d.x, d.y).then(&p.transform);
            (Prim::Path(p), Rect::ZERO)
        }
        Prim::Surface(s) => {
            let mut s = *s;
            s.rect = s.rect.translate(d);
            (Prim::Surface(s), s.rect)
        }
        Prim::Canvas(c) => {
            let mut c = *c;
            c.rect = c.rect.translate(d);
            (Prim::Canvas(c), c.rect)
        }
        Prim::PushClip(r) => {
            let mut r = *r;
            r.rect = r.rect.translate(d);
            (Prim::PushClip(r), Rect::ZERO)
        }
        Prim::PopClip => (Prim::PopClip, Rect::ZERO),
    }
}
