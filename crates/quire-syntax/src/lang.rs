//! The languages quire-syntax knows, and their tree-sitter grammars (one Cargo feature each).

/// A language with a grammar in this crate. Whether its grammar is compiled in depends on the
/// crate features; [`Lang::is_available`] says.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Lang {
    /// Svelte components (`tree-sitter-htmlx-svelte`), with injected script, style and expressions.
    Svelte,
    /// TypeScript (`tree-sitter-typescript`, the `typescript` dialect).
    TypeScript,
    /// TypeScript with JSX (`tree-sitter-typescript`, the `tsx` dialect).
    Tsx,
    /// JavaScript with JSX (`tree-sitter-javascript`).
    JavaScript,
    /// CSS (`tree-sitter-css`).
    Css,
}

impl Lang {
    /// Every language, in a fixed order (also the index order of per-language tables).
    pub const ALL: [Lang; 5] = [Lang::Svelte, Lang::TypeScript, Lang::Tsx, Lang::JavaScript, Lang::Css];

    /// The stable id: the query directory name and the name injections use.
    pub fn id(self) -> &'static str {
        match self {
            Lang::Svelte => "svelte",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::JavaScript => "javascript",
            Lang::Css => "css",
        }
    }

    /// Looks a language up by id or common alias (`ts`, `js`, `jsx`, `mjs`, ...), ASCII
    /// case-insensitively, as `<script lang="...">` and tagged templates name them.
    pub fn from_name(name: &str) -> Option<Lang> {
        let n = name.trim().to_ascii_lowercase();
        Some(match n.as_str() {
            "svelte" => Lang::Svelte,
            "ts" | "typescript" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "js" | "javascript" | "jsx" | "mjs" | "cjs" | "ecmascript" => Lang::JavaScript,
            "css" => Lang::Css,
            _ => return None,
        })
    }

    /// Looks a language up by file extension (without the dot).
    pub fn from_extension(ext: &str) -> Option<Lang> {
        Lang::from_name(ext)
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }

    /// Whether this language's grammar is compiled in (its Cargo feature is on).
    pub fn is_available(self) -> bool {
        self.grammar().is_some()
    }

    /// The tree-sitter grammar, if its feature is on.
    pub(crate) fn grammar(self) -> Option<tree_sitter::Language> {
        match self {
            #[cfg(feature = "svelte")]
            Lang::Svelte => Some(tree_sitter_htmlx_svelte::LANGUAGE.into()),
            #[cfg(feature = "typescript")]
            Lang::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            #[cfg(feature = "typescript")]
            Lang::Tsx => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
            #[cfg(feature = "javascript")]
            Lang::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            #[cfg(feature = "css")]
            Lang::Css => Some(tree_sitter_css::LANGUAGE.into()),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}
