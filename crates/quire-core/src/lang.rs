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

use crate::commands::model::{next_indent_tab_stop, prev_indent_tab_stop, visible_col};
use crate::config::{EditorConfig, EnterAction, IndentAction};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// ---------------------------------------------------------------------------------------------
// regexes

type Re = Arc<fancy_regex::Regex>;

/// Rewrites a JavaScript regex source into fancy-regex syntax: a lone `{` or `}` that is not part
/// of a quantifier is a literal in JavaScript, and inside a class `[` and doubled `&`, `-`, `~` are
/// literals.
fn js_to_rust(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 8);
    let mut i = 0;
    let mut in_class = false;
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' {
            out.push(ch);
            if let Some(&n) = c.get(i + 1) {
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
            match ch {
                ']' => in_class = false,
                '[' => {
                    out.push_str("\\[");
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
                in_class = true;
                out.push(ch);
                // `[]` / `[^]` at the start: `]` is not a literal in JavaScript, but keep it simple.
                if c.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                if c.get(i + 1) == Some(&']') {
                    out.push_str("\\]");
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
    let re = fancy_regex::RegexBuilder::new(&js_to_rust(src))
        .backtrack_limit(100_000)
        .build()
        .ok()
        .map(Arc::new);
    map.insert(src.to_string(), re.clone());
    re
}

/// JavaScript `RegExp.test`; a pattern that does not compile or runs out of backtracking never
/// matches.
pub(crate) fn test(src: &str, text: &str) -> bool {
    regex(src).is_some_and(|r| r.is_match(text).unwrap_or(false))
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
}
