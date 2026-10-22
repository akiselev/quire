//! Language rules: `onEnterRules`, `indentationRules` and bracket electric characters.
//!
//! Translated from Visual Studio Code (microsoft/vscode @ bdce63db), files
//! `src/vs/editor/common/languages/supports/onEnter.ts`, `supports/indentRules.ts`,
//! `enterAction.ts`, `autoIndent.ts` and `supports/electricCharacter.ts`.
//!
//! Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see THIRD_PARTY_NOTICES.md for the full text).
//!
//! Differences: quire has no tokens, so the "processed" lines VS Code gets by blanking brackets in
//! strings, comments and regexes are the raw lines, and there are no embedded languages. Lines are
//! 0-based here.
//!
//! # Untrusted regexes
//!
//! Rule regexes can come from extensions, so they must not hang the editor (user, 2026-10-22:
//! fancy-regex is accepted with a low backtrack limit).
//! - **Backtrack limit 20,000** per match call (fancy-regex's default is 1,000,000). Patterns
//!   without lookaround or backreferences run on regex-automata and never backtrack; the limit
//!   bounds the rest. Measured 2026-10-22 over the 99 distinct rule regexes in our fixtures,
//!   every VS Code built-in `language-configuration.json` (1.105 on macOS), svelte-vscode's
//!   and rust-analyzer's rules, against 26,148 lines of Rust, TS/JS, Svelte, Python, C headers,
//!   CSS and HTML, their halves (text before a caret) and 40 joined lines of 1,000 to 1,545 bytes:
//!   on lines up to 400 bytes the JS/TS, CSS, HTML, Svelte, Rust, Python and C-like rules need at
//!   most 6,100 steps (Ruby 12,000), on the long lines at most 23,300 (one C-like test rule; JS,
//!   HTML and Svelte under 8,000). Julia's `increaseIndentPattern` is the outlier (40,000 on a
//!   200-byte line). 20,000 keeps a 3x margin over the common languages on ordinary lines and costs
//!   about 1 ms per limit hit in a release build (11 ms in debug). With it every case answers as
//!   JavaScript does, except long Julia, Ruby and C-like-test lines, which become "no match".
//! - A match that hits the limit is **no match**, per call. After `DISABLE_AFTER_HITS` (8) hits a
//!   pattern is no longer run, which bounds a command that tests many lines (re-indent); a text that
//!   just hit the limit is not retried with the same pattern.
//! - Compile size: each delegated regex-automata program and its lazy DFA cache are capped at 1 MiB
//!   (`delegate_size_limit`, `delegate_dfa_size_limit`); a larger pattern does not compile and
//!   never matches.

use crate::commands::model::{next_indent_tab_stop, prev_indent_tab_stop, visible_col};
use crate::config::{EditorConfig, EnterAction, IndentAction};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

// ---------------------------------------------------------------------------------------------
// regexes

type Re = Arc<Rule>;

const BACKTRACK_LIMIT: usize = 20_000;
const DELEGATE_SIZE_LIMIT: usize = 1 << 20;
const DISABLE_AFTER_HITS: u32 = 8;

/// Rewrites a JavaScript regex source into fancy-regex syntax:
/// - a lone `{` or `}` that is not part of a quantifier is a literal in JavaScript;
/// - inside a class, `[`, doubled `&`, `-`, `~`, and a `-` next to a class escape (`[\w-.]`) are
///   literals;
/// - `[]` never matches and `[^]` matches any character;
/// - a backreference to a group that has not matched matches the empty string (`\1` becomes
///   `(?(1)\1|)`).
fn js_to_rust(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 8);
    let mut i = 0;
    let mut in_class = false;
    // Inside a class: the previous item was a class escape such as `\w`.
    let mut after_class_escape = false;
    let is_class_escape =
        |j: usize| c.get(j) == Some(&'\\') && c.get(j + 1).is_some_and(|n| "wWdDsS".contains(*n));
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' {
            let n = c.get(i + 1).copied();
            if !in_class && n.is_some_and(|n| ('1'..='9').contains(&n)) {
                let mut j = i + 1;
                while j < c.len() && c[j].is_ascii_digit() {
                    j += 1;
                }
                let num: String = c[i + 1..j].iter().collect();
                out.push_str(&format!("(?({num})\\{num}|)"));
                i = j;
                continue;
            }
            after_class_escape = in_class && is_class_escape(i);
            out.push(ch);
            if let Some(n) = n {
                // `\/` is a JavaScript-only escape.
                if n == '/' {
                    out.pop();
                }
                out.push(n);
            }
            i += 2;
            continue;
        }
        if in_class {
            let prev_escape = std::mem::take(&mut after_class_escape);
            match ch {
                ']' => in_class = false,
                '[' => {
                    out.push_str("\\[");
                    i += 1;
                    continue;
                }
                '-' if prev_escape || is_class_escape(i + 1) => {
                    out.push_str("\\-");
                    i += 1;
                    continue;
                }
                '&' | '-' | '~' if c.get(i + 1) == Some(&ch) => {
                    out.push('\\');
                    out.push(ch);
                    i += 1;
                    continue;
                }
                _ => {}
            }
            out.push(ch);
            i += 1;
            continue;
        }
        match ch {
            '[' => {
                // `[]` matches nothing and `[^]` anything in JavaScript.
                if c.get(i + 1) == Some(&']') {
                    out.push_str("[^\\s\\S]");
                    i += 2;
                    continue;
                }
                if c.get(i + 1) == Some(&'^') && c.get(i + 2) == Some(&']') {
                    out.push_str("[\\s\\S]");
                    i += 3;
                    continue;
                }
                in_class = true;
                out.push(ch);
                if c.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
            }
            '{' => {
                // A quantifier is `{n}`, `{n,}` or `{n,m}` after something quantifiable.
                let mut j = i + 1;
                let mut digits = 0;
                while j < c.len() && c[j].is_ascii_digit() {
                    j += 1;
                    digits += 1;
                }
                let mut ok = digits > 0;
                if ok && c.get(j) == Some(&',') {
                    j += 1;
                    while j < c.len() && c[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                ok = ok && c.get(j) == Some(&'}') && !out.is_empty();
                if ok {
                    out.extend(&c[i..=j]);
                    i = j + 1;
                    continue;
                }
                out.push_str("\\{");
            }
            '}' => out.push_str("\\}"),
            _ => out.push(ch),
        }
        i += 1;
    }
    out
}

fn build(src: &str, backtrack_limit: usize) -> Option<fancy_regex::Regex> {
    fancy_regex::RegexBuilder::new(&js_to_rust(src))
        .backtrack_limit(backtrack_limit)
        .delegate_size_limit(DELEGATE_SIZE_LIMIT)
        .delegate_dfa_size_limit(DELEGATE_SIZE_LIMIT)
        .build()
        .ok()
}

/// A compiled rule regex and how many matches ran out of backtracking.
pub(crate) struct Rule {
    re: fancy_regex::Regex,
    hits: AtomicU32,
    /// Hash of the last text that hit the limit (one command often tests a line several times).
    last_hit: AtomicU64,
}

/// Compiles a language-rule regex (cached; `None` if it does not compile).
pub(crate) fn regex(src: &str) -> Option<Re> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Re>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = map.get(src) {
        return r.clone();
    }
    if map.len() > 512 {
        map.clear();
    }
    let re = build(src, BACKTRACK_LIMIT).map(|re| {
        Arc::new(Rule {
            re,
            hits: AtomicU32::new(0),
            last_hit: AtomicU64::new(0),
        })
    });
    map.insert(src.to_string(), re.clone());
    re
}

/// JavaScript `RegExp.test`. A pattern that does not compile never matches; a match that runs out
/// of backtracking is no match, and after `DISABLE_AFTER_HITS` such matches the pattern is not run
/// again (until the cache drops it).
pub(crate) fn test(src: &str, text: &str) -> bool {
    let Some(r) = regex(src) else {
        return false;
    };
    let hash = |t: &str| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        t.hash(&mut h);
        h.finish() | 1
    };
    if r.hits.load(Ordering::Relaxed) >= DISABLE_AFTER_HITS
        || (r.hits.load(Ordering::Relaxed) > 0 && r.last_hit.load(Ordering::Relaxed) == hash(text))
    {
        return false;
    }
    r.re.is_match(text).unwrap_or_else(|_| {
        r.hits.fetch_add(1, Ordering::Relaxed);
        r.last_hit.store(hash(text), Ordering::Relaxed);
        false
    })
}

// ---------------------------------------------------------------------------------------------
// indentation helpers

/// VS Code `strings.getLeadingWhitespace`.
pub(crate) fn leading_ws(s: &str) -> &str {
    let end = s.find(|c| c != ' ' && c != '\t').unwrap_or(s.len());
    &s[..end]
}

fn is_blank(s: &str) -> bool {
    s.chars().all(char::is_whitespace)
}

fn indent_vis(cfg: &EditorConfig, indentation: &str) -> u32 {
    let chars: Vec<char> = indentation.chars().collect();
    visible_col(&chars, chars.len(), cfg.tab_size)
}

/// `shiftIndent(config, indentation)` (`ShiftCommand.shiftIndent` at the end of `indentation`).
pub(crate) fn shift_indent(cfg: &EditorConfig, indentation: &str) -> String {
    let vis = indent_vis(cfg, indentation);
    if cfg.insert_spaces {
        " ".repeat(next_indent_tab_stop(vis, cfg.indent_size.max(1)) as usize)
    } else {
        let t = cfg.tab_size.max(1);
        "\t".repeat((next_indent_tab_stop(vis, t) / t) as usize)
    }
}

/// `unshiftIndent(config, indentation)`.
pub(crate) fn unshift_indent(cfg: &EditorConfig, indentation: &str) -> String {
    let vis = indent_vis(cfg, indentation);
    if cfg.insert_spaces {
        " ".repeat(prev_indent_tab_stop(vis, cfg.indent_size.max(1)) as usize)
    } else {
        let t = cfg.tab_size.max(1);
        "\t".repeat((prev_indent_tab_stop(vis, t) / t) as usize)
    }
}

// ---------------------------------------------------------------------------------------------
// onEnter (OnEnterSupport)

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `_createOpenBracketRegExp(open).test(text)`: `\b?open\s*$`.
fn open_bracket_matches(open: &str, text: &str) -> bool {
    let t = text.trim_end();
    if open.is_empty() || !t.ends_with(open) {
        return false;
    }
    if open.chars().next().is_some_and(is_word) {
        let before = &t[..t.len() - open.len()];
        return !before.chars().next_back().is_some_and(is_word);
    }
    true
}

/// `_createCloseBracketRegExp(close).test(text)`: `^\s*close\b?`.
fn close_bracket_matches(close: &str, text: &str) -> bool {
    let t = text.trim_start();
    if close.is_empty() || !t.starts_with(close) {
        return false;
    }
    if close.chars().next_back().is_some_and(is_word) {
        return !t[close.len()..].chars().next().is_some_and(is_word);
    }
    true
}

/// `OnEnterSupport.onEnter` (autoIndent is `full` when `cfg.auto_indent`).
pub(crate) fn on_enter(
    cfg: &EditorConfig,
    previous_line: &str,
    before: &str,
    after: &str,
) -> Option<EnterAction> {
    if !cfg.auto_indent {
        return None;
    }
    // (1) regex rules
    for rule in &cfg.on_enter_rules {
        let ok = test(&rule.before_text, before)
            && rule.after_text.as_deref().is_none_or(|r| test(r, after))
            && rule
                .previous_line_text
                .as_deref()
                .is_none_or(|r| test(r, previous_line));
        if ok {
            return Some(rule.action.clone());
        }
    }
    // (2) indent-outdent between brackets
    if !before.is_empty() && !after.is_empty() {
        for (open, close) in &cfg.brackets {
            if open_bracket_matches(open, before) && close_bracket_matches(close, after) {
                return Some(EnterAction {
                    indent: IndentAction::IndentOutdent,
                    ..Default::default()
                });
            }
        }
    }
    // (4) after an open bracket
    if !before.is_empty() {
        for (open, _) in &cfg.brackets {
            if open_bracket_matches(open, before) {
                return Some(EnterAction {
                    indent: IndentAction::Indent,
                    ..Default::default()
                });
            }
        }
    }
    None
}

/// VS Code `CompleteEnterAction`.
#[derive(Debug)]
pub(crate) struct CompleteEnterAction {
    pub indent: IndentAction,
    pub append_text: String,
    pub remove_text: usize,
    pub indentation: String,
}

/// `getEnterAction` for a range given as (line, char column) start and end positions.
pub(crate) fn enter_action(
    cfg: &EditorConfig,
    lines: &Lines<'_>,
    start: (usize, usize),
    end: (usize, usize),
) -> Option<CompleteEnterAction> {
    let start_line = lines.get(start.0);
    let end_line = if end.0 == start.0 {
        start_line.clone()
    } else {
        lines.get(end.0)
    };
    let previous = if start.0 == 0 {
        String::new()
    } else {
        lines.get(start.0 - 1)
    };
    let before: String = start_line.chars().take(start.1).collect();
    let after: String = end_line.chars().skip(end.1).collect();
    let r = on_enter(cfg, &previous, &before, &after)?;
    let remove_text = r.remove_text.unwrap_or(0);
    let indents = matches!(r.indent, IndentAction::Indent | IndentAction::IndentOutdent);
    let append_text = match r.append_text.filter(|a| !a.is_empty()) {
        None if indents => "\t".to_string(),
        None => String::new(),
        Some(a) if r.indent == IndentAction::Indent => format!("\t{a}"),
        Some(a) => a,
    };
    let mut indentation: String = leading_ws(&start_line).chars().take(start.1).collect();
    if remove_text > 0 {
        let n = indentation.chars().count().saturating_sub(remove_text);
        indentation = indentation.chars().take(n).collect();
    }
    Some(CompleteEnterAction {
        indent: r.indent,
        append_text,
        remove_text,
        indentation,
    })
}

// ---------------------------------------------------------------------------------------------
// indentation rules (IndentRulesSupport, autoIndent.ts)

/// A virtual model: the document's lines, with one line optionally replaced.
pub(crate) struct Lines<'a> {
    pub text: &'a crate::text::Text,
    pub patch: Option<(usize, String)>,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a crate::text::Text) -> Self {
        Lines { text, patch: None }
    }

    pub fn get(&self, line: usize) -> String {
        if let Some((l, s)) = &self.patch
            && *l == line
        {
            return s.clone();
        }
        self.text.line(line).to_string()
    }
}

pub(crate) fn should_increase(cfg: &EditorConfig, text: &str) -> bool {
    cfg.indentation_rules
        .as_ref()
        .is_some_and(|r| test(&r.increase, text))
}

pub(crate) fn should_decrease(cfg: &EditorConfig, text: &str) -> bool {
    cfg.indentation_rules
        .as_ref()
        .is_some_and(|r| test(&r.decrease, text))
}

pub(crate) fn should_indent_next_line(cfg: &EditorConfig, text: &str) -> bool {
    cfg.indentation_rules
        .as_ref()
        .and_then(|r| r.indent_next_line.as_deref())
        .is_some_and(|p| test(p, text))
}

pub(crate) fn should_ignore(cfg: &EditorConfig, text: &str) -> bool {
    cfg.indentation_rules
        .as_ref()
        .and_then(|r| r.unindented_line.as_deref())
        .is_some_and(|p| test(p, text))
}

/// Whether indentation rules are active (autoIndent `full` with rules).
pub(crate) fn has_indent_rules(cfg: &EditorConfig) -> bool {
    cfg.auto_indent && cfg.indentation_rules.is_some()
}

/// `getPrecedingValidLine`: the nearest line above that is neither blank nor ignored; `Err` (VS
/// Code's -1) when there is none.
fn preceding_valid_line(cfg: &EditorConfig, m: &Lines<'_>, line: usize) -> Result<usize, ()> {
    if line > 0 {
        let mut l = line;
        while l > 0 {
            l -= 1;
            let text = m.get(l);
            if should_ignore(cfg, &text) || is_blank(&text) {
                continue;
            }
            return Ok(l);
        }
    }
    Err(())
}

/// The result of `getInheritIndentForLine`.
#[derive(Debug)]
pub(crate) struct Inherit {
    pub indentation: String,
    pub indent: bool,
    #[allow(dead_code)] // read by `good_indent_for_line`
    pub line: Option<usize>,
}

/// `getInheritIndentForLine`.
pub(crate) fn inherit_indent_for_line(
    cfg: &EditorConfig,
    m: &Lines<'_>,
    line: usize,
    honor_intentional: bool,
) -> Option<Inherit> {
    if !has_indent_rules(cfg) {
        return None;
    }
    let none = || Inherit {
        indentation: String::new(),
        indent: false,
        line: None,
    };
    if line == 0 {
        return Some(none());
    }
    // No indent if this is the first non-blank line.
    let mut p = line;
    while p > 0 {
        p -= 1;
        if !m.get(p).is_empty() {
            break;
        }
        if p == 0 {
            return Some(none());
        }
    }
    // Every line above blank or ignored: VS Code's -1, no inherited indentation.
    let preceding = preceding_valid_line(cfg, m, line).ok()?;
    let pre_text = m.get(preceding);
    if should_increase(cfg, &pre_text) || should_indent_next_line(cfg, &pre_text) {
        return Some(Inherit {
            indentation: leading_ws(&pre_text).to_string(),
            indent: true,
            line: Some(preceding),
        });
    }
    if should_decrease(cfg, &pre_text) {
        return Some(Inherit {
            indentation: leading_ws(&pre_text).to_string(),
            indent: false,
            line: Some(preceding),
        });
    }
    if preceding == 0 {
        return Some(Inherit {
            indentation: leading_ws(&pre_text).to_string(),
            indent: false,
            line: Some(0),
        });
    }
    let previous = preceding - 1;
    let prev_text = m.get(previous);
    if !(should_increase(cfg, &prev_text) || should_decrease(cfg, &prev_text))
        && should_indent_next_line(cfg, &prev_text)
    {
        // The first line of a run of indent-next-line lines ending at `previous`.
        let mut stop: Option<usize> = None;
        let mut i = previous;
        while i > 0 {
            i -= 1;
            if should_indent_next_line(cfg, &m.get(i)) {
                continue;
            }
            stop = Some(i);
            break;
        }
        let l = stop.map_or(0, |s| s + 1);
        return Some(Inherit {
            indentation: leading_ws(&m.get(l)).to_string(),
            indent: false,
            line: Some(l),
        });
    }
    if honor_intentional {
        return Some(Inherit {
            indentation: leading_ws(&pre_text).to_string(),
            indent: false,
            line: Some(preceding),
        });
    }
    // Search upward from `preceding` for a line whose indentation is not temporary.
    let mut i = preceding + 1;
    while i > 0 {
        i -= 1;
        let t = m.get(i);
        if should_increase(cfg, &t) {
            return Some(Inherit {
                indentation: leading_ws(&t).to_string(),
                indent: true,
                line: Some(i),
            });
        } else if should_indent_next_line(cfg, &t) {
            // VS Code tests line `i` (not `j`) in its inner loop, so the run never stops and the
            // indentation comes from the first line.
            let l = 0;
            return Some(Inherit {
                indentation: leading_ws(&m.get(l)).to_string(),
                indent: false,
                line: Some(l),
            });
        } else if should_decrease(cfg, &t) {
            return Some(Inherit {
                indentation: leading_ws(&t).to_string(),
                indent: false,
                line: Some(i),
            });
        }
    }
    Some(Inherit {
        indentation: leading_ws(&m.get(0)).to_string(),
        indent: false,
        line: Some(0),
    })
}

/// `getGoodIndentForLine`.
#[allow(dead_code)] // the indentation contrib (reindent) uses it next
pub(crate) fn good_indent_for_line(
    cfg: &EditorConfig,
    m: &Lines<'_>,
    line: usize,
) -> Option<String> {
    if !has_indent_rules(cfg) {
        return None;
    }
    let indent = inherit_indent_for_line(cfg, m, line, true)?;
    let line_text = m.get(line);
    if let Some(inherit_line) = indent.line {
        let mut apply_enter = true;
        let mut b = inherit_line;
        while b + 1 < line {
            if !is_blank(&m.get(b)) {
                apply_enter = false;
                break;
            }
            b += 1;
        }
        if apply_enter {
            let inherit_text = m.get(inherit_line);
            if let Some(r) = on_enter(cfg, "", &inherit_text, "") {
                let mut indentation = leading_ws(&inherit_text).to_string();
                if let Some(n) = r.remove_text
                    && n > 0
                {
                    let keep = indentation.chars().count().saturating_sub(n);
                    indentation = indentation.chars().take(keep).collect();
                }
                match r.indent {
                    IndentAction::Indent | IndentAction::IndentOutdent => {
                        indentation = shift_indent(cfg, &indentation)
                    }
                    IndentAction::Outdent => indentation = unshift_indent(cfg, &indentation),
                    IndentAction::None => {}
                }
                if should_decrease(cfg, &line_text) {
                    indentation = unshift_indent(cfg, &indentation);
                }
                if let Some(a) = &r.append_text {
                    indentation.push_str(a);
                }
                return Some(leading_ws(&indentation).to_string());
            }
        }
    }
    Some(if should_decrease(cfg, &line_text) {
        if indent.indent {
            indent.indentation
        } else {
            unshift_indent(cfg, &indent.indentation)
        }
    } else if indent.indent {
        shift_indent(cfg, &indent.indentation)
    } else {
        indent.indentation
    })
}

/// `getIndentForEnter`: (indentation before the cursor, indentation for the new line).
pub(crate) fn indent_for_enter(
    cfg: &EditorConfig,
    text: &crate::text::Text,
    start: (usize, usize),
    end: (usize, usize),
) -> Option<(String, String)> {
    if !has_indent_rules(cfg) {
        return None;
    }
    let start_line = text.line(start.0).to_string();
    let end_line = text.line(end.0).to_string();
    let before: String = start_line.chars().take(start.1).collect();
    let after: String = end_line.chars().skip(end.1).collect();
    let before_indent = leading_ws(&before).to_string();
    let vm = Lines {
        text,
        patch: Some((start.0, before.clone())),
    };
    let Some(a) = inherit_indent_for_line(cfg, &vm, start.0 + 1, true) else {
        return Some((before_indent.clone(), before_indent));
    };
    let mut after_indent = a.indentation;
    if a.indent {
        after_indent = shift_indent(cfg, &after_indent);
    }
    if should_decrease(cfg, &after) {
        after_indent = unshift_indent(cfg, &after_indent);
    }
    Some((before_indent, after_indent))
}

/// `getIndentActionForType` for a selection on one line (`start`..`end` columns on `line`,
/// `end_line` the selection's last line). `auto_close_open_ends` says whether some auto-closing
/// pair's open ends with `ch`.
pub(crate) fn indent_action_for_type(
    cfg: &EditorConfig,
    text: &crate::text::Text,
    start: (usize, usize),
    end: (usize, usize),
    ch: &str,
    auto_close_open_ends: bool,
) -> Option<String> {
    if !has_indent_rules(cfg) {
        return None;
    }
    let start_line = text.line(start.0).to_string();
    let end_line = if end.0 == start.0 {
        start_line.clone()
    } else {
        text.line(end.0).to_string()
    };
    let before: String = start_line.chars().take(start.1).collect();
    let after: String = end_line.chars().skip(end.1).collect();
    let around = format!("{before}{after}");
    let around_ch = format!("{before}{ch}{after}");
    let m = Lines::new(text);
    if !should_decrease(cfg, &around) && should_decrease(cfg, &around_ch) {
        let r = inherit_indent_for_line(cfg, &m, start.0, false)?;
        let mut indentation = r.indentation;
        if !r.indent {
            indentation = unshift_indent(cfg, &indentation);
        }
        return Some(indentation);
    }
    if start == end && start.0 > 0 {
        let previous = text.line(start.0 - 1).to_string();
        if should_indent_next_line(cfg, &previous) && should_increase(cfg, &around_ch) {
            let inherited = inherit_indent_for_line(cfg, &m, start.0, false)?.indentation;
            let actual = leading_ws(&start_line);
            let inferred = shift_indent(cfg, &inherited);
            if inferred == actual && auto_close_open_ends && is_blank(&around) {
                return Some(inherited);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn javascript_regexes() {
        assert_eq!(
            js_to_rust(r"\\begin{(?!document)"),
            r"\\begin\{(?!document)"
        );
        assert_eq!(js_to_rust(r"a{2,3}\/[[x]"), r"a{2,3}/[\[x]");
        // Lookahead, backreference, inline flags.
        assert!(test(r"^((?!\/\/).)*\{\s*$", "if (a) {"));
        assert!(!test(r"^((?!\/\/).)*\{\s*$", "// {"));
        assert!(test(r"\\begin{([^}]*)}(?!.*\\end{\1})", r"\begin{x}"));
        assert!(!test(
            r"\\begin{([^}]*)}(?!.*\\end{\1})",
            r"\begin{x} \end{x}"
        ));
        assert!(test("(?i)^END", "end"));
        assert!(!test("(", "("));
        // JavaScript-only class syntax (HTML's onEnter rules) and empty classes.
        assert!(test(r"^<\/([_:\w][_:\w-.\d]*)\s*>", "</my-el.x>"));
        assert!(test(r"^\s*(}|\)[]?)$", "  )"));
        assert!(!test(r"a[]", "a"));
        assert!(test(r"a[^]b", "a\nb"));
        // A backreference to a group that did not take part matches the empty string (Ruby's
        // increaseIndentPattern relies on it).
        assert!(test(r"^(?:(x)|y)\1z$", "yz"));
        assert!(test(r"^(a)\1$", "aa"));
        assert!(!test(r"^(a)\1$", "ab"));
        // Compile size is bounded: a huge repetition does not compile (and never matches).
        assert!(regex(r"\w{1000}{1000}").is_none());
    }

    #[test]
    fn bracket_rules() {
        let cfg = EditorConfig::default();
        let a = on_enter(&cfg, "", "if (a) {", "}").expect("rule");
        assert_eq!(a.indent, IndentAction::IndentOutdent);
        let a = on_enter(&cfg, "", "foo(", "").expect("rule");
        assert_eq!(a.indent, IndentAction::Indent);
        assert!(on_enter(&cfg, "", "foo", "").is_none());
    }

    #[test]
    fn shift_and_unshift() {
        let cfg = EditorConfig::default();
        assert_eq!(shift_indent(&cfg, "  "), "    ");
        assert_eq!(unshift_indent(&cfg, "      "), "    ");
        let tabs = EditorConfig {
            insert_spaces: false,
            ..EditorConfig::default()
        };
        assert_eq!(shift_indent(&tabs, "\t"), "\t\t");
        assert_eq!(unshift_indent(&tabs, "\t"), "");
    }

    /// Backtrack-limit survey (see the module doc): `QUIRE_BT_CASES=<ndjson> cargo test -p
    /// quire-core --release --lib backtrack_survey -- --ignored --nocapture`. Each line of the
    /// file is `{"p": pattern, "cases": [[line, javascript_verdict], ...]}`. Prints the smallest
    /// limit each case needs, by line length, and every disagreement with JavaScript.
    #[test]
    #[ignore]
    fn backtrack_survey() {
        let Ok(path) = std::env::var("QUIRE_BT_CASES") else {
            return;
        };
        // Smallest limit for which `re` answers (binary search over 1..=2^27).
        fn needed(p: &str, line: &str, fast: &fancy_regex::Regex) -> Option<(usize, bool)> {
            if let Ok(m) = fast.is_match(line) {
                return Some((1_000, m));
            }
            let (mut lo, mut hi) = (1_000usize, 1usize << 27);
            build(p, hi)?.is_match(line).ok()?;
            while hi - lo > lo / 100 + 1 {
                let mid = lo + (hi - lo) / 2;
                if build(p, mid)?.is_match(line).is_ok() {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            Some((hi, build(p, hi)?.is_match(line).ok()?))
        }
        let classes = [80usize, 200, 400, 1000, usize::MAX];
        let mut max_by_class: [(usize, String, String); 5] = Default::default();
        let mut over_1000 = [0usize; 5];
        let mut wrong: HashMap<String, usize> = HashMap::new();
        let (mut n, mut uncompiled) = (0usize, 0usize);
        let mut per_pattern = Vec::new();
        for row in std::fs::read_to_string(path).unwrap().lines() {
            let v: serde_json::Value = serde_json::from_str(row).unwrap();
            let p = v["p"].as_str().unwrap();
            let origin = v["origin"].as_str().unwrap_or("");
            let mut pmax = [0usize; 2];
            let Some(fast) = build(p, 1_000) else {
                uncompiled += 1;
                println!("does not compile: {p}");
                continue;
            };
            for c in v["cases"].as_array().unwrap() {
                let line = c[0].as_str().unwrap();
                let js = c[1].as_bool().unwrap();
                n += 1;
                let k = classes.iter().position(|&c| line.len() <= c).unwrap();
                let got = needed(p, line, &fast);
                let (need, m) = got.unwrap_or((usize::MAX, !js));
                pmax[usize::from(line.len() > 400)] = pmax[usize::from(line.len() > 400)].max(need);
                if need > 1_000 {
                    over_1000[k] += 1;
                }
                if need > max_by_class[k].0 {
                    max_by_class[k] = (need, p.to_string(), line.to_string());
                }
                if m != js {
                    *wrong.entry(p.to_string()).or_default() += 1;
                }
            }
            per_pattern.push((origin.to_string(), p.to_string(), pmax));
        }
        println!("cases {n}, uncompiled patterns {uncompiled}");
        for (o, p, m) in &per_pattern {
            println!("pattern {o} <=400: {} >400: {} {p}", m[0], m[1]);
        }
        for (p, k) in &wrong {
            println!("disagrees with JavaScript on {k} lines: {p}");
        }
        for (i, (need, p, line)) in max_by_class.iter().enumerate() {
            println!(
                "lines <= {:>5} bytes: max needed {need}, {} cases need > 1000; {p:?} on {:?}",
                classes[i].min(99_999),
                over_1000[i],
                &line[..line.len().min(100)]
            );
        }
    }
}
