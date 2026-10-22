//! Core laws (ARCH-QUIRE §3.10), proptested.

use proptest::prelude::*;
use quire_core::*;

const ALPHABET: &[&str] = &["a", "b", " ", "\n", "é", "😀", "xy"];

fn text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(ALPHABET), 0..20).prop_map(|v| v.concat())
}

/// Char-boundary byte offsets of a string.
fn boundaries(s: &str) -> Vec<usize> {
    let mut v: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    v.push(s.len());
    v
}

/// Random edits against `s`: (sorted, non-overlapping ranges on boundaries, inserted strings).
fn edits_for(s: &str, seeds: &[(usize, usize, usize)]) -> Vec<(std::ops::Range<usize>, String)> {
    let b = boundaries(s);
    let mut points: Vec<(usize, usize, String)> = seeds
        .iter()
        .map(|&(x, len, ins)| {
            let i = x % b.len();
            let j = (i + len % 3).min(b.len() - 1);
            (b[i], b[j], ALPHABET[ins % ALPHABET.len()].repeat(ins % 3))
        })
        .collect();
    points.sort_by_key(|p| (p.0, p.1));
    let mut out = Vec::new();
    let mut last_end = 0;
    for (a, e, ins) in points {
        if a < last_end {
            continue;
        }
        out.push((a..e, ins));
        last_end = e.max(a + 1);
    }
    out
}

fn cs(s: &str, seeds: &[(usize, usize, usize)]) -> ChangeSet {
    ChangeSet::replace(s.len(), edits_for(s, seeds)).unwrap()
}

fn seeds() -> impl Strategy<Value = Vec<(usize, usize, usize)>> {
    prop::collection::vec((0usize..64, 0usize..4, 0usize..16), 0..5)
}

proptest! {
    #[test]
    fn compose_applies_in_order(s in text_strategy(), sa in seeds(), sb in seeds()) {
        let t = Text::from_file(&s);
        let a = cs(&s, &sa);
        let t1 = a.apply(&t).unwrap();
        let b = cs(&t1.to_string(), &sb);
        let t2 = b.apply(&t1).unwrap();
        prop_assert_eq!(a.compose(&b).unwrap().apply(&t).unwrap(), t2);
    }

    #[test]
    fn compose_is_associative(s in text_strategy(), sa in seeds(), sb in seeds(), sc in seeds()) {
        let t = Text::from_file(&s);
        let a = cs(&s, &sa);
        let t1 = a.apply(&t).unwrap();
        let b = cs(&t1.to_string(), &sb);
        let t2 = b.apply(&t1).unwrap();
        let c = cs(&t2.to_string(), &sc);
        let left = a.compose(&b).unwrap().compose(&c).unwrap();
        let right = a.compose(&b.compose(&c).unwrap()).unwrap();
        prop_assert_eq!(left.apply(&t).unwrap(), right.apply(&t).unwrap());
        prop_assert_eq!(left, right);
    }

    #[test]
    fn invert_restores(s in text_strategy(), sa in seeds()) {
        let t = Text::from_file(&s);
        let a = cs(&s, &sa);
        let after = a.apply(&t).unwrap();
        prop_assert_eq!(a.invert(&t).apply(&after).unwrap().to_string(), s);
    }

    #[test]
    fn map_pos_is_monotone(s in text_strategy(), sa in seeds()) {
        let a = cs(&s, &sa);
        for bias in [Bias::Before, Bias::After] {
            let mut prev = 0;
            for p in 0..=s.len() {
                let q = a.map_pos(p, bias);
                prop_assert!(q >= prev, "map_pos not monotone at {p}: {q} < {prev}");
                prop_assert!(q <= a.len_after());
                prev = q;
            }
        }
    }

    #[test]
    fn map_transform_converges(s in text_strategy(), sa in seeds(), sb in seeds()) {
        let t = Text::from_file(&s);
        let a = cs(&s, &sa);
        let b = cs(&s, &sb);
        let a2 = a.map(&b, true).unwrap();
        let b2 = b.map(&a, false).unwrap();
        prop_assert_eq!(
            a2.apply(&b.apply(&t).unwrap()).unwrap(),
            b2.apply(&a.apply(&t).unwrap()).unwrap()
        );
    }

    #[test]
    fn anchors_survive_random_edits(s in text_strategy(), steps in prop::collection::vec(seeds(), 1..10), at in 0usize..64) {
        let mut doc = Document::new(&s, EditorConfig::default());
        let b = boundaries(&s);
        let pos = b[at % b.len()];
        let anchors = [doc.anchor(pos, Bias::Before), doc.anchor(pos, Bias::After)];
        let mut expect = [pos, pos];
        for st in &steps {
            let cur = doc.text().to_string();
            let c = cs(&cur, st);
            expect[0] = c.map_pos(expect[0], Bias::Before);
            expect[1] = c.map_pos(expect[1], Bias::After);
            doc.apply_remote(doc.version(), c, Origin::Remote).unwrap();
        }
        for (a, e) in anchors.iter().zip(expect) {
            let r = doc.resolve_anchor(a).unwrap();
            prop_assert_eq!(r, e);
            prop_assert!(doc.text().is_char_boundary(r));
        }
    }

    #[test]
    fn undo_then_redo_is_identity(s in text_strategy(), steps in prop::collection::vec(seeds(), 1..8)) {
        let mut doc = Document::new(&s, EditorConfig::default());
        let mut view = EditorView::new(&doc);
        let mut texts = vec![doc.text().to_string()];
        for (i, st) in steps.iter().enumerate() {
            let cur = doc.text().to_string();
            let c = cs(&cur, st);
            if c.is_empty() { continue; }
            let tr = Transaction::new(doc.version(), c).with_origin(Origin::Input(InputKind::Typing)).with_time(i as u64 * 5000);
            doc.apply(tr, &mut view).unwrap();
            texts.push(doc.text().to_string());
        }
        // Each edit is its own group (5 s apart): undo walks back through every text.
        for want in texts.iter().rev().skip(1) {
            doc.execute(&mut view, &EditorCommand::Undo, None).unwrap();
            prop_assert_eq!(&doc.text().to_string(), want);
        }
        prop_assert!(!doc.history().can_undo());
        for want in texts.iter().skip(1) {
            doc.execute(&mut view, &EditorCommand::Redo, None).unwrap();
            prop_assert_eq!(&doc.text().to_string(), want);
        }
        prop_assert!(!doc.history().can_redo());
        prop_assert!(view.selection().is_valid_in(doc.text()));
    }

    #[test]
    fn remote_interleaving_keeps_selections_valid(
        s in text_strategy(),
        steps in prop::collection::vec((any::<bool>(), seeds(), 0usize..64), 1..12),
    ) {
        let mut doc = Document::new(&s, EditorConfig::default());
        let mut view = EditorView::new(&doc);
        for (i, (remote, st, caret)) in steps.iter().enumerate() {
            let cur = doc.text().to_string();
            let c = cs(&cur, st);
            if *remote {
                doc.apply_remote(doc.version(), c, Origin::Remote).unwrap();
                view.sync(&doc);
            } else {
                let after = c.apply(doc.text()).unwrap();
                let b = boundaries(&after.to_string());
                let sel = Selection::point(b[caret % b.len()]);
                let tr = Transaction::new(doc.version(), c).with_selection(sel).with_origin(Origin::Input(InputKind::Typing)).with_time(i as u64 * 2000);
                doc.apply(tr, &mut view).unwrap();
            }
            prop_assert!(view.selection().is_valid_in(doc.text()));
        }
        // Local undo skips remote changes and still yields valid states.
        while doc.history().can_undo() {
            doc.execute(&mut view, &EditorCommand::Undo, None).unwrap();
            prop_assert!(view.selection().is_valid_in(doc.text()));
        }
    }

    #[test]
    fn two_views_stay_valid(
        s in text_strategy(),
        steps in prop::collection::vec((0u8..3, seeds(), 0usize..64), 1..30),
        window in 1usize..6,
    ) {
        let cfg = EditorConfig { recent_changes_window: window, ..EditorConfig::default() };
        let mut doc = Document::new(&s, cfg);
        let mut views = [EditorView::new(&doc), EditorView::new(&doc)];
        for (who, st, caret) in &steps {
            let cur = doc.text().to_string();
            let c = cs(&cur, st);
            match who {
                // view 0 or 1 edits (the other one may miss more edits than the window holds)
                0 | 1 => {
                    let v = &mut views[*who as usize];
                    v.sync(&doc);
                    let after = c.apply(doc.text()).unwrap();
                    let b = boundaries(&after.to_string());
                    let sel = Selection::point(b[caret % b.len()]);
                    let tr = Transaction::new(doc.version(), c).with_selection(sel);
                    doc.apply(tr, v).unwrap();
                }
                _ => {
                    doc.apply_remote(doc.version(), c, Origin::Remote).unwrap();
                }
            }
            for v in views.iter_mut() {
                let mut probe = v.clone();
                probe.sync(&doc);
                prop_assert!(probe.selection().is_valid_in(doc.text()));
                prop_assert_eq!(probe.synced_at(), doc.version());
            }
        }
        for v in views.iter_mut() {
            v.sync(&doc);
            prop_assert!(v.selection().is_valid_in(doc.text()));
        }
    }
}

#[test]
fn view_missing_more_than_the_window_clamps() {
    let cfg = EditorConfig {
        recent_changes_window: 2,
        ..EditorConfig::default()
    };
    let mut doc = Document::new("hello world", cfg);
    let mut a = EditorView::new(&doc);
    let mut b = EditorView::new(&doc);
    b.set_selection(&doc, Selection::point(11));
    for _ in 0..5 {
        let len = doc.text().len_bytes();
        let c = ChangeSet::delete_range(len, len - 1..len).unwrap();
        doc.apply(Transaction::new(doc.version(), c), &mut a)
            .unwrap();
    }
    assert_eq!(b.sync(&doc), SyncOutcome::Clamped);
    assert_eq!(b.selection().primary().head, 6);
    assert!(b.selection().is_valid_in(doc.text()));
}
