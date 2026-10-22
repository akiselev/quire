//! Compiled queries per language, built once per process.

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use crate::lang::Lang;
use crate::queries;
use crate::style;

/// How an injection pattern names its language.
#[derive(Clone, Debug)]
pub(crate) enum InjLang {
    /// `(#set! injection.language "x")`.
    Fixed(String),
    /// The text of the `@injection.language` capture.
    Captured,
}

/// Per-pattern properties of an injection query.
#[derive(Clone, Debug)]
pub(crate) struct InjPattern {
    pub lang: Option<InjLang>,
    pub combined: bool,
    pub include_children: bool,
}

#[derive(Debug)]
pub(crate) struct Injections {
    pub query: Query,
    pub content: u32,
    pub language: Option<u32>,
    pub patterns: Vec<InjPattern>,
}

#[derive(Debug)]
pub(crate) struct Captured {
    pub query: Query,
    /// Capture indices: highlights use one style per capture; folds `@fold`; indents
    /// `@indent`/`@outdent`; brackets `@open`/`@close`.
    pub a: Option<u32>,
    pub b: Option<u32>,
}

/// Everything quire-syntax needs for one language.
#[derive(Debug)]
pub(crate) struct LangConfig {
    pub grammar: Language,
    pub highlights: Query,
    /// Per highlight capture index: `None` = not a highlight (`@_x`, unknown names);
    /// `Some(style)`, where `style::NONE` clears the token (`@none`).
    pub capture_styles: Vec<Option<u32>>,
    pub injections: Option<Injections>,
    pub folds: Option<Captured>,
    pub indents: Option<Captured>,
    pub brackets: Option<Captured>,
}

/// A query that failed to compile (a bug in the shipped queries; the tests compile them all).
#[derive(Clone, Debug)]
pub struct QueryError {
    /// The language.
    pub lang: Lang,
    /// The query kind (`highlights`, `injections`, ...).
    pub kind: &'static str,
    /// tree-sitter's message.
    pub message: String,
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} query: {}", self.lang, self.kind, self.message)
    }
}
impl std::error::Error for QueryError {}

static CONFIGS: [OnceLock<Result<LangConfig, QueryError>>; 5] = [
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
];

/// The compiled configuration, or `None` if the grammar is not compiled in.
pub(crate) fn get(lang: Lang) -> Option<&'static LangConfig> {
    try_get(lang)?.ok()
}

pub(crate) fn try_get(lang: Lang) -> Option<Result<&'static LangConfig, &'static QueryError>> {
    let grammar = lang.grammar()?;
    Some(
        CONFIGS[lang.index()]
            .get_or_init(|| build(lang, grammar))
            .as_ref(),
    )
}

fn compile(
    lang: Lang,
    grammar: &Language,
    kind: &'static str,
) -> Result<Option<Query>, QueryError> {
    let Some(src) = queries::resolve(lang.id(), kind) else {
        return Ok(None);
    };
    Query::new(grammar, &src).map(Some).map_err(|e| QueryError {
        lang,
        kind,
        message: format!("{e}"),
    })
}

fn captured(q: Option<Query>, a: &str, b: &str) -> Option<Captured> {
    q.map(|query| {
        let a = query.capture_index_for_name(a);
        let b = query.capture_index_for_name(b);
        Captured { query, a, b }
    })
}

fn build(lang: Lang, grammar: Language) -> Result<LangConfig, QueryError> {
    let highlights = compile(lang, &grammar, "highlights")?
        .unwrap_or_else(|| Query::new(&grammar, "").expect("the empty query compiles"));
    let capture_styles = highlights
        .capture_names()
        .iter()
        .map(|n| style::for_capture(n))
        .collect();
    let injections = compile(lang, &grammar, "injections")?.map(|query| {
        let content = query
            .capture_index_for_name("injection.content")
            .unwrap_or(u32::MAX);
        let language = query.capture_index_for_name("injection.language");
        let patterns = (0..query.pattern_count())
            .map(|i| {
                let mut p = InjPattern {
                    lang: None,
                    combined: false,
                    include_children: false,
                };
                for prop in query.property_settings(i) {
                    match &*prop.key {
                        "injection.language" => {
                            p.lang = prop.value.as_deref().map(|v| InjLang::Fixed(v.to_owned()))
                        }
                        "injection.combined" => p.combined = true,
                        "injection.include-children" => p.include_children = true,
                        _ => {}
                    }
                }
                if p.lang.is_none() && language.is_some() {
                    p.lang = Some(InjLang::Captured);
                }
                p
            })
            .collect();
        Injections {
            query,
            content,
            language,
            patterns,
        }
    });
    let folds = captured(compile(lang, &grammar, "folds")?, "fold", "");
    let indents = captured(compile(lang, &grammar, "indents")?, "indent", "outdent");
    let brackets = captured(compile(lang, &grammar, "brackets")?, "open", "close");
    Ok(LangConfig {
        grammar,
        highlights,
        capture_styles,
        injections,
        folds,
        indents,
        brackets,
    })
}

/// Compiles every query of every available language; the first error, if any.
pub fn check_queries() -> Result<(), QueryError> {
    for lang in Lang::ALL {
        if let Some(r) = try_get(lang) {
            r.map_err(Clone::clone)?;
        }
    }
    Ok(())
}
