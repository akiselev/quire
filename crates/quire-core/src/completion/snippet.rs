//! Snippets: VS Code's snippet grammar and marker tree (`editor/contrib/snippet/browser/
//! snippetParser.ts`, microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`), translated
//! literally into an arena (markers are [`NodeId`]s of a [`Snippet`]).
//!
//! Lengths and offsets are UTF-8 bytes (VS Code: UTF-16 units).
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// A scanner token type (VS Code `TokenType`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(missing_docs)]
pub enum TokenType {
    Dollar,
    Colon,
    Comma,
    CurlyOpen,
    CurlyClose,
    Backslash,
    Forwardslash,
    Pipe,
    Int,
    VariableName,
    Format,
    Plus,
    Dash,
    QuestionMark,
    Eof,
}

/// A token: type, byte position and length.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Token {
    /// The type.
    pub ty: TokenType,
    /// Byte position.
    pub pos: usize,
    /// Byte length.
    pub len: usize,
}

fn static_type(ch: u8) -> Option<TokenType> {
    Some(match ch {
        b'$' => TokenType::Dollar,
        b':' => TokenType::Colon,
        b',' => TokenType::Comma,
        b'{' => TokenType::CurlyOpen,
        b'}' => TokenType::CurlyClose,
        b'\\' => TokenType::Backslash,
        b'/' => TokenType::Forwardslash,
        b'|' => TokenType::Pipe,
        b'+' => TokenType::Plus,
        b'-' => TokenType::Dash,
        b'?' => TokenType::QuestionMark,
        _ => return None,
    })
}

fn is_digit(ch: u8) -> bool {
    ch.is_ascii_digit()
}

fn is_variable_character(ch: u8) -> bool {
    ch == b'_' || ch.is_ascii_alphabetic()
}

/// VS Code's snippet `Scanner`.
#[derive(Clone, Debug, Default)]
pub struct Scanner {
    value: String,
    /// The byte position.
    pub pos: usize,
}

impl Scanner {
    /// A scanner over nothing.
    pub fn new() -> Self {
        Scanner::default()
    }

    /// Scans `value` from its start.
    pub fn text(&mut self, value: &str) {
        self.value = value.to_string();
        self.pos = 0;
    }

    /// A token's text.
    pub fn token_text(&self, t: &Token) -> &str {
        &self.value[t.pos..t.pos + t.len]
    }

    /// The next token.
    pub fn next(&mut self) -> Token {
        let b = self.value.as_bytes();
        if self.pos >= b.len() {
            return Token {
                ty: TokenType::Eof,
                pos: self.pos,
                len: 0,
            };
        }
        let pos = self.pos;
        let mut len = 0;
        let ch = b[pos];
        if let Some(ty) = static_type(ch) {
            self.pos += 1;
            return Token { ty, pos, len: 1 };
        }
        if is_digit(ch) {
            while pos + len < b.len() && is_digit(b[pos + len]) {
                len += 1;
            }
            self.pos += len;
            return Token {
                ty: TokenType::Int,
                pos,
                len,
            };
        }
        if is_variable_character(ch) {
            len = 1;
            while pos + len < b.len()
                && (is_variable_character(b[pos + len]) || is_digit(b[pos + len]))
            {
                len += 1;
            }
            self.pos += len;
            return Token {
                ty: TokenType::VariableName,
                pos,
                len,
            };
        }
        len = 1;
        while pos + len < b.len() {
            let c = b[pos + len];
            if static_type(c).is_some() || is_digit(c) || is_variable_character(c) {
                break;
            }
            len += 1;
        }
        self.pos += len;
        Token {
            ty: TokenType::Format,
            pos,
            len,
        }
    }
}

/// A marker id in a [`Snippet`].
pub type NodeId = usize;

/// `${n:/upcase}`, `${n:+if}`, `${n:-else}`, `${n:?if:else}` in a transform's format.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FormatString {
    /// The capture group.
    pub index: usize,
    /// `upcase`, `downcase`, `capitalize`, `pascalcase`, `camelcase`, `kebabcase`, `snakecase`.
    pub shorthand_name: Option<String>,
    /// Inserted when the group matched.
    pub if_value: Option<String>,
    /// Inserted when it did not.
    pub else_value: Option<String>,
}

fn js_upper_first(v: &str) -> String {
    let mut c = v.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn js_lower_first(v: &str) -> String {
    let mut c = v.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn re(src: &str) -> fancy_regex::Regex {
    fancy_regex::Regex::new(src).expect("built-in regex")
}

fn find_all(r: &fancy_regex::Regex, s: &str) -> Vec<String> {
    r.find_iter(s)
        .filter_map(Result::ok)
        .map(|m| m.as_str().to_string())
        .collect()
}

impl FormatString {
    /// A plain `$n`.
    pub fn group(index: usize) -> Self {
        FormatString {
            index,
            shorthand_name: None,
            if_value: None,
            else_value: None,
        }
    }

    /// The text for a group's value (VS Code `FormatString.resolve`).
    pub fn resolve(&self, value: Option<&str>) -> String {
        let v = value.unwrap_or("");
        let empty = v.is_empty();
        match self.shorthand_name.as_deref() {
            Some("upcase") => v.to_uppercase(),
            Some("downcase") => v.to_lowercase(),
            Some("capitalize") => js_upper_first(v),
            Some("pascalcase") => {
                if empty {
                    return String::new();
                }
                let words = find_all(&re(r"[\p{L}0-9]+"), v);
                if words.is_empty() {
                    return v.to_string();
                }
                words.iter().map(|w| js_upper_first(w)).collect()
            }
            Some("camelcase") => {
                if empty {
                    return String::new();
                }
                let words = find_all(&re(r"[\p{L}0-9]+"), v);
                if words.is_empty() {
                    return v.to_string();
                }
                words
                    .iter()
                    .enumerate()
                    .map(|(i, w)| if i == 0 { js_lower_first(w) } else { js_upper_first(w) })
                    .collect()
            }
            Some("kebabcase") => {
                if empty {
                    return String::new();
                }
                to_kebab_case(v)
            }
            Some("snakecase") => {
                if empty {
                    return String::new();
                }
                let s = re(r"(\p{Ll})(\p{Lu})").replace_all(v, "${1}_${2}");
                re(r"[\s\-]+").replace_all(&s, "_").to_lowercase()
            }
            _ => {
                if !empty && let Some(i) = &self.if_value {
                    i.clone()
                } else if empty && let Some(e) = &self.else_value {
                    e.clone()
                } else {
                    v.to_string()
                }
            }
        }
    }

    fn to_textmate_string(&self) -> String {
        let mut value = format!("${{{}", self.index);
        if let Some(s) = &self.shorthand_name {
            value += &format!(":/{s}");
        } else if let (Some(i), Some(e)) = (&self.if_value, &self.else_value)
            && !i.is_empty()
            && !e.is_empty()
        {
            value += &format!(":?{i}:{e}");
        } else if let Some(i) = self.if_value.as_ref().filter(|i| !i.is_empty()) {
            value += &format!(":+{i}");
        } else if let Some(e) = self.else_value.as_ref().filter(|e| !e.is_empty()) {
            value += &format!(":-{e}");
        }
        value + "}"
    }
}

fn to_kebab_case(value: &str) -> String {
    if find_all(&re(r"[\p{L}0-9]+"), value).is_empty() {
        return value.to_string();
    }
    let trim_us = re(r"^_+|_+$");
    let cleaned = trim_us.replace_all(value.trim(), "").to_string();
    let words = find_all(
        &re(r"\p{Lu}{2,}(?=\p{Lu}\p{Ll}+[0-9]*|[\s_-]|$)|\p{Lu}?\p{Ll}+[0-9]*|\p{Lu}(?=\p{Lu}\p{Ll})|\p{Lu}(?=[\s_-]|$)|[0-9]+"),
        &cleaned,
    );
    if words.is_empty() {
        return re(r"[\s_-]+")
            .split(&cleaned)
            .filter_map(Result::ok)
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join("-");
    }
    words
        .iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

/// An item of a transform's format.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FormatItem {
    /// Literal text.
    Text(String),
    /// A group reference.
    Format(FormatString),
}

/// `/regex/format/options` on a placeholder or variable (VS Code `Transform`).
#[derive(Clone, Debug)]
pub struct Transform {
    /// The regex source as written.
    pub source: String,
    /// The options as written (`i`, `g`, ...).
    pub flags: String,
    regexp: fancy_regex::Regex,
    /// The format.
    pub items: Vec<FormatItem>,
}

impl PartialEq for Transform {
    fn eq(&self, o: &Self) -> bool {
        self.source == o.source && self.flags == o.flags && self.items == o.items
    }
}

impl Transform {
    /// A transform of a JavaScript regex with JavaScript flags; `None` when either is invalid.
    pub fn new(source: &str, flags: &str, items: Vec<FormatItem>) -> Option<Self> {
        let mut t = Transform {
            source: source.to_string(),
            flags: flags.to_string(),
            regexp: crate::lang::build_js_regex(source, flags)?,
            items: Vec::new(),
        };
        for i in items {
            t.push(i);
        }
        Some(t)
    }

    fn push(&mut self, item: FormatItem) {
        if let (FormatItem::Text(t), Some(FormatItem::Text(last))) = (&item, self.items.last_mut())
        {
            last.push_str(t);
            return;
        }
        self.items.push(item);
    }

    /// Applies the transform to a value (JavaScript `String.replace`: every match with `g`,
    /// else the first; with no match, the else branches if there are any).
    pub fn resolve(&self, value: &str) -> String {
        let global = self.flags.contains('g');
        let mut out = String::new();
        let mut last = 0;
        let mut did_match = false;
        let mut at = 0;
        while at <= value.len() {
            let Ok(Some(caps)) = self.regexp.captures_from_pos(value, at) else {
                break;
            };
            let m = caps.get(0).expect("group 0");
            did_match = true;
            out.push_str(&value[last..m.start()]);
            let groups: Vec<Option<&str>> =
                (0..caps.len()).map(|i| caps.get(i).map(|g| g.as_str())).collect();
            out.push_str(&self.replace(&groups));
            last = m.end();
            if !global {
                break;
            }
            at = if m.end() == m.start() {
                // step past an empty match by one character
                value[m.end()..]
                    .chars()
                    .next()
                    .map_or(value.len() + 1, |c| m.end() + c.len_utf8())
            } else {
                m.end()
            };
        }
        out.push_str(&value[last.min(value.len())..]);
        if !did_match
            && self.items.iter().any(|i| {
                matches!(i, FormatItem::Format(f) if f.else_value.as_deref().is_some_and(|e| !e.is_empty()))
            })
        {
            return self.replace(&[]);
        }
        out
    }

    fn replace(&self, groups: &[Option<&str>]) -> String {
        let mut ret = String::new();
        for item in &self.items {
            match item {
                FormatItem::Format(f) => {
                    let v = groups.get(f.index).copied().flatten().filter(|v| !v.is_empty());
                    ret.push_str(&f.resolve(v));
                }
                FormatItem::Text(t) => ret.push_str(t),
            }
        }
        ret
    }

    /// The transform in TextMate syntax (`/regex/format/flags`).
    pub fn to_textmate_string(&self) -> String {
        let mut fmt = String::new();
        for i in &self.items {
            match i {
                FormatItem::Text(t) => fmt.push_str(t),
                FormatItem::Format(f) => fmt.push_str(&f.to_textmate_string()),
            }
        }
        let flags: String = ['i', 'g'].iter().filter(|f| self.flags.contains(**f)).collect();
        format!("/{}/{}/{}", self.source, fmt, flags)
    }
}

/// What a marker is.
#[derive(Clone, PartialEq, Debug)]
pub enum Marker {
    /// The snippet root (VS Code `TextmateSnippet`).
    Root,
    /// Text.
    Text(String),
    /// A tabstop or placeholder (`$1`, `${1:default}`, `${1|a,b|}`); `0` is the final tabstop.
    Placeholder {
        /// The index.
        index: u32,
        /// A transform applied when leaving it.
        transform: Option<Transform>,
    },
    /// The options of a choice placeholder.
    Choice(Vec<String>),
    /// A variable (`$TM_SELECTED_TEXT`, `${name:default}`).
    Variable {
        /// The name.
        name: String,
        /// A transform of its value.
        transform: Option<Transform>,
    },
}

#[derive(Clone, Debug)]
struct Node {
    marker: Marker,
    children: Vec<NodeId>,
    parent: Option<NodeId>,
}

/// A parsed snippet: a marker tree in an arena. The root is [`Snippet::ROOT`].
#[derive(Clone, Debug)]
pub struct Snippet {
    nodes: Vec<Node>,
}

/// Resolves variables (VS Code `VariableResolver`): `None` leaves the variable unresolved.
pub trait VariableResolver {
    /// The value of a variable.
    fn resolve(&self, name: &str) -> Option<String>;
}

impl<F: Fn(&str) -> Option<String>> VariableResolver for F {
    fn resolve(&self, name: &str) -> Option<String> {
        self(name)
    }
}

impl Default for Snippet {
    fn default() -> Self {
        Snippet {
            nodes: vec![Node {
                marker: Marker::Root,
                children: Vec::new(),
                parent: None,
            }],
        }
    }
}

impl Snippet {
    /// The root marker.
    pub const ROOT: NodeId = 0;

    /// Parses a snippet (VS Code `SnippetParser.parse(value, insertFinalTabstop,
    /// enforceFinalTabstop)`).
    pub fn parse(value: &str, insert_final_tabstop: bool, enforce_final_tabstop: bool) -> Self {
        let mut p = SnippetParser::new();
        p.parse(value, insert_final_tabstop, enforce_final_tabstop)
    }

    /// The text a snippet inserts with no tabstops or variables (VS Code
    /// `SnippetParser.asInsertText`).
    pub fn as_insert_text(value: &str) -> String {
        Snippet::parse(value, false, false).to_string()
    }

    /// Escapes `$`, `}` and `\` (VS Code `SnippetParser.escape`).
    pub fn escape(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for c in value.chars() {
            if matches!(c, '$' | '}' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }

    fn add(&mut self, marker: Marker) -> NodeId {
        self.nodes.push(Node {
            marker,
            children: Vec::new(),
            parent: None,
        });
        self.nodes.len() - 1
    }

    /// The marker.
    pub fn marker(&self, id: NodeId) -> &Marker {
        &self.nodes[id].marker
    }

    /// Its children.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id].children
    }

    /// Its parent.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].parent
    }

    /// The placeholder index, if `id` is a placeholder.
    pub fn placeholder_index(&self, id: NodeId) -> Option<u32> {
        match self.nodes[id].marker {
            Marker::Placeholder { index, .. } => Some(index),
            _ => None,
        }
    }

    /// A choice placeholder's options.
    pub fn choice(&self, id: NodeId) -> Option<&[String]> {
        match self.children(id) {
            [c] => match &self.nodes[*c].marker {
                Marker::Choice(o) => Some(o),
                _ => None,
            },
            _ => None,
        }
    }

    /// Replaces a text marker's value.
    pub fn set_text(&mut self, id: NodeId, value: String) {
        if let Marker::Text(t) = &mut self.nodes[id].marker {
            *t = value;
        }
    }

    /// Appends a child (adjacent texts merge), VS Code `appendChild`.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        if let Marker::Choice(opts) = &self.nodes[parent].marker {
            if let Marker::Text(t) = self.nodes[child].marker.clone() {
                let mut opts = opts.clone();
                opts.push(t);
                self.nodes[parent].marker = Marker::Choice(opts);
            }
            return;
        }
        if let Marker::Text(t) = &self.nodes[child].marker
            && let Some(&last) = self.nodes[parent].children.last()
            && let Marker::Text(prev) = &self.nodes[last].marker
        {
            let merged = format!("{prev}{t}");
            self.nodes[last].marker = Marker::Text(merged);
            return;
        }
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }

    /// Replaces a child with others (VS Code `replace`).
    pub fn replace(&mut self, child: NodeId, others: &[NodeId]) {
        let Some(parent) = self.nodes[child].parent else {
            return;
        };
        let idx = self.nodes[parent]
            .children
            .iter()
            .position(|&c| c == child)
            .expect("child of its parent");
        self.nodes[parent].children.splice(idx..idx + 1, others.iter().copied());
        for &o in others {
            self.nodes[o].parent = Some(parent);
        }
        self.nodes[child].parent = None;
    }

    /// Copies markers of another snippet (with their subtrees) into this one, detached; returns
    /// their ids here (for [`Snippet::replace`]).
    pub fn graft(&mut self, other: &Snippet, nodes: &[NodeId]) -> Vec<NodeId> {
        fn copy(dst: &mut Snippet, src: &Snippet, id: NodeId) -> NodeId {
            let n = dst.add(src.nodes[id].marker.clone());
            for &k in &src.nodes[id].children {
                let c = copy(dst, src, k);
                dst.nodes[c].parent = Some(n);
                dst.nodes[n].children.push(c);
            }
            n
        }
        nodes.iter().map(|&n| copy(self, other, n)).collect()
    }

    /// A deep copy of a marker's subtree, detached.
    pub fn clone_node(&mut self, id: NodeId) -> NodeId {
        let marker = self.nodes[id].marker.clone();
        let n = self.add(marker);
        let kids = self.nodes[id].children.clone();
        for k in kids {
            let c = self.clone_node(k);
            self.nodes[c].parent = Some(n);
            self.nodes[n].children.push(c);
        }
        n
    }

    /// A marker's text as inserted (VS Code `toString`).
    pub fn node_string(&self, id: NodeId) -> String {
        match &self.nodes[id].marker {
            Marker::Text(t) => t.clone(),
            Marker::Choice(o) => o.first().cloned().unwrap_or_default(),
            _ => self.nodes[id]
                .children
                .iter()
                .map(|&c| self.node_string(c))
                .collect(),
        }
    }

    /// A marker's own length (texts and choices; containers 0), VS Code `len()`.
    pub fn len_of(&self, id: NodeId) -> usize {
        match &self.nodes[id].marker {
            Marker::Text(t) => t.len(),
            Marker::Choice(o) => o.first().map_or(0, String::len),
            _ => 0,
        }
    }

    /// A marker in TextMate syntax (VS Code `toTextmateString`).
    pub fn node_textmate_string(&self, id: NodeId) -> String {
        let kids = || -> String {
            self.nodes[id]
                .children
                .iter()
                .map(|&c| self.node_textmate_string(c))
                .collect()
        };
        match &self.nodes[id].marker {
            Marker::Root => kids(),
            Marker::Text(t) => Snippet::escape(t),
            Marker::Choice(o) => o
                .iter()
                .map(|v| {
                    let mut s = String::new();
                    for c in v.chars() {
                        if matches!(c, '|' | ',' | '\\') {
                            s.push('\\');
                        }
                        s.push(c);
                    }
                    s
                })
                .collect::<Vec<_>>()
                .join(","),
            Marker::Placeholder { index, transform } => {
                let t = transform.as_ref().map_or(String::new(), |t| t.to_textmate_string());
                if self.nodes[id].children.is_empty() && transform.is_none() {
                    format!("${index}")
                } else if self.nodes[id].children.is_empty() {
                    format!("${{{index}{t}}}")
                } else if let [c] = self.nodes[id].children[..]
                    && matches!(self.nodes[c].marker, Marker::Choice(_))
                {
                    format!("${{{index}|{}|{t}}}", self.node_textmate_string(c))
                } else {
                    format!("${{{index}:{}{t}}}", kids())
                }
            }
            Marker::Variable { name, transform } => {
                let t = transform.as_ref().map_or(String::new(), |t| t.to_textmate_string());
                if self.nodes[id].children.is_empty() {
                    format!("${{{name}{t}}}")
                } else {
                    format!("${{{name}:{}{t}}}", kids())
                }
            }
        }
    }

    /// The snippet in TextMate syntax.
    pub fn to_textmate_string(&self) -> String {
        self.node_textmate_string(Snippet::ROOT)
    }

    /// Pre-order walk from `starts` (VS Code's `walk`); the visitor returns `false` to stop.
    pub fn walk_from(&self, starts: &[NodeId], mut visitor: impl FnMut(NodeId) -> bool) {
        let mut stack: std::collections::VecDeque<NodeId> = starts.iter().copied().collect();
        while let Some(m) = stack.pop_front() {
            if !visitor(m) {
                break;
            }
            for &c in self.nodes[m].children.iter().rev() {
                stack.push_front(c);
            }
        }
    }

    /// Walks the whole snippet (not the root).
    pub fn walk(&self, visitor: impl FnMut(NodeId) -> bool) {
        let kids = self.nodes[Snippet::ROOT].children.clone();
        self.walk_from(&kids, visitor);
    }

    /// Every placeholder in walk order.
    pub fn placeholders(&self) -> Vec<NodeId> {
        let mut all = Vec::new();
        self.walk(|m| {
            if self.placeholder_index(m).is_some() {
                all.push(m);
            }
            true
        });
        all
    }

    /// The placeholder with the largest index (the first of them).
    pub fn last_placeholder(&self) -> Option<NodeId> {
        let mut last: Option<NodeId> = None;
        for p in self.placeholders() {
            let i = self.placeholder_index(p);
            if last.is_none_or(|l| self.placeholder_index(l) < i) {
                last = Some(p);
            }
        }
        last
    }

    /// The offset of a marker in the inserted text (VS Code `offset`), `None` if not found.
    pub fn offset(&self, marker: NodeId) -> Option<usize> {
        let mut pos = 0;
        let mut found = false;
        self.walk(|c| {
            if c == marker {
                found = true;
                return false;
            }
            pos += self.len_of(c);
            true
        });
        found.then_some(pos)
    }

    /// The length of a marker's text with its descendants (VS Code `fullLen`).
    pub fn full_len(&self, marker: NodeId) -> usize {
        let mut ret = 0;
        self.walk_from(&[marker], |m| {
            ret += self.len_of(m);
            true
        });
        ret
    }

    /// The placeholders enclosing a placeholder, innermost first.
    pub fn enclosing_placeholders(&self, placeholder: NodeId) -> Vec<NodeId> {
        let mut ret = Vec::new();
        let mut p = self.nodes[placeholder].parent;
        while let Some(id) = p {
            if self.placeholder_index(id).is_some() {
                ret.push(id);
            }
            p = self.nodes[id].parent;
        }
        ret
    }

    /// Resolves variables: a resolved variable's children become its value (VS Code
    /// `resolveVariables`).
    pub fn resolve_variables(&mut self, resolver: &dyn VariableResolver) {
        let mut vars = Vec::new();
        self.walk(|m| {
            if matches!(self.nodes[m].marker, Marker::Variable { .. }) {
                vars.push(m);
            }
            true
        });
        for v in vars {
            if self.nodes[v].parent.is_none() && v != Snippet::ROOT {
                continue; // replaced meanwhile
            }
            let Marker::Variable { name, transform } = self.nodes[v].marker.clone() else {
                continue;
            };
            let mut value = resolver.resolve(&name);
            if let Some(t) = &transform {
                value = Some(t.resolve(value.as_deref().unwrap_or("")));
            }
            if let Some(value) = value {
                for c in std::mem::take(&mut self.nodes[v].children) {
                    self.nodes[c].parent = None;
                }
                let t = self.add(Marker::Text(value));
                self.nodes[t].parent = Some(v);
                self.nodes[v].children.push(t);
            }
        }
    }

    /// Placeholder order (VS Code `Placeholder.compareByIndex`): by index, the final tabstop
    /// last.
    pub fn compare_by_index(&self, a: NodeId, b: NodeId) -> Ordering {
        let (ia, ib) = (self.placeholder_index(a), self.placeholder_index(b));
        if ia == ib {
            Ordering::Equal
        } else if ia == Some(0) {
            Ordering::Greater
        } else if ib == Some(0) {
            Ordering::Less
        } else {
            ia.cmp(&ib)
        }
    }

    /// The rightmost descendant of a marker (VS Code `rightMostDescendant`).
    pub fn right_most_descendant(&self, mut id: NodeId) -> NodeId {
        while let Some(&l) = self.nodes[id].children.last() {
            id = l;
        }
        id
    }
}

impl std::fmt::Display for Snippet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.node_string(Snippet::ROOT))
    }
}

/// VS Code's `SnippetParser`.
#[derive(Debug)]
pub struct SnippetParser {
    scanner: Scanner,
    token: Token,
}

impl Default for SnippetParser {
    fn default() -> Self {
        SnippetParser::new()
    }
}

impl SnippetParser {
    /// A parser.
    pub fn new() -> Self {
        SnippetParser {
            scanner: Scanner::new(),
            token: Token {
                ty: TokenType::Eof,
                pos: 0,
                len: 0,
            },
        }
    }

    /// Parses `value`.
    pub fn parse(
        &mut self,
        value: &str,
        insert_final_tabstop: bool,
        enforce_final_tabstop: bool,
    ) -> Snippet {
        let mut snippet = Snippet::default();
        self.parse_fragment(value, &mut snippet);
        self.ensure_final_tabstop(&mut snippet, enforce_final_tabstop, insert_final_tabstop);
        snippet
    }

    /// Parses `value` into `snippet` after its existing children; returns the new top-level
    /// markers.
    pub fn parse_fragment(&mut self, value: &str, snippet: &mut Snippet) -> Vec<NodeId> {
        let offset = snippet.children(Snippet::ROOT).len();
        self.scanner.text(value);
        self.token = self.scanner.next();
        while self.parse_marker(snippet, Snippet::ROOT) {}

        // Fill in values for placeholders: the first placeholder of an index that has a value
        // defines the value for all placeholders with that index.
        let mut defaults: BTreeMap<u32, Option<Vec<NodeId>>> = BTreeMap::new();
        let mut incomplete: Vec<NodeId> = Vec::new();
        snippet.walk(|m| {
            if let Some(index) = snippet.placeholder_index(m) {
                if index == 0 {
                    defaults.insert(0, None);
                } else if !defaults.contains_key(&index) && !snippet.children(m).is_empty() {
                    defaults.insert(index, Some(snippet.children(m).to_vec()));
                } else {
                    incomplete.push(m);
                }
            }
            true
        });
        let mut stack = BTreeSet::new();
        for p in incomplete {
            fill_in_incomplete(snippet, &defaults, p, &mut stack);
        }
        snippet.children(Snippet::ROOT)[offset..].to_vec()
    }

    fn ensure_final_tabstop(&self, snippet: &mut Snippet, enforce: bool, insert: bool) {
        if enforce || (insert && !snippet.placeholders().is_empty()) {
            let has_final = snippet
                .placeholders()
                .iter()
                .any(|&p| snippet.placeholder_index(p) == Some(0));
            if !has_final {
                let p = snippet.add(Marker::Placeholder {
                    index: 0,
                    transform: None,
                });
                snippet.append_child(Snippet::ROOT, p);
            }
        }
    }

    fn accept(&mut self, ty: Option<TokenType>) -> bool {
        if ty.is_none_or(|t| self.token.ty == t) {
            self.token = self.scanner.next();
            true
        } else {
            false
        }
    }

    fn accept_text(&mut self, ty: Option<TokenType>) -> Option<String> {
        if ty.is_none_or(|t| self.token.ty == t) {
            let s = self.scanner.token_text(&self.token).to_string();
            self.token = self.scanner.next();
            // JavaScript: an empty string is falsy (only EOF has one).
            (!s.is_empty()).then_some(s)
        } else {
            None
        }
    }

    fn back_to(&mut self, token: Token) -> bool {
        self.scanner.pos = token.pos + token.len;
        self.token = token;
        false
    }

    fn until(&mut self, ty: TokenType) -> Option<String> {
        let start = self.token;
        while self.token.ty != ty {
            if self.token.ty == TokenType::Eof {
                return None;
            } else if self.token.ty == TokenType::Backslash {
                let next = self.scanner.next();
                if !matches!(
                    next.ty,
                    TokenType::Dollar | TokenType::CurlyClose | TokenType::Backslash
                ) {
                    return None;
                }
            }
            self.token = self.scanner.next();
        }
        let raw = self.scanner.value[start.pos..self.token.pos].to_string();
        let mut value = String::new();
        let mut it = raw.chars().peekable();
        while let Some(c) = it.next() {
            if c == '\\' && it.peek().is_some_and(|n| matches!(n, '$' | '}' | '\\')) {
                value.push(it.next().expect("peeked"));
            } else {
                value.push(c);
            }
        }
        self.token = self.scanner.next();
        // JavaScript: an empty value is falsy.
        (!value.is_empty()).then_some(value)
    }

    fn parse_marker(&mut self, s: &mut Snippet, parent: NodeId) -> bool {
        self.parse_escaped(s, parent)
            || self.parse_tabstop_or_variable_name(s, parent)
            || self.parse_complex_placeholder(s, parent)
            || self.parse_complex_variable(s, parent)
            || self.parse_anything(s, parent)
    }

    fn text(&mut self, s: &mut Snippet, parent: NodeId, value: String) {
        let t = s.add(Marker::Text(value));
        s.append_child(parent, t);
    }

    // \$, \\, \} -> just text
    fn parse_escaped(&mut self, s: &mut Snippet, parent: NodeId) -> bool {
        if let Some(bs) = self.accept_text(Some(TokenType::Backslash)) {
            let value = self
                .accept_text(Some(TokenType::Dollar))
                .or_else(|| self.accept_text(Some(TokenType::CurlyClose)))
                .or_else(|| self.accept_text(Some(TokenType::Backslash)))
                .unwrap_or(bs);
            self.text(s, parent, value);
            return true;
        }
        false
    }

    // $foo -> variable, $1 -> tabstop
    fn parse_tabstop_or_variable_name(&mut self, s: &mut Snippet, parent: NodeId) -> bool {
        let token = self.token;
        let value = if self.accept(Some(TokenType::Dollar)) {
            self.accept_text(Some(TokenType::VariableName))
                .or_else(|| self.accept_text(Some(TokenType::Int)))
        } else {
            None
        };
        let Some(value) = value else {
            return self.back_to(token);
        };
        let m = if value.bytes().all(|b| b.is_ascii_digit()) {
            Marker::Placeholder {
                index: value.parse().unwrap_or(u32::MAX),
                transform: None,
            }
        } else {
            Marker::Variable {
                name: value,
                transform: None,
            }
        };
        let id = s.add(m);
        s.append_child(parent, id);
        true
    }

    // ${1:<children>}, ${1} -> placeholder
    fn parse_complex_placeholder(&mut self, s: &mut Snippet, parent: NodeId) -> bool {
        let token = self.token;
        let index = if self.accept(Some(TokenType::Dollar)) && self.accept(Some(TokenType::CurlyOpen))
        {
            self.accept_text(Some(TokenType::Int))
        } else {
            None
        };
        let Some(index) = index else {
            return self.back_to(token);
        };
        let n: u32 = index.parse().unwrap_or(u32::MAX);
        let placeholder = s.add(Marker::Placeholder {
            index: n,
            transform: None,
        });

        if self.accept(Some(TokenType::Colon)) {
            // ${1:<children>}
            loop {
                if self.accept(Some(TokenType::CurlyClose)) {
                    s.append_child(parent, placeholder);
                    return true;
                }
                if self.parse_marker(s, placeholder) {
                    continue;
                }
                // fallback
                self.text(s, parent, format!("${{{index}:"));
                for c in s.children(placeholder).to_vec() {
                    s.append_child(parent, c);
                }
                return true;
            }
        } else if n > 0 && self.accept(Some(TokenType::Pipe)) {
            // ${1|one,two,three|}
            let choice = s.add(Marker::Choice(Vec::new()));
            loop {
                if self.parse_choice_element(s, choice) {
                    if self.accept(Some(TokenType::Comma)) {
                        continue;
                    }
                    if self.accept(Some(TokenType::Pipe)) {
                        s.append_child(placeholder, choice);
                        if self.accept(Some(TokenType::CurlyClose)) {
                            s.append_child(parent, placeholder);
                            return true;
                        }
                    }
                }
                self.back_to(token);
                return false;
            }
        } else if self.accept(Some(TokenType::Forwardslash)) {
            // ${1/<regex>/<format>/<options>}
            if let Some(t) = self.parse_transform() {
                if let Marker::Placeholder { transform, .. } = &mut s.nodes[placeholder].marker {
                    *transform = Some(t);
                }
                s.append_child(parent, placeholder);
                return true;
            }
            self.back_to(token);
            false
        } else if self.accept(Some(TokenType::CurlyClose)) {
            // ${1}
            s.append_child(parent, placeholder);
            true
        } else {
            // ${1 <- missing curly or colon
            self.back_to(token)
        }
    }

    fn parse_choice_element(&mut self, s: &mut Snippet, choice: NodeId) -> bool {
        let token = self.token;
        let mut values: Vec<String> = Vec::new();
        loop {
            if matches!(self.token.ty, TokenType::Comma | TokenType::Pipe) {
                break;
            }
            let value = if let Some(bs) = self.accept_text(Some(TokenType::Backslash)) {
                // \, \|, or \\
                Some(
                    self.accept_text(Some(TokenType::Comma))
                        .or_else(|| self.accept_text(Some(TokenType::Pipe)))
                        .or_else(|| self.accept_text(Some(TokenType::Backslash)))
                        .unwrap_or(bs),
                )
            } else {
                self.accept_text(None)
            };
            let Some(value) = value else {
                // EOF
                self.back_to(token);
                return false;
            };
            values.push(value);
        }
        if values.is_empty() {
            self.back_to(token);
            return false;
        }
        let t = s.add(Marker::Text(values.concat()));
        s.append_child(choice, t);
        true
    }

    // ${foo:<children>}, ${foo} -> variable
    fn parse_complex_variable(&mut self, s: &mut Snippet, parent: NodeId) -> bool {
        let token = self.token;
        let name = if self.accept(Some(TokenType::Dollar)) && self.accept(Some(TokenType::CurlyOpen))
        {
            self.accept_text(Some(TokenType::VariableName))
        } else {
            None
        };
        let Some(name) = name else {
            return self.back_to(token);
        };
        let variable = s.add(Marker::Variable {
            name: name.clone(),
            transform: None,
        });
        if self.accept(Some(TokenType::Colon)) {
            loop {
                if self.accept(Some(TokenType::CurlyClose)) {
                    s.append_child(parent, variable);
                    return true;
                }
                if self.parse_marker(s, variable) {
                    continue;
                }
                self.text(s, parent, format!("${{{name}:"));
                for c in s.children(variable).to_vec() {
                    s.append_child(parent, c);
                }
                return true;
            }
        } else if self.accept(Some(TokenType::Forwardslash)) {
            if let Some(t) = self.parse_transform() {
                if let Marker::Variable { transform, .. } = &mut s.nodes[variable].marker {
                    *transform = Some(t);
                }
                s.append_child(parent, variable);
                return true;
            }
            self.back_to(token);
            false
        } else if self.accept(Some(TokenType::CurlyClose)) {
            s.append_child(parent, variable);
            true
        } else {
            self.back_to(token)
        }
    }

    fn parse_transform(&mut self) -> Option<Transform> {
        let mut regex_value = String::new();
        let mut regex_options = String::new();
        let mut items = Transform {
            source: String::new(),
            flags: String::new(),
            regexp: re(""),
            items: Vec::new(),
        };
        // (1) /regex
        loop {
            if self.accept(Some(TokenType::Forwardslash)) {
                break;
            }
            if let Some(bs) = self.accept_text(Some(TokenType::Backslash)) {
                let e = self.accept_text(Some(TokenType::Forwardslash)).unwrap_or(bs);
                regex_value.push_str(&e);
                continue;
            }
            if self.token.ty != TokenType::Eof {
                regex_value.push_str(&self.accept_text(None).unwrap_or_default());
                continue;
            }
            return None;
        }
        // (2) /format
        loop {
            if self.accept(Some(TokenType::Forwardslash)) {
                break;
            }
            if let Some(bs) = self.accept_text(Some(TokenType::Backslash)) {
                let e = self
                    .accept_text(Some(TokenType::Backslash))
                    .or_else(|| self.accept_text(Some(TokenType::Forwardslash)))
                    .unwrap_or(bs);
                items.push(FormatItem::Text(e));
                continue;
            }
            if self.parse_format_string(&mut items) {
                continue;
            }
            if self.token.ty != TokenType::Eof {
                let t = self.scanner.token_text(&self.token).to_string();
                items.push(FormatItem::Text(t));
                self.accept(None);
                continue;
            }
            return None;
        }
        // (3) /option
        loop {
            if self.accept(Some(TokenType::CurlyClose)) {
                break;
            }
            if self.token.ty != TokenType::Eof {
                regex_options.push_str(&self.accept_text(None).unwrap_or_default());
                continue;
            }
            return None;
        }
        let regexp = crate::lang::build_js_regex(&regex_value, &regex_options)?;
        items.source = regex_value;
        items.flags = regex_options;
        items.regexp = regexp;
        Some(items)
    }

    fn parse_format_string(&mut self, t: &mut Transform) -> bool {
        let token = self.token;
        if !self.accept(Some(TokenType::Dollar)) {
            return false;
        }
        let complex = self.accept(Some(TokenType::CurlyOpen));
        let Some(index) = self.accept_text(Some(TokenType::Int)) else {
            self.back_to(token);
            return false;
        };
        let index: usize = index.parse().unwrap_or(usize::MAX);
        if !complex || self.accept(Some(TokenType::CurlyClose)) {
            // $1, ${1}
            t.push(FormatItem::Format(FormatString::group(index)));
            return true;
        }
        if !self.accept(Some(TokenType::Colon)) {
            self.back_to(token);
            return false;
        }
        let mk = |sh: Option<String>, i: Option<String>, e: Option<String>| {
            FormatItem::Format(FormatString {
                index,
                shorthand_name: sh,
                if_value: i,
                else_value: e,
            })
        };
        if self.accept(Some(TokenType::Forwardslash)) {
            // ${1:/upcase}
            let sh = self.accept_text(Some(TokenType::VariableName));
            if sh.is_none() || !self.accept(Some(TokenType::CurlyClose)) {
                self.back_to(token);
                return false;
            }
            t.push(mk(sh, None, None));
            return true;
        } else if self.accept(Some(TokenType::Plus)) {
            if let Some(i) = self.until(TokenType::CurlyClose) {
                t.push(mk(None, Some(i), None));
                return true;
            }
        } else if self.accept(Some(TokenType::Dash)) {
            if let Some(e) = self.until(TokenType::CurlyClose) {
                t.push(mk(None, None, Some(e)));
                return true;
            }
        } else if self.accept(Some(TokenType::QuestionMark)) {
            if let Some(i) = self.until(TokenType::Colon)
                && let Some(e) = self.until(TokenType::CurlyClose)
            {
                t.push(mk(None, Some(i), Some(e)));
                return true;
            }
        } else if let Some(e) = self.until(TokenType::CurlyClose) {
            t.push(mk(None, None, Some(e)));
            return true;
        }
        self.back_to(token);
        false
    }

    fn parse_anything(&mut self, s: &mut Snippet, parent: NodeId) -> bool {
        if self.token.ty != TokenType::Eof {
            let t = self.scanner.token_text(&self.token).to_string();
            self.text(s, parent, t);
            self.accept(None);
            return true;
        }
        false
    }
}

fn fill_in_incomplete(
    s: &mut Snippet,
    defaults: &BTreeMap<u32, Option<Vec<NodeId>>>,
    placeholder: NodeId,
    stack: &mut BTreeSet<u32>,
) {
    let Some(index) = s.placeholder_index(placeholder) else {
        return;
    };
    let Some(Some(values)) = defaults.get(&index) else {
        return;
    };
    let transform = match &s.nodes[placeholder].marker {
        Marker::Placeholder { transform, .. } => transform.clone(),
        _ => None,
    };
    let clone = s.add(Marker::Placeholder { index, transform });
    for &child in values {
        let new_child = s.clone_node(child);
        s.append_child(clone, new_child);
        // "recurse" on children that are again placeholders
        if let Some(ci) = s.placeholder_index(new_child)
            && defaults.contains_key(&ci)
            && !stack.contains(&ci)
        {
            stack.insert(ci);
            fill_in_incomplete(s, defaults, new_child, stack);
            stack.remove(&ci);
        }
    }
    s.replace(placeholder, &[clone]);
}
