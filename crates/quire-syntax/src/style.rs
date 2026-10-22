//! Capture name -> quire-core token style.

use quire_core::syntax::{TokenModifiers, TokenStyle, TokenType};

/// The style that clears a token (`@none`): plain text.
pub(crate) const NONE: u32 = 0;

/// Capture names (and prefixes) to token types. Lookup drops trailing `.segments` until a name
/// matches (`keyword.conditional` -> `keyword`).
const TABLE: &[(&str, TokenType)] = &[
    ("comment", TokenType::Comment),
    ("string.special", TokenType::StringSpecial),
    ("string.escape", TokenType::StringEscape),
    ("constant.character.escape", TokenType::StringEscape),
    ("string.regexp", TokenType::Regexp),
    ("string.regex", TokenType::Regexp),
    ("string", TokenType::String),
    ("number", TokenType::Number),
    ("float", TokenType::Number),
    ("boolean", TokenType::Boolean),
    ("constant", TokenType::Constant),
    ("keyword", TokenType::Keyword),
    ("type.qualifier", TokenType::Modifier),
    ("operator", TokenType::Operator),
    ("punctuation.bracket", TokenType::PunctuationBracket),
    ("punctuation.delimiter", TokenType::PunctuationDelimiter),
    ("punctuation", TokenType::Punctuation),
    ("tag.delimiter", TokenType::PunctuationBracket),
    ("tag.error", TokenType::Error),
    ("tag", TokenType::Tag),
    ("attribute", TokenType::Attribute),
    ("property", TokenType::Property),
    ("function.method", TokenType::Method),
    ("function", TokenType::Function),
    ("method", TokenType::Method),
    ("constructor", TokenType::Constructor),
    ("type", TokenType::Type),
    ("variable.parameter", TokenType::Parameter),
    ("parameter", TokenType::Parameter),
    ("variable.member", TokenType::Property),
    ("variable", TokenType::Variable),
    ("namespace", TokenType::Namespace),
    ("module", TokenType::Namespace),
    ("label", TokenType::Label),
    ("decorator", TokenType::Decorator),
    ("embedded", TokenType::Embedded),
    ("error", TokenType::Error),
    ("text", TokenType::Text),
];

/// The packed style for a capture name; `None` for private (`_x`) and unknown names.
/// `*.builtin` captures get the `defaultLibrary` modifier.
pub(crate) fn for_capture(name: &str) -> Option<u32> {
    if name.starts_with('_') {
        return None;
    }
    if name == "none" {
        return Some(NONE);
    }
    let builtin = name.split('.').any(|s| s == "builtin");
    let mut n = name;
    loop {
        if let Some((_, ty)) = TABLE.iter().find(|(k, _)| *k == n) {
            let m = if builtin {
                TokenModifiers::DEFAULT_LIBRARY
            } else {
                TokenModifiers::NONE
            };
            return Some(TokenStyle::new(*ty, m, 0).0);
        }
        n = &n[..n.rfind('.')?];
    }
}

/// A readable name for a style (fixtures and debugging): `type` or `type+lib`.
pub fn name(style: u32) -> String {
    let s = TokenStyle(style);
    let mut out = s.ty().name().to_owned();
    if s.modifiers().contains(TokenModifiers::DEFAULT_LIBRARY) {
        out.push_str("+lib");
    }
    out
}
