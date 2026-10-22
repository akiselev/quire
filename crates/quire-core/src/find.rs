//! Find and replace: [`FindQuery`] and [`FindState`], matched with `regex-automata` under a
//! compile size limit.
//!
//! Matching joins the text into one string per search (the regex engines need a contiguous
//! haystack); the result is cached per version in [`FindState`]. A plain query is escaped into a
//! regex. "Whole word" uses the configured word separators at both ends of a match (VS Code's
//! `isValidMatch`).

use crate::text::Text;
use crate::version::Version;
use std::ops::Range;

/// The compile size limit for find regexes (bytes of NFA), so a pathological query cannot stall
/// the UI thread.
pub const FIND_REGEX_SIZE_LIMIT: usize = 1 << 20;

/// At most this many matches are collected.
pub const FIND_MATCH_LIMIT: usize = 20_000;

/// A find query.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FindQuery {
    /// The search string (a regex if `is_regex`).
    pub search_string: String,
    /// Treat `search_string` as a regex.
    pub is_regex: bool,
    /// Case-sensitive matching.
    pub match_case: bool,
    /// Only matches bounded by word separators or whitespace.
    pub whole_word: bool,
}

/// Why a query could not be compiled.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FindError(pub String);

impl std::fmt::Display for FindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid find query: {}", self.0)
    }
}
impl std::error::Error for FindError {}

/// A view's find state: the query and its matches at a version.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FindState {
    /// The query.
    pub query: FindQuery,
    /// Matches (byte ranges), valid at `version`.
    pub matches: Vec<Range<usize>>,
    /// The version `matches` belong to.
    pub version: Version,
    /// The matches were mapped forward or the view fell out of the recent-changes window; search
    /// again before trusting them.
    pub stale: bool,
    /// The query failed to compile.
    pub error: Option<FindError>,
}

impl FindState {
    /// Runs `query` over `text` at `version`.
    pub fn new(query: FindQuery, text: &Text, version: Version, word_separators: &str) -> Self {
        let (matches, error) = match find_all(&query, text, word_separators) {
            Ok(m) => (m, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        FindState {
            query,
            matches,
            version,
            stale: false,
            error,
        }
    }
}

impl FindState {
    /// The index of the match equal to `primary`'s range (the "3 of 12" a find bar shows), or
    /// `None` when the primary selection is not a match.
    pub fn current_match(&self, primary: &crate::selection::SelRange) -> Option<usize> {
        let r = primary.range();
        self.matches
            .binary_search_by(|m| (m.start, m.end).cmp(&(r.start, r.end)))
            .ok()
    }
}

fn build_regex(q: &FindQuery) -> Result<regex_automata::meta::Regex, FindError> {
    use regex_automata::meta::Regex;
    use regex_automata::util::syntax;
    let pattern = if q.is_regex {
        q.search_string.clone()
    } else {
        escape(&q.search_string)
    };
    Regex::builder()
        .syntax(
            syntax::Config::new()
                .case_insensitive(!q.match_case)
                .multi_line(true)
                .crlf(false),
        )
        .configure(
            Regex::config()
                .nfa_size_limit(Some(FIND_REGEX_SIZE_LIMIT))
                .hybrid_cache_capacity(2 * FIND_REGEX_SIZE_LIMIT),
        )
        .build(&pattern)
        .map_err(|e| FindError(e.to_string()))
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~ \t\n".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn is_boundary_char(c: Option<char>, seps: &str) -> bool {
    match c {
        None => true,
        Some(c) => c.is_whitespace() || seps.contains(c),
    }
}

/// All matches of `query` in `text` (non-overlapping; zero-length matches such as `^` are kept,
/// as in VS Code, stepping over whole code points), up to [`FIND_MATCH_LIMIT`].
pub fn find_all(
    query: &FindQuery,
    text: &Text,
    word_separators: &str,
) -> Result<Vec<Range<usize>>, FindError> {
    if query.search_string.is_empty() {
        return Ok(Vec::new());
    }
    let re = build_regex(query)?;
    let hay = text.to_string();
    let mut out = Vec::new();
    for m in re.find_iter(hay.as_bytes()) {
        let r = m.range();
        if query.whole_word {
            let before = hay[..r.start].chars().next_back();
            let after = hay[r.end..].chars().next();
            let first = hay[r.start..].chars().next();
            let last = hay[..r.end].chars().next_back();
            let left_ok = is_boundary_char(before, word_separators)
                || is_boundary_char(first, word_separators);
            let right_ok =
                is_boundary_char(after, word_separators) || is_boundary_char(last, word_separators);
            if !(left_ok && right_ok) {
                continue;
            }
        }
        out.push(r);
        if out.len() >= FIND_MATCH_LIMIT {
            break;
        }
    }
    Ok(out)
}

/// Brings `state` up to date after edits by re-searching only the lines that contain `dirty`
/// (byte ranges of the current text that changed since `state.version`; the matches must already
/// be mapped through the edits). Only for plain queries without a line break, whose matches
/// cannot cross a line, so the result equals a full search. Returns `false` (and changes nothing)
/// when the query does not qualify or the match limit was reached; search everything then.
pub(crate) fn refresh_lines(
    state: &mut FindState,
    text: &Text,
    dirty: &[Range<usize>],
    version: Version,
    word_separators: &str,
) -> bool {
    let q = &state.query;
    if q.is_regex
        || q.search_string.is_empty()
        || q.search_string.contains('\n')
        || state.error.is_some()
        || state.matches.len() >= FIND_MATCH_LIMIT
    {
        return false;
    }
    let len = text.len_bytes();
    // Whole lines around each dirty range, merged.
    let mut regions: Vec<Range<usize>> = Vec::new();
    for d in dirty {
        let a = text.line_to_byte(text.byte_to_line(d.start.min(len)));
        let b = text.line_range(text.byte_to_line(d.end.min(len))).end;
        match regions.last_mut() {
            Some(r) if a <= r.end + 1 => r.end = r.end.max(b),
            _ => regions.push(a..b),
        }
    }
    regions.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for r in regions {
        match merged.last_mut() {
            Some(m) if r.start <= m.end + 1 => m.end = m.end.max(r.end),
            _ => merged.push(r),
        }
    }
    let Ok(re) = build_regex(q) else {
        return false;
    };
    let mut out: Vec<Range<usize>> = Vec::with_capacity(state.matches.len());
    let mut old = std::mem::take(&mut state.matches).into_iter().peekable();
    for region in &merged {
        // Keep the mapped matches before the region; drop those touching it.
        while let Some(m) = old.peek() {
            if m.end < region.start || (m.end == region.start && m.start < m.end) {
                out.push(old.next().expect("peeked"));
            } else if m.start <= region.end {
                old.next();
            } else {
                break;
            }
        }
        let hay = text.slice(region.clone()).to_string();
        for m in re.find_iter(hay.as_bytes()) {
            let r = m.range();
            if q.whole_word {
                // A region is whole lines, so its edges are line edges (boundaries) in the full
                // text too.
                let before = hay[..r.start].chars().next_back();
                let after = hay[r.end..].chars().next();
                let first = hay[r.start..].chars().next();
                let last = hay[..r.end].chars().next_back();
                let left_ok = is_boundary_char(before, word_separators)
                    || is_boundary_char(first, word_separators);
                let right_ok = is_boundary_char(after, word_separators)
                    || is_boundary_char(last, word_separators);
                if !(left_ok && right_ok) {
                    continue;
                }
            }
            out.push(region.start + r.start..region.start + r.end);
        }
    }
    out.extend(old);
    out.truncate(FIND_MATCH_LIMIT);
    state.matches = out;
    state.version = version;
    state.stale = false;
    true
}

/// Expands `$0`..`$9` and `$$` in a regex replacement for one match. Plain queries use the
/// replacement literally.
pub fn expand_replacement(
    query: &FindQuery,
    text: &Text,
    m: Range<usize>,
    replacement: &str,
) -> Result<String, FindError> {
    if !query.is_regex {
        return Ok(replacement.to_string());
    }
    let re = build_regex(query)?;
    let hay = text.to_string();
    let mut caps = re.create_captures();
    let input = regex_automata::Input::new(hay.as_bytes())
        .range(m.start..)
        .anchored(regex_automata::Anchored::Yes);
    re.search_captures(&input, &mut caps);
    let mut out = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            match chars.peek() {
                Some('$') => {
                    chars.next();
                    out.push('$');
                }
                Some(d) if d.is_ascii_digit() => {
                    let g = d.to_digit(10).unwrap_or(0) as usize;
                    chars.next();
                    if let Some(span) = caps.get_group(g) {
                        out.push_str(&hay[span.range()]);
                    }
                }
                _ => out.push('$'),
            }
        } else if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> FindQuery {
        FindQuery {
            search_string: s.into(),
            ..Default::default()
        }
    }

    #[test]
    fn plain_case_insensitive() {
        let t = Text::from_file("Foo foo (foo)");
        assert_eq!(
            find_all(&q("foo"), &t, "()").unwrap(),
            vec![0..3, 4..7, 9..12]
        );
        let mut cs = q("foo");
        cs.match_case = true;
        assert_eq!(find_all(&cs, &t, "()").unwrap(), vec![4..7, 9..12]);
        assert_eq!(find_all(&q("("), &t, "").unwrap(), vec![8..9]);
    }

    #[test]
    fn whole_word_and_regex() {
        let t = Text::from_file("foo foobar bar.foo");
        let mut w = q("foo");
        w.whole_word = true;
        assert_eq!(find_all(&w, &t, ".").unwrap(), vec![0..3, 15..18]);
        let mut r = q("b(a)r");
        r.is_regex = true;
        let m = find_all(&r, &t, "").unwrap();
        assert_eq!(m, vec![7..10, 11..14]);
        assert_eq!(expand_replacement(&r, &t, 7..10, "<$1$$>").unwrap(), "<a$>");
        let st = FindState::new(r.clone(), &t, Version(0), "");
        assert_eq!(st.current_match(&crate::SelRange::new(11, 14)), Some(1));
        assert_eq!(st.current_match(&crate::SelRange::new(14, 11)), Some(1));
        assert_eq!(st.current_match(&crate::SelRange::point(11)), None);
        let mut bad = q("(");
        bad.is_regex = true;
        assert!(find_all(&bad, &t, "").is_err());
    }
}
