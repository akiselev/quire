// ported from svelters crates/svelters-egui/src/native/undo.rs @ e17ebbff

//! FX3: undo grouping per toolkit (WG3b pin 4, `plans/milestones/FX3-text-editing.md`).
//!
//! In quire-text, [`History`](crate::History) feeds these models from each applied change set
//! (`NativeRules::Gtk` / `NativeRules::Qt`), so the before/after reconstruction (`splice`) is gone.
//!
//! egui's `Undoer` groups by time (a point once the text was stable for a second), so a fast run of
//! edits is one step. GTK and Qt group by what the edit was. A field's [`NativeHistory`] is fed every
//! edit (the text before and after, and what made it, [`EditKind`]) and every caret move, and
//! answers Undo and Redo itself (on `ci/fx3`, egui's `TextEdit` never saw those keys).
//!
//! - `NativeRules::Gtk` (`GtkTextHistory`, fitted to the GTK 4.24.1 golden): an insertion merges
//!   into the previous one when it continues it, unless it starts with white space after a non-space
//!   (one step per word: `foo bar baz` undoes as `foo bar`, `foo`, `""`; a paste merges like typing).
//!   Each deletion is its own step, typing over a selection is one closed step with the deletion,
//!   and a caret move ends merging. Undoing a deletion puts the caret after the restored text, or
//!   selects it (caret at its start) when it was a selection.
//! - `NativeRules::Qt` (`QWidgetLineControl`'s command history, ported): one command per character
//!   (Insert, Remove for BackSpace, Delete for Delete, Remove/DeleteSelection for a selection, with a
//!   SetSelection before), Separators at caret moves, pastes and selection removals, and undo/redo
//!   walk the commands until the type changes, as `internalUndo`/`internalRedo` do. A typing run is
//!   one step; so is a deletion run and the typing that follows a selection removal.
//!
//! Offsets are chars.

/// Whose undo grouping a field follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeRules {
    /// GTK 4 (`GtkTextHistory`).
    Gtk,
    /// Qt 6 widgets (`QWidgetLineControl`).
    Qt,
}

/// What made an edit, read from the pass's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Typed text (a key's text, an input method's commit).
    Typing,
    /// A paste (clipboard or middle click).
    Paste,
    /// BackSpace.
    Backspace,
    /// Delete.
    Delete,
    /// Ctrl+BackSpace.
    WordBack,
    /// Ctrl+Delete.
    WordForward,
    /// Cut.
    Cut,
    /// Anything else (a binding the toolkits do not share, an accessibility value).
    Other,
}

/// A field's text and selection (`anchor`, `caret`; chars).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snap {
    /// The text.
    pub text: String,
    /// The selection's anchor (chars).
    pub anchor: usize,
    /// The caret (chars).
    pub caret: usize,
}

impl Snap {
    /// A snapshot (offsets clamped to the text).
    pub fn new(text: &str, anchor: usize, caret: usize) -> Snap {
        let len = text.chars().count();
        Snap { text: text.to_string(), anchor: anchor.min(len), caret: caret.min(len) }
    }

    fn lo(&self) -> usize {
        self.anchor.min(self.caret)
    }

    fn hi(&self) -> usize {
        self.anchor.max(self.caret)
    }

    fn selected(&self) -> bool {
        self.anchor != self.caret
    }
}

/// One edit as a splice: at `at`, `removed` became `inserted`.
#[derive(Clone, Debug)]
struct Splice {
    at: usize,
    removed: Vec<char>,
    inserted: Vec<char>,
}

/// A field's undo history.
#[derive(Clone, Debug)]
pub enum NativeHistory {
    /// GTK's.
    Gtk(GtkHistory),
    /// Qt's.
    Qt(QtHistory),
}

impl NativeHistory {
    /// An empty history.
    pub fn new(rules: NativeRules) -> NativeHistory {
        match rules {
            NativeRules::Gtk => NativeHistory::Gtk(GtkHistory::default()),
            NativeRules::Qt => NativeHistory::Qt(QtHistory::default()),
        }
    }

    /// The rule set.
    pub fn rules(&self) -> NativeRules {
        match self {
            NativeHistory::Gtk(_) => NativeRules::Gtk,
            NativeHistory::Qt(_) => NativeRules::Qt,
        }
    }

    /// The text of `before` changed because of `kind`: at char `at`, `removed` became `inserted`.
    pub fn edit(&mut self, before: &Snap, at: usize, removed: Vec<char>, inserted: Vec<char>, kind: EditKind) {
        if removed == inserted {
            return;
        }
        let s = Splice { at, removed, inserted };
        match self {
            NativeHistory::Gtk(h) => h.edit(before, s),
            NativeHistory::Qt(h) => h.edit(before, s, kind),
        }
    }

    /// The caret or selection changed without an edit.
    pub fn moved(&mut self, before: &Snap, after: &Snap) {
        match self {
            NativeHistory::Gtk(h) => h.merge &= before.caret == after.caret && before.anchor == after.anchor,
            NativeHistory::Qt(h) => h.separator |= before.caret != after.caret,
        }
    }

    /// Keeps at most `n` undo steps (the oldest are dropped).
    pub fn limit(&mut self, n: usize) {
        match self {
            NativeHistory::Gtk(h) => {
                if h.undo.len() > n {
                    let drop = h.undo.len() - n;
                    h.undo.drain(..drop);
                }
            }
            NativeHistory::Qt(h) => {
                // Steps start at separators; drop whole steps from the front.
                let seps: Vec<usize> = (0..h.state).filter(|&i| h.cmds[i].ty == Q::Separator).collect();
                if seps.len() > n {
                    let cut = seps[seps.len() - n];
                    h.cmds.drain(..cut);
                    h.state -= cut;
                }
            }
        }
    }

    /// Undoes one step of `now`; `None` when there is nothing to undo.
    pub fn undo(&mut self, now: &Snap) -> Option<Snap> {
        let r = match self {
            NativeHistory::Gtk(h) => h.undo(now),
            NativeHistory::Qt(h) => h.undo(now),
        };
        if r.is_none() && !self.can_undo() {
            return None;
        }
        r.or_else(|| {
            // inconsistent with the text (should not happen): forget everything
            *self = NativeHistory::new(self.rules());
            None
        })
    }

    /// Redoes one step of `now`; `None` when there is nothing to redo.
    pub fn redo(&mut self, now: &Snap) -> Option<Snap> {
        let had = self.can_redo();
        let r = match self {
            NativeHistory::Gtk(h) => h.redo(now),
            NativeHistory::Qt(h) => h.redo(now),
        };
        if r.is_none() && had {
            *self = NativeHistory::new(self.rules());
        }
        r
    }

    /// Whether undo would do something.
    pub fn can_undo(&self) -> bool {
        match self {
            NativeHistory::Gtk(h) => !h.undo.is_empty(),
            NativeHistory::Qt(h) => h.cmds[..h.state].iter().any(|c| c.ty != Q::Separator),
        }
    }

    /// Whether redo would do something.
    pub fn can_redo(&self) -> bool {
        match self {
            NativeHistory::Gtk(h) => !h.redo.is_empty(),
            NativeHistory::Qt(h) => h.state < h.cmds.len(),
        }
    }
}

fn insert(text: &mut Vec<char>, at: usize, s: &[char]) -> Option<()> {
    (at <= text.len()).then(|| {
        text.splice(at..at, s.iter().copied());
    })
}

fn remove(text: &mut Vec<char>, at: usize, n: usize) -> Option<()> {
    (at + n <= text.len()).then(|| {
        text.drain(at..at + n);
    })
}

#[derive(Clone, Debug)]
enum G {
    Insert { at: usize, text: Vec<char> },
    Delete { at: usize, text: Vec<char>, selected: bool },
    Replace { at: usize, removed: Vec<char>, inserted: Vec<char> },
}

/// `NativeRules::Gtk`.
#[derive(Clone, Debug, Default)]
pub struct GtkHistory {
    undo: Vec<G>,
    redo: Vec<G>,
    /// The next insertion may merge into the last one.
    merge: bool,
}

impl GtkHistory {
    fn edit(&mut self, before: &Snap, s: Splice) {
        self.redo.clear();
        if s.removed.is_empty() {
            let starts_space = s.inserted.first().is_some_and(|c| c.is_whitespace());
            if self.merge
                && let Some(G::Insert { at, text }) = self.undo.last_mut()
                && *at + text.len() == s.at
                && !(starts_space && text.last().is_some_and(|c| !c.is_whitespace()))
            {
                text.extend(s.inserted);
            } else {
                self.undo.push(G::Insert { at: s.at, text: s.inserted });
            }
            self.merge = true;
            return;
        }
        let selected = before.selected() && s.at == before.lo() && s.removed.len() == before.hi() - before.lo();
        if s.inserted.is_empty() {
            self.undo.push(G::Delete { at: s.at, text: s.removed, selected });
        } else {
            self.undo.push(G::Replace { at: s.at, removed: s.removed, inserted: s.inserted });
        }
        self.merge = false;
    }

    fn undo(&mut self, now: &Snap) -> Option<Snap> {
        let a = self.undo.pop()?;
        self.merge = false;
        let mut t: Vec<char> = now.text.chars().collect();
        let (anchor, caret) = match &a {
            G::Insert { at, text } => {
                remove(&mut t, *at, text.len())?;
                (*at, *at)
            }
            G::Delete { at, text, selected } => {
                insert(&mut t, *at, text)?;
                if *selected { (at + text.len(), *at) } else { (at + text.len(), at + text.len()) }
            }
            G::Replace { at, removed, inserted } => {
                remove(&mut t, *at, inserted.len())?;
                insert(&mut t, *at, removed)?;
                (at + removed.len(), *at)
            }
        };
        self.redo.push(a);
        Some(Snap { text: t.into_iter().collect(), anchor, caret })
    }

    fn redo(&mut self, now: &Snap) -> Option<Snap> {
        let a = self.redo.pop()?;
        self.merge = false;
        let mut t: Vec<char> = now.text.chars().collect();
        let caret = match &a {
            G::Insert { at, text } => {
                insert(&mut t, *at, text)?;
                at + text.len()
            }
            G::Delete { at, text, .. } => {
                remove(&mut t, *at, text.len())?;
                *at
            }
            G::Replace { at, removed, inserted } => {
                remove(&mut t, *at, removed.len())?;
                insert(&mut t, *at, inserted)?;
                at + inserted.len()
            }
        };
        self.undo.push(a);
        Some(Snap { text: t.into_iter().collect(), anchor: caret, caret })
    }
}

/// `QWidgetLineControl::CommandType`, in its order (the walk compares them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Q {
    Separator,
    Insert,
    Remove,
    Delete,
    RemoveSelection,
    DeleteSelection,
    SetSelection,
}

#[derive(Clone, Copy, Debug)]
struct Cmd {
    ty: Q,
    pos: usize,
    ch: char,
    /// `[start, end)` (SetSelection, Separator).
    sel: Option<(usize, usize)>,
}

/// `QWidgetLineControl`'s line: text, cursor, selection `[start, end)`.
struct Line {
    text: Vec<char>,
    cursor: usize,
    sel: Option<(usize, usize)>,
}

impl Line {
    fn of(s: &Snap) -> Line {
        Line { text: s.text.chars().collect(), cursor: s.caret, sel: s.selected().then(|| (s.lo(), s.hi())) }
    }

    fn snap(self) -> Snap {
        let len = self.text.len();
        let cursor = self.cursor.min(len);
        let (anchor, caret) = match self.sel {
            Some((s, e)) if s < e && e <= len => (if cursor == s { e } else { s }, cursor),
            _ => (cursor, cursor),
        };
        Snap { text: self.text.into_iter().collect(), anchor, caret }
    }
}

/// `NativeRules::Qt`.
#[derive(Clone, Debug, Default)]
pub struct QtHistory {
    cmds: Vec<Cmd>,
    /// `m_undoState`: commands below it are done.
    state: usize,
    /// `m_separator`: the next command starts a new step.
    separator: bool,
}

impl QtHistory {
    /// `addCommand`; `cursor` and `sel` are the line's at that moment (a pending separator takes them).
    fn add(&mut self, ty: Q, pos: usize, ch: char, sel: Option<(usize, usize)>, cursor: usize, line_sel: Option<(usize, usize)>) {
        self.cmds.truncate(self.state);
        if self.separator && ty != Q::Separator {
            self.cmds.push(Cmd { ty: Q::Separator, pos: cursor, ch: '\0', sel: line_sel });
        }
        self.separator = false;
        self.cmds.push(Cmd { ty, pos, ch, sel });
        self.state = self.cmds.len();
    }

    /// `removeSelectedText` on `text` with `cursor` and the selection `[s, e)`.
    fn remove_selected(&mut self, text: &[char], cursor: usize, s: usize, e: usize) {
        self.separator = true;
        self.add(Q::SetSelection, cursor, '\0', Some((s, e)), cursor, Some((s, e)));
        if s <= cursor && cursor < e {
            // the cursor is inside: split the commands so that undo puts it back
            for i in (s..=cursor).rev() {
                self.add(Q::DeleteSelection, i, text[i], None, cursor, None);
            }
            for i in (cursor + 1..e).rev() {
                self.add(Q::DeleteSelection, i - cursor + s - 1, text[i], None, cursor, None);
            }
        } else {
            for i in (s..e).rev() {
                self.add(Q::RemoveSelection, i, text[i], None, cursor, None);
            }
        }
    }

    fn edit(&mut self, before: &Snap, s: Splice, kind: EditKind) {
        let line = Line::of(before);
        let paste = kind == EditKind::Paste;
        if paste {
            self.separator = true;
        }
        if !s.removed.is_empty() {
            let r = s.removed.len();
            let is_sel = line.sel == Some((s.at, s.at + r));
            match kind {
                _ if is_sel => self.remove_selected(&line.text, line.cursor, s.at, s.at + r),
                // `cursorWordBackward(true)`/`cursorWordForward(true)` then `del()`
                EditKind::WordBack | EditKind::Cut | EditKind::Other | EditKind::Paste | EditKind::Typing => {
                    self.separator = true;
                    self.remove_selected(&line.text, s.at, s.at, s.at + r)
                }
                EditKind::WordForward => {
                    self.separator = true;
                    self.remove_selected(&line.text, s.at + r, s.at, s.at + r)
                }
                EditKind::Delete => {
                    for c in &s.removed {
                        self.add(Q::Delete, s.at, *c, None, s.at, None);
                    }
                }
                EditKind::Backspace => {
                    for k in (0..r).rev() {
                        self.add(Q::Remove, s.at + k, s.removed[k], None, s.at + k, None);
                    }
                }
            }
        }
        for (k, c) in s.inserted.iter().enumerate() {
            // `addCommand(Command(Insert, m_cursor++, …))`: a pending separator sees the cursor after it
            self.add(Q::Insert, s.at + k, *c, None, s.at + k + 1, None);
        }
        if paste {
            self.separator = true;
        }
    }

    /// `internalUndo(-1)`.
    fn undo(&mut self, now: &Snap) -> Option<Snap> {
        if !self.cmds[..self.state].iter().any(|c| c.ty != Q::Separator) {
            return None;
        }
        let mut l = Line::of(now);
        l.sel = None;
        while self.state > 0 {
            self.state -= 1;
            let cmd = self.cmds[self.state];
            match cmd.ty {
                Q::Insert => {
                    remove(&mut l.text, cmd.pos, 1)?;
                    l.cursor = cmd.pos;
                }
                Q::SetSelection => {
                    l.sel = cmd.sel;
                    l.cursor = cmd.pos;
                }
                Q::Remove | Q::RemoveSelection => {
                    insert(&mut l.text, cmd.pos, &[cmd.ch])?;
                    l.cursor = cmd.pos + 1;
                }
                Q::Delete | Q::DeleteSelection => {
                    insert(&mut l.text, cmd.pos, &[cmd.ch])?;
                    l.cursor = cmd.pos;
                }
                Q::Separator => continue,
            }
            if self.state > 0 {
                let next = self.cmds[self.state - 1].ty;
                if next != cmd.ty && next < Q::RemoveSelection && (cmd.ty < Q::RemoveSelection || next == Q::Separator) {
                    break;
                }
            }
        }
        Some(l.snap())
    }

    /// `internalRedo()`.
    fn redo(&mut self, now: &Snap) -> Option<Snap> {
        if self.state >= self.cmds.len() {
            return None;
        }
        let mut l = Line::of(now);
        l.sel = None;
        while self.state < self.cmds.len() {
            let cmd = self.cmds[self.state];
            self.state += 1;
            match cmd.ty {
                Q::Insert => {
                    insert(&mut l.text, cmd.pos, &[cmd.ch])?;
                    l.cursor = cmd.pos + 1;
                }
                Q::SetSelection | Q::Separator => {
                    l.sel = cmd.sel;
                    l.cursor = cmd.pos;
                }
                Q::Remove | Q::Delete | Q::RemoveSelection | Q::DeleteSelection => {
                    remove(&mut l.text, cmd.pos, 1)?;
                    l.sel = cmd.sel;
                    l.cursor = cmd.pos;
                }
            }
            if self.state < self.cmds.len() {
                let next = self.cmds[self.state].ty;
                if next != cmd.ty
                    && cmd.ty < Q::RemoveSelection
                    && next != Q::Separator
                    && (next < Q::RemoveSelection || cmd.ty == Q::Separator)
                {
                    break;
                }
            }
        }
        Some(l.snap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field driven like the pins: edits and moves recorded, undo/redo answered by the history.
    struct F {
        h: NativeHistory,
        s: Snap,
    }

    impl F {
        fn new(rules: NativeRules, text: &str, at: usize) -> F {
            F { h: NativeHistory::new(rules), s: Snap::new(text, at, at) }
        }
        fn set(&mut self, next: Snap, at: usize, removed: Vec<char>, inserted: &[char], kind: EditKind) {
            self.h.edit(&self.s, at, removed, inserted.to_vec(), kind);
            self.s = next;
        }
        fn typ(&mut self, t: &str) {
            for c in t.chars() {
                let mut x: Vec<char> = self.s.text.chars().collect();
                let (lo, hi) = (self.s.lo(), self.s.hi());
                let removed: Vec<char> = x.splice(lo..hi, [c]).collect();
                self.set(Snap { text: x.into_iter().collect(), anchor: lo + 1, caret: lo + 1 }, lo, removed, &[c], EditKind::Typing);
            }
        }
        fn backspace(&mut self) {
            let mut x: Vec<char> = self.s.text.chars().collect();
            let c = self.s.caret;
            let r = x.remove(c - 1);
            self.set(Snap { text: x.into_iter().collect(), anchor: c - 1, caret: c - 1 }, c - 1, vec![r], &[], EditKind::Backspace);
        }
        fn word_back(&mut self, to: usize) {
            let mut x: Vec<char> = self.s.text.chars().collect();
            let removed: Vec<char> = x.drain(to..self.s.caret).collect();
            self.set(Snap { text: x.into_iter().collect(), anchor: to, caret: to }, to, removed, &[], EditKind::WordBack);
        }
        fn paste(&mut self, t: &str) {
            let mut x: Vec<char> = self.s.text.chars().collect();
            let lo = self.s.lo();
            let ins: Vec<char> = t.chars().collect();
            let removed: Vec<char> = x.splice(lo..self.s.hi(), ins.iter().copied()).collect();
            let at = lo + ins.len();
            self.set(Snap { text: x.into_iter().collect(), anchor: at, caret: at }, lo, removed, &ins, EditKind::Paste);
        }
        fn select(&mut self, anchor: usize, caret: usize) {
            let next = Snap { anchor, caret, ..self.s.clone() };
            self.h.moved(&self.s, &next);
            self.s = next;
        }
        fn undo(&mut self) -> (String, [usize; 2]) {
            if let Some(n) = self.h.undo(&self.s) {
                self.s = n;
            }
            (self.s.text.clone(), [self.s.anchor, self.s.caret])
        }
        fn redo(&mut self) -> (String, [usize; 2]) {
            if let Some(n) = self.h.redo(&self.s) {
                self.s = n;
            }
            (self.s.text.clone(), [self.s.anchor, self.s.caret])
        }
    }

    fn t(s: &str, a: usize, c: usize) -> (String, [usize; 2]) {
        (s.to_string(), [a, c])
    }

    #[test]
    fn fx3_undo_typing_against_both_goldens() {
        let mut g = F::new(NativeRules::Gtk, "", 0);
        g.typ("foo bar baz");
        assert_eq!(g.undo(), t("foo bar", 7, 7));
        assert_eq!(g.undo(), t("foo", 3, 3));
        assert_eq!(g.undo(), t("", 0, 0));
        assert_eq!(g.undo(), t("", 0, 0));
        assert_eq!(g.redo(), t("foo", 3, 3));
        assert_eq!(g.redo(), t("foo bar", 7, 7));
        assert_eq!(g.redo(), t("foo bar baz", 11, 11));
        let mut q = F::new(NativeRules::Qt, "", 0);
        q.typ("foo bar baz");
        assert_eq!(q.undo(), t("", 0, 0));
        assert_eq!(q.undo(), t("", 0, 0));
        assert_eq!(q.redo(), t("foo bar baz", 11, 11));
        assert_eq!(q.redo(), t("foo bar baz", 11, 11));
        assert_eq!(q.undo(), t("", 0, 0));
    }

    fn undo_edits(rules: NativeRules) -> F {
        let mut f = F::new(rules, "alpha beta gamma", 16);
        f.word_back(11);
        f.typ("delta");
        f.backspace();
        f.backspace();
        f.select(13, 13);
        f.select(12, 12);
        f.typ("x");
        f.select(0, 15);
        f.typ("new");
        f
    }

    #[test]
    fn fx3_undo_edits_against_both_goldens() {
        let mut g = undo_edits(NativeRules::Gtk);
        assert_eq!(g.undo(), t("n", 1, 1));
        assert_eq!(g.undo(), t("alpha beta dxel", 15, 0));
        assert_eq!(g.undo(), t("alpha beta del", 12, 12));
        assert_eq!(g.undo(), t("alpha beta delt", 15, 15));
        assert_eq!(g.undo(), t("alpha beta delta", 16, 16));
        assert_eq!(g.undo(), t("alpha beta ", 11, 11));
        assert_eq!(g.undo(), t("alpha beta gamma", 16, 16));
        assert_eq!(g.undo(), t("alpha beta gamma", 16, 16));
        assert_eq!(g.redo(), t("alpha beta ", 11, 11));
        // GTK selects the last character here ([15, 16]); not modelled
        assert_eq!(g.redo().0, "alpha beta delta");
        assert_eq!(g.redo(), t("alpha beta delt", 15, 15));
        let mut q = undo_edits(NativeRules::Qt);
        assert_eq!(q.undo(), t("alpha beta dxel", 0, 15));
        assert_eq!(q.undo(), t("alpha beta del", 12, 12));
        assert_eq!(q.undo(), t("alpha beta delta", 16, 16));
        assert_eq!(q.undo(), t("alpha beta gamma", 16, 11));
        for _ in 0..4 {
            assert_eq!(q.undo(), t("alpha beta gamma", 16, 11));
        }
        assert_eq!(q.redo(), t("alpha beta delta", 16, 16));
        assert_eq!(q.redo(), t("alpha beta del", 13, 13));
        assert_eq!(q.redo(), t("alpha beta dxel", 0, 15));
    }

    #[test]
    fn fx3_undo_paste_against_both_goldens() {
        for rules in [NativeRules::Gtk, NativeRules::Qt] {
            let mut f = F::new(rules, "one two", 7);
            f.typ(" three ");
            f.paste("PASTED");
            f.typ(" four");
            assert_eq!(f.undo(), t("one two three PASTED", 20, 20));
            let (three, four) = if rules == NativeRules::Gtk { (13, "one two three") } else { (14, "one two three ") };
            assert_eq!(f.undo(), t(four, three, three));
            assert_eq!(f.undo(), t("one two", 7, 7));
            assert_eq!(f.undo(), t("one two", 7, 7));
            assert_eq!(f.redo(), t(four, three, three));
            assert_eq!(f.redo(), t("one two three PASTED", 20, 20));
        }
    }
}
