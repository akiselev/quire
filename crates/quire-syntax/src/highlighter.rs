//! [`TreeSitterHighlighter`]: quire-core's [`Highlighter`] over the layered tree-sitter engine.

use std::ops::Range;

use quire_core::syntax::{
    BracketPair, CancelFlag, FoldKind, FoldRange, Highlighter, IndentHint, Token, TokenRuns,
    TokenStyle,
};
use quire_core::{ChangeSet, Snapshot, Text};

use crate::highlight;
use crate::lang::Lang;
use crate::tree::{self, EditRef, Layers, ParseStats, Prev};

/// A tree-sitter highlighter for one document language; injected languages (Svelte's script,
/// style and expressions, CSS in tagged templates) come with it.
#[derive(Clone, Debug)]
pub struct TreeSitterHighlighter {
    lang: Lang,
}

impl TreeSitterHighlighter {
    /// The highlighter for `lang`, or `None` if its grammar feature is off or its queries do
    /// not compile (a bug the crate's tests catch).
    pub fn new(lang: Lang) -> Option<Self> {
        crate::config::get(lang).map(|_| TreeSitterHighlighter { lang })
    }

    /// The highlighter for a language name or alias (`svelte`, `ts`, `typescript`, `tsx`, `js`,
    /// `javascript`, `css`, ...).
    pub fn for_name(name: &str) -> Option<Self> {
        Lang::from_name(name).and_then(Self::new)
    }

    /// The document language.
    pub fn lang(&self) -> Lang {
        self.lang
    }
}

/// The parse result: one tree per language layer, and the text they were parsed from.
#[derive(Clone, Debug)]
pub struct SyntaxTree {
    layers: Layers,
    text: Text,
    stats: ParseStats,
}

impl SyntaxTree {
    /// What the parse that made this tree did.
    pub fn stats(&self) -> ParseStats {
        self.stats
    }

    /// The text this tree was parsed from.
    pub fn text(&self) -> &Text {
        &self.text
    }

    /// The injected layers: language and byte span, sorted by start (the root excluded).
    pub fn injections(&self) -> Vec<(Lang, Range<usize>)> {
        let mut v: Vec<_> = self
            .layers
            .layers
            .iter()
            .skip(1)
            .map(|l| (l.lang, l.span.clone()))
            .collect();
        v.sort_by_key(|(_, r)| (r.start, r.end));
        v
    }

    /// The root layer's tree as an S-expression (tests and debugging).
    #[doc(hidden)]
    pub fn root_sexp(&self) -> String {
        self.layers.layers[0].data.tree.root_node().to_sexp()
    }

    /// Every layer's language, span and tree S-expression, sorted by span (tests and
    /// debugging; injected trees' positions may be lazily shifted, so only the shape is
    /// comparable).
    #[doc(hidden)]
    pub fn layer_sexps(&self) -> Vec<(Lang, Range<usize>, String)> {
        let mut v: Vec<_> = self
            .layers
            .layers
            .iter()
            .map(|l| (l.lang, l.span.clone(), l.data.tree.root_node().to_sexp()))
            .collect();
        v.sort_by_key(|a| (a.1.start, a.1.end));
        v
    }

    /// Whether any layer's tree contains a syntax error.
    pub fn has_error(&self) -> bool {
        self.layers
            .layers
            .iter()
            .any(|l| l.data.tree.root_node().has_error())
    }

    fn byte_range(&self, lines: Range<usize>) -> Range<usize> {
        let n = self.text.len_lines();
        let s = self.text.line_to_byte(lines.start.min(n));
        let e = if lines.end >= n {
            self.text.len_bytes()
        } else {
            self.text.line_to_byte(lines.end)
        };
        s..e.max(s)
    }
}

impl Highlighter for TreeSitterHighlighter {
    type Tree = SyntaxTree;

    fn language(&self) -> &str {
        self.lang.id()
    }

    fn parse(
        &self,
        snap: &Snapshot,
        prev: Option<(&SyntaxTree, &ChangeSet)>,
        cancel: &CancelFlag,
    ) -> Option<SyntaxTree> {
        let text = &snap.text;
        let cancelled = || cancel.is_cancelled();
        // A previous tree is only usable if the changes lead from its text to this one.
        let prev = prev
            .filter(|(t, c)| c.len() == t.text.len_bytes() && c.len_after() == text.len_bytes());
        let (layers, stats) = match prev {
            Some((old, changes)) => {
                let edits: Vec<EditRef<'_>> = changes
                    .edits()
                    .into_iter()
                    .map(|e| EditRef {
                        range: e.range,
                        insert: e.insert,
                    })
                    .collect();
                let input = tree::input_edits(&old.text, text, &edits);
                tree::parse(
                    self.lang,
                    text,
                    Some(Prev {
                        layers: &old.layers,
                        edits: &edits,
                        input_edits: &input,
                    }),
                    &cancelled,
                )?
            }
            None => tree::parse(self.lang, text, None, &cancelled)?,
        };
        Some(SyntaxTree {
            layers,
            text: text.clone(),
            stats,
        })
    }

    /// Tokens for `lines` of the tree's own text (`snap` should be the snapshot it was parsed
    /// from; positions are only valid with that version).
    fn highlights(&self, tree: &SyntaxTree, _snap: &Snapshot, lines: Range<usize>) -> TokenRuns {
        let first_line = lines.start.min(tree.text.len_lines());
        let runs = highlight::highlight(&tree.layers, &tree.text, lines);
        TokenRuns {
            first_line,
            lines: runs
                .into_iter()
                .map(|l| {
                    l.into_iter()
                        .map(|(start, st)| Token::new(start, TokenStyle(st)))
                        .collect()
                })
                .collect(),
        }
    }

    fn brackets(&self, tree: &SyntaxTree, lines: Range<usize>) -> Vec<BracketPair> {
        highlight::brackets(&tree.layers, tree.byte_range(lines))
            .into_iter()
            .map(|b| BracketPair {
                open: b.open,
                close: b.close,
            })
            .collect()
    }

    fn folds(&self, tree: &SyntaxTree) -> Vec<FoldRange> {
        highlight::folds(&tree.layers)
            .into_iter()
            .map(|f| FoldRange {
                start_line: f.start,
                end_line: f.end,
                kind: if f.comment {
                    FoldKind::Comment
                } else {
                    FoldKind::Other
                },
            })
            .collect()
    }

    /// The indent of `line` relative to the line above, from the indent queries' levels.
    fn indent(&self, tree: &SyntaxTree, _snap: &Snapshot, line: usize) -> Option<IndentHint> {
        let here = highlight::indent(&tree.layers, &tree.text, line)?;
        if line == 0 {
            return Some(if here > 0 {
                IndentHint::Indent
            } else {
                IndentHint::Keep
            });
        }
        let above = highlight::indent(&tree.layers, &tree.text, line - 1)?;
        Some(match here.cmp(&above) {
            std::cmp::Ordering::Greater => IndentHint::Indent,
            std::cmp::Ordering::Less => IndentHint::Outdent,
            std::cmp::Ordering::Equal => IndentHint::Keep,
        })
    }
}

impl TreeSitterHighlighter {
    /// The absolute indent level of `line` (number of indent units), from the indent queries.
    pub fn indent_level(&self, tree: &SyntaxTree, line: usize) -> Option<u32> {
        highlight::indent(&tree.layers, &tree.text, line)
    }
}
