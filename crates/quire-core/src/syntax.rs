//! The syntax seam: the [`Highlighter`] trait, the host's [`Spawner`], [`CancelFlag`], packed
//! token styles and Monaco's compact per-line [`TokenStore`].
//!
//! - **Token style** ([`TokenStyle`], a `u32`): bits 0–7 the [`TokenType`] (LSP semantic token
//!   types plus a few tree-sitter capture kinds), bits 8–23 [`TokenModifiers`] (LSP's ten plus
//!   room), bits 24–31 a host-assigned language index (for embedded languages). Themes resolve
//!   styles at paint time, so a theme switch repaints without re-tokenising.
//! - **Token store:** per line, a sorted list of `(start byte in line, style)`; a token runs to
//!   the next token's start or the line end. Edits shift tokens immediately
//!   ([`TokenStore::shift`]): text inserted inside a token takes that token's style and later
//!   tokens move, so nothing flickers until fresh tokens arrive. Shifted lines are marked invalid
//!   from the first edited line ([`TokenStore::invalid_from`]).
//! - The `SyntaxDriver` (debounce, parse jobs on snapshots through the `Spawner`, results applied
//!   by version) lives with the highlighters in `quire-syntax`; this module only defines the seam.

use crate::change::ChangeSet;
use crate::document::Snapshot;
use crate::recent::LineEdit;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Token types, aligned with LSP 3.17 semantic token types (first 23, in LSP's order), then
/// kinds tree-sitter highlight queries need. `Text` (0) is unstyled.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(u8)]
#[allow(missing_docs)]
pub enum TokenType {
    #[default]
    Text = 0,
    Namespace,
    Type,
    Class,
    Enum,
    Interface,
    Struct,
    TypeParameter,
    Parameter,
    Variable,
    Property,
    EnumMember,
    Event,
    Function,
    Method,
    Macro,
    Keyword,
    Modifier,
    Comment,
    String,
    Number,
    Regexp,
    Operator,
    Decorator,
    // ---- beyond LSP ----
    Punctuation,
    PunctuationBracket,
    PunctuationDelimiter,
    Tag,
    Attribute,
    Constant,
    Boolean,
    StringEscape,
    StringSpecial,
    Label,
    Constructor,
    Embedded,
    Error,
}

impl TokenType {
    /// Every type, in discriminant order.
    pub const ALL: &'static [TokenType] = &[
        TokenType::Text,
        TokenType::Namespace,
        TokenType::Type,
        TokenType::Class,
        TokenType::Enum,
        TokenType::Interface,
        TokenType::Struct,
        TokenType::TypeParameter,
        TokenType::Parameter,
        TokenType::Variable,
        TokenType::Property,
        TokenType::EnumMember,
        TokenType::Event,
        TokenType::Function,
        TokenType::Method,
        TokenType::Macro,
        TokenType::Keyword,
        TokenType::Modifier,
        TokenType::Comment,
        TokenType::String,
        TokenType::Number,
        TokenType::Regexp,
        TokenType::Operator,
        TokenType::Decorator,
        TokenType::Punctuation,
        TokenType::PunctuationBracket,
        TokenType::PunctuationDelimiter,
        TokenType::Tag,
        TokenType::Attribute,
        TokenType::Constant,
        TokenType::Boolean,
        TokenType::StringEscape,
        TokenType::StringSpecial,
        TokenType::Label,
        TokenType::Constructor,
        TokenType::Embedded,
        TokenType::Error,
    ];

    /// The type from its discriminant.
    pub fn from_u8(v: u8) -> Option<TokenType> {
        Self::ALL.get(v as usize).copied()
    }

    /// The LSP legend name (`"typeParameter"`), or quire's name for the extra kinds
    /// (`"punctuation.bracket"`, tree-sitter capture style).
    pub fn name(self) -> &'static str {
        use TokenType::*;
        match self {
            Text => "text",
            Namespace => "namespace",
            Type => "type",
            Class => "class",
            Enum => "enum",
            Interface => "interface",
            Struct => "struct",
            TypeParameter => "typeParameter",
            Parameter => "parameter",
            Variable => "variable",
            Property => "property",
            EnumMember => "enumMember",
            Event => "event",
            Function => "function",
            Method => "method",
            Macro => "macro",
            Keyword => "keyword",
            Modifier => "modifier",
            Comment => "comment",
            String => "string",
            Number => "number",
            Regexp => "regexp",
            Operator => "operator",
            Decorator => "decorator",
            Punctuation => "punctuation",
            PunctuationBracket => "punctuation.bracket",
            PunctuationDelimiter => "punctuation.delimiter",
            Tag => "tag",
            Attribute => "attribute",
            Constant => "constant",
            Boolean => "boolean",
            StringEscape => "string.escape",
            StringSpecial => "string.special",
            Label => "label",
            Constructor => "constructor",
            Embedded => "embedded",
            Error => "error",
        }
    }

    /// The type for a name from [`TokenType::name`].
    pub fn from_name(name: &str) -> Option<TokenType> {
        Self::ALL.iter().copied().find(|t| t.name() == name)
    }
}

/// Token modifiers: LSP 3.17's ten standard modifiers as bits 0–9.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct TokenModifiers(pub u16);

#[allow(missing_docs)]
impl TokenModifiers {
    pub const NONE: TokenModifiers = TokenModifiers(0);
    pub const DECLARATION: TokenModifiers = TokenModifiers(1 << 0);
    pub const DEFINITION: TokenModifiers = TokenModifiers(1 << 1);
    pub const READONLY: TokenModifiers = TokenModifiers(1 << 2);
    pub const STATIC: TokenModifiers = TokenModifiers(1 << 3);
    pub const DEPRECATED: TokenModifiers = TokenModifiers(1 << 4);
    pub const ABSTRACT: TokenModifiers = TokenModifiers(1 << 5);
    pub const ASYNC: TokenModifiers = TokenModifiers(1 << 6);
    pub const MODIFICATION: TokenModifiers = TokenModifiers(1 << 7);
    pub const DOCUMENTATION: TokenModifiers = TokenModifiers(1 << 8);
    pub const DEFAULT_LIBRARY: TokenModifiers = TokenModifiers(1 << 9);

    /// Whether every bit of `other` is set.
    pub fn contains(self, other: TokenModifiers) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for TokenModifiers {
    type Output = TokenModifiers;
    fn bitor(self, rhs: Self) -> Self {
        TokenModifiers(self.0 | rhs.0)
    }
}

/// A packed token style: type, modifiers and language in one `u32`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct TokenStyle(pub u32);

impl TokenStyle {
    /// Packs a style.
    pub fn new(ty: TokenType, modifiers: TokenModifiers, language: u8) -> Self {
        TokenStyle(ty as u32 | (modifiers.0 as u32) << 8 | (language as u32) << 24)
    }

    /// A style with only a type.
    pub fn of(ty: TokenType) -> Self {
        TokenStyle::new(ty, TokenModifiers::NONE, 0)
    }

    /// The type (unknown discriminants read as `Text`).
    pub fn ty(self) -> TokenType {
        TokenType::from_u8((self.0 & 0xff) as u8).unwrap_or_default()
    }

    /// The modifiers.
    pub fn modifiers(self) -> TokenModifiers {
        TokenModifiers(((self.0 >> 8) & 0xffff) as u16)
    }

    /// The host-assigned language index.
    pub fn language(self) -> u8 {
        (self.0 >> 24) as u8
    }
}

/// One token: it starts at `start` (byte column in its line) and runs to the next token or the
/// line end.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[repr(C)]
pub struct Token {
    /// Byte column in the line.
    pub start: u32,
    /// The style.
    pub style: TokenStyle,
}

impl Token {
    /// A token.
    pub fn new(start: u32, style: TokenStyle) -> Self {
        Token { start, style }
    }
}

/// Highlighter output for a block of lines.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TokenRuns {
    /// The first line.
    pub first_line: usize,
    /// Tokens per line from `first_line`, each sorted by start.
    pub lines: Vec<Vec<Token>>,
}

impl TokenRuns {
    /// The covered line range.
    pub fn line_range(&self) -> Range<usize> {
        self.first_line..self.first_line + self.lines.len()
    }
}

/// Monaco's compact per-line token store.
#[derive(Clone, Debug)]
pub struct TokenStore {
    lines: Vec<Arc<[Token]>>,
    invalid_from: Option<usize>,
    generation: u64,
}

impl Default for TokenStore {
    fn default() -> Self {
        TokenStore::new(1)
    }
}

fn empty_line() -> Arc<[Token]> {
    Arc::from(Vec::new())
}

fn normalize(tokens: &mut Vec<Token>) {
    // Sorted by start, unique starts (the later wins), and no repeated styles.
    tokens.sort_by_key(|t| t.start);
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    for t in tokens.drain(..) {
        if let Some(last) = out.last_mut() {
            if last.start == t.start {
                *last = t;
                continue;
            }
            if last.style == t.style {
                continue;
            }
        }
        out.push(t);
    }
    // A leading run may repeat after the start == merge; fix once more.
    out.dedup_by(|b, a| a.style == b.style);
    *tokens = out;
}

impl TokenStore {
    /// An empty store for `lines` lines; every line is invalid.
    pub fn new(lines: usize) -> Self {
        TokenStore {
            lines: vec![empty_line(); lines.max(1)],
            invalid_from: Some(0),
            generation: 0,
        }
    }

    /// Number of lines (matches the text's line count).
    pub fn len_lines(&self) -> usize {
        self.lines.len()
    }

    /// The tokens of a line (empty for unknown lines).
    pub fn line(&self, i: usize) -> &[Token] {
        self.lines.get(i).map_or(&[], |l| l)
    }

    /// The style at a byte column of a line.
    pub fn style_at(&self, line: usize, col: usize) -> TokenStyle {
        let toks = self.line(line);
        let i = toks.partition_point(|t| t.start as usize <= col);
        if i == 0 {
            TokenStyle::default()
        } else {
            toks[i - 1].style
        }
    }

    /// The first line whose tokens were shifted by an edit and not re-tokenised since (`None`:
    /// all fresh).
    pub fn invalid_from(&self) -> Option<usize> {
        self.invalid_from
    }

    /// Bumps on every change, for render caches.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Stores highlighter output for its lines (lines past the end are ignored). Lines it covers
    /// stop being invalid when the runs reach the end of the invalid region.
    pub fn set(&mut self, runs: TokenRuns) {
        let first = runs.first_line;
        let n = runs.lines.len();
        for (i, mut toks) in runs.lines.into_iter().enumerate() {
            if let Some(slot) = self.lines.get_mut(first + i) {
                normalize(&mut toks);
                *slot = Arc::from(toks);
            }
        }
        if let Some(inv) = self.invalid_from
            && first <= inv
            && first + n > inv
        {
            self.invalid_from = if first + n >= self.lines.len() {
                None
            } else {
                Some(first + n)
            };
        }
        self.generation += 1;
    }

    /// Marks lines from `line` invalid.
    pub fn invalidate_from(&mut self, line: usize) {
        self.invalid_from = Some(self.invalid_from.map_or(line, |l| l.min(line)));
    }

    /// Marks every line fresh (the host or driver re-tokenised everything).
    pub fn mark_valid(&mut self) {
        self.invalid_from = None;
    }

    /// Shifts tokens through one change's line edits (base order, old coordinates).
    pub fn shift(&mut self, edits: &[LineEdit]) {
        if edits.is_empty() {
            return;
        }
        for e in edits.iter().rev() {
            self.shift_one(e);
        }
        self.generation += 1;
    }

    fn shift_one(&mut self, e: &LineEdit) {
        let sl = e.start_line.min(self.lines.len() - 1);
        let el = e.end_line.min(self.lines.len() - 1);
        let first = self.lines[sl].clone();
        let last = self.lines[el].clone();
        let sc = e.start_col as u32;
        let ec = e.end_col as u32;
        let mut prefix: Vec<Token> = first.iter().copied().filter(|t| t.start < sc).collect();
        let left_style = prefix.last().map(|t| t.style).or_else(|| {
            // An insertion at a token's start takes that token's style.
            first.iter().find(|t| t.start == sc).map(|t| t.style)
        });
        let contain = last.iter().rev().find(|t| t.start <= ec).map(|t| t.style);
        let base = if e.inserted_lines == 0 {
            sc + e.inserted_first_len as u32
        } else {
            e.inserted_last_len as u32
        };
        let mut suffix: Vec<Token> = Vec::new();
        if let Some(s) = contain {
            suffix.push(Token::new(base, s));
        }
        suffix.extend(
            last.iter()
                .filter(|t| t.start > ec)
                .map(|t| Token::new(t.start - ec + base, t.style)),
        );
        let mut new_lines: Vec<Arc<[Token]>> = Vec::with_capacity(e.inserted_lines + 1);
        if e.inserted_lines == 0 {
            if e.inserted_first_len > 0
                && prefix.is_empty()
                && let Some(s) = left_style
            {
                prefix.push(Token::new(sc, s));
            }
            prefix.extend(suffix);
            normalize(&mut prefix);
            new_lines.push(Arc::from(prefix));
        } else {
            if e.inserted_first_len > 0
                && prefix.is_empty()
                && let Some(s) = left_style
            {
                prefix.push(Token::new(sc, s));
            }
            normalize(&mut prefix);
            new_lines.push(Arc::from(prefix));
            let mid: Arc<[Token]> = match left_style {
                Some(s) => Arc::from(vec![Token::new(0, s)]),
                None => empty_line(),
            };
            for _ in 1..e.inserted_lines {
                new_lines.push(mid.clone());
            }
            let mut lastv: Vec<Token> = Vec::new();
            if e.inserted_last_len > 0
                && let Some(s) = left_style
            {
                lastv.push(Token::new(0, s));
            }
            lastv.extend(suffix);
            normalize(&mut lastv);
            new_lines.push(Arc::from(lastv));
        }
        self.lines.splice(sl..=el, new_lines);
        self.invalidate_from(sl);
    }

    /// Resets to `lines` empty lines, all invalid.
    pub fn reset(&mut self, lines: usize) {
        *self = TokenStore {
            generation: self.generation + 1,
            ..TokenStore::new(lines)
        };
    }
}

/// Maps token runs computed `edits_since` changes ago to now: whole-line shifts for edits
/// entirely before the runs, truncation at the first line an edit touches. Returns `None` when
/// nothing is left.
pub(crate) fn map_runs<'a>(
    mut runs: TokenRuns,
    changes: impl Iterator<Item = &'a [LineEdit]>,
) -> Option<TokenRuns> {
    for edits in changes {
        for e in edits.iter().rev() {
            let end = runs.first_line + runs.lines.len();
            if e.start_line >= end {
                continue;
            }
            if e.end_line < runs.first_line {
                runs.first_line = (runs.first_line as isize + e.line_delta()).max(0) as usize;
                continue;
            }
            // Touches the block: keep lines before the edit only.
            let keep = e.start_line.saturating_sub(runs.first_line);
            runs.lines.truncate(keep);
        }
        if runs.lines.is_empty() {
            return None;
        }
    }
    Some(runs)
}

/// A pair of matching brackets (byte ranges).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct BracketPair {
    /// The opening bracket.
    pub open: Range<usize>,
    /// The closing bracket.
    pub close: Range<usize>,
}

/// What a fold range folds.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum FoldKind {
    /// A comment block.
    Comment,
    /// An import block.
    Imports,
    /// A `#region` marker pair.
    Region,
    /// Anything else (a block).
    #[default]
    Other,
}

/// A foldable line range: `start_line` stays visible, lines after it through `end_line` fold.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FoldRange {
    /// The header line.
    pub start_line: usize,
    /// The last folded line.
    pub end_line: usize,
    /// The kind.
    pub kind: FoldKind,
}

/// An indentation hint for a new line.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum IndentHint {
    /// Same indentation as the previous line.
    Keep,
    /// One level deeper.
    Indent,
    /// One level shallower.
    Outdent,
    /// An absolute indentation in columns.
    Columns(u32),
}

/// A cooperative cancellation flag for background work. Cloning shares the flag.
#[derive(Clone, Debug, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// A fresh flag.
    pub fn new() -> Self {
        CancelFlag::default()
    }
    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Host-owned threads: the editor never spawns. A synchronous spawner (run inline) is valid,
/// e.g. in tests or for a native reparse under 0.5 ms.
pub trait Spawner: Send + Sync {
    /// Runs `job` somewhere.
    fn spawn(&self, job: Box<dyn FnOnce() + Send>);
}

/// Runs jobs inline on the calling thread.
#[derive(Copy, Clone, Debug, Default)]
pub struct InlineSpawner;

impl Spawner for InlineSpawner {
    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        job();
    }
}

/// A language's highlighter. Tree types are associated so tree-sitter and native CSTs both fit.
pub trait Highlighter: Send + Sync + 'static {
    /// The parse tree.
    type Tree: Send + Sync + 'static;
    /// The language id (e.g. `"svelte"`).
    fn language(&self) -> &str;
    /// Parses a snapshot, incrementally from the previous tree and the changes since it when
    /// given. Returns `None` if cancelled.
    fn parse(
        &self,
        snap: &Snapshot,
        prev: Option<(&Self::Tree, &ChangeSet)>,
        cancel: &CancelFlag,
    ) -> Option<Self::Tree>;
    /// Tokens for a line range.
    fn highlights(&self, tree: &Self::Tree, snap: &Snapshot, lines: Range<usize>) -> TokenRuns;
    /// Bracket pairs in a line range.
    fn brackets(&self, _tree: &Self::Tree, _lines: Range<usize>) -> Vec<BracketPair> {
        Vec::new()
    }
    /// Fold ranges.
    fn folds(&self, _tree: &Self::Tree) -> Vec<FoldRange> {
        Vec::new()
    }
    /// An indentation hint for a line.
    fn indent(&self, _tree: &Self::Tree, _snap: &Snapshot, _line: usize) -> Option<IndentHint> {
        None
    }
}

/// A counter of pending background work (parse jobs, debounces) for the settled probe. Cloning
/// shares the counter; it is `Send + Sync`.
#[derive(Clone, Debug, Default)]
pub struct PendingWork(Arc<AtomicUsize>);

impl PendingWork {
    /// Starts a piece of work; it ends when the token drops.
    pub fn begin(&self) -> PendingToken {
        self.0.fetch_add(1, Ordering::AcqRel);
        PendingToken(self.0.clone())
    }
    /// How many pieces are pending.
    pub fn count(&self) -> usize {
        self.0.load(Ordering::Acquire)
    }
}

/// One pending piece of work; dropping it ends the work.
#[derive(Debug)]
pub struct PendingToken(Arc<AtomicUsize>);

impl Drop for PendingToken {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Text;

    fn kw() -> TokenStyle {
        TokenStyle::of(TokenType::Keyword)
    }
    fn id() -> TokenStyle {
        TokenStyle::of(TokenType::Variable)
    }

    #[test]
    fn style_packing() {
        let s = TokenStyle::new(
            TokenType::Function,
            TokenModifiers::ASYNC | TokenModifiers::STATIC,
            3,
        );
        assert_eq!(s.ty(), TokenType::Function);
        assert!(s.modifiers().contains(TokenModifiers::ASYNC));
        assert_eq!(s.language(), 3);
        assert_eq!(
            TokenType::from_name("typeParameter"),
            Some(TokenType::TypeParameter)
        );
        assert_eq!(TokenType::Decorator as u8, 23);
    }

    fn store(text: &str, lines: Vec<Vec<Token>>) -> (Text, TokenStore) {
        let t = Text::from_file(text);
        let mut s = TokenStore::new(t.len_lines());
        s.set(TokenRuns {
            first_line: 0,
            lines,
        });
        (t, s)
    }

    #[test]
    fn shift_within_line() {
        // "let x" : kw 0..3, id 4..5
        let (t, mut s) = store(
            "let x",
            vec![vec![
                Token::new(0, kw()),
                Token::new(3, TokenStyle::default()),
                Token::new(4, id()),
            ]],
        );
        let c = ChangeSet::insert_at(5, 4, "yy").unwrap();
        s.shift(&LineEdit::compute(&c, &t));
        assert_eq!(s.style_at(0, 0), kw());
        assert_eq!(s.style_at(0, 6), id());
        assert_eq!(s.invalid_from(), Some(0));
    }

    #[test]
    fn shift_newline_split() {
        let (t, mut s) = store(
            "let x\nfoo",
            vec![
                vec![Token::new(0, kw()), Token::new(4, id())],
                vec![Token::new(0, id())],
            ],
        );
        // Enter after "let"
        let c = ChangeSet::insert_at(9, 3, "\n").unwrap();
        s.shift(&LineEdit::compute(&c, &t));
        assert_eq!(s.len_lines(), 3);
        assert_eq!(s.style_at(0, 0), kw());
        assert_eq!(s.style_at(1, 1), id());
        assert_eq!(s.style_at(2, 0), id());
        // join lines 0 and 1 back
        let t2 = c.apply(&t).unwrap();
        let c2 = ChangeSet::delete_range(10, 3..4).unwrap();
        s.shift(&LineEdit::compute(&c2, &t2));
        assert_eq!(s.len_lines(), 2);
        assert_eq!(s.style_at(0, 4), id());
    }

    #[test]
    fn map_stale_runs() {
        let runs = TokenRuns {
            first_line: 5,
            lines: vec![vec![]; 5],
        };
        let before = LineEdit {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 0,
            inserted_lines: 2,
            inserted_last_len: 0,
            inserted_first_len: 0,
        };
        let r = map_runs(runs.clone(), [&[before][..]].into_iter()).unwrap();
        assert_eq!(r.line_range(), 7..12);
        let inside = LineEdit {
            start_line: 7,
            end_line: 7,
            ..before
        };
        let r = map_runs(runs, [&[inside][..]].into_iter()).unwrap();
        assert_eq!(r.line_range(), 5..7);
    }

    #[test]
    fn pending_work() {
        let p = PendingWork::default();
        let t = p.begin();
        assert_eq!(p.count(), 1);
        drop(t);
        assert_eq!(p.count(), 0);
    }
}
