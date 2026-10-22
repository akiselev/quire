//! Capture name -> packed token style. PROVISIONAL until quire-core's token type list lands.

/// The style that clears a token (`@none`).
pub(crate) const NONE: u32 = 0;

const NAMES: &[&str] = &[
    "comment",
    "string.special",
    "string",
    "number",
    "constant.builtin",
    "constant",
    "keyword",
    "operator",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "punctuation",
    "function.method",
    "function.builtin",
    "function",
    "constructor",
    "type.builtin",
    "type",
    "variable.builtin",
    "variable.parameter",
    "variable",
    "property",
    "attribute",
    "tag",
    "embedded",
    "error",
];

/// The style for a capture name, falling back by dropping trailing `.segments`
/// (`keyword.conditional` -> `keyword`). `None` for private (`_x`) and unknown names.
pub(crate) fn for_capture(name: &str) -> Option<u32> {
    if name.starts_with('_') {
        return None;
    }
    if name == "none" {
        return Some(NONE);
    }
    let mut n = name;
    loop {
        if let Some(i) = NAMES.iter().position(|x| *x == n) {
            return Some(i as u32 + 1);
        }
        n = &n[..n.rfind('.')?];
    }
}

/// The capture name of a style (tests and debugging).
#[allow(dead_code)]
pub(crate) fn name(style: u32) -> &'static str {
    if style == NONE {
        "none"
    } else {
        NAMES.get(style as usize - 1).copied().unwrap_or("?")
    }
}
