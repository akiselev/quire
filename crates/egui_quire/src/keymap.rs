//! Keymaps as data: VS Code `keybindings.json` entries (`key`, `mac`, `linux`, `win`, `command`,
//! `args`, `when`) resolved to editor commands.
//!
//! The default keymap is `keymaps/vscode-default.json` (VS Code's default editor bindings, the
//! subset quire implements). `key` applies on every platform unless a platform field (`mac`,
//! `linux`, `win`) overrides it; an entry with only `mac` binds on macOS only. Command ids that
//! do not resolve to a known command are not bound ("not implemented means not registered") and
//! are listed in [`Keymap::unresolved`].

use egui::os::OperatingSystem;
use egui::{Key, Modifiers};
use serde::Deserialize;

use crate::chord::{Chord, KeySeq};
use crate::command::KeyAction;

/// The VS Code default keymap shipped with the crate.
pub const VSCODE_DEFAULT_JSON: &str = include_str!("../keymaps/vscode-default.json");

/// Synthetic input (tests, remote control) may set only the logical `command` flag; map it to
/// the platform's physical key (⌘ on macOS, Ctrl elsewhere).
fn normalize(os: OperatingSystem, mut m: Modifiers) -> Modifiers {
    if m.command && !m.ctrl && !m.mac_cmd {
        if matches!(os, OperatingSystem::Mac | OperatingSystem::IOS) {
            m.mac_cmd = true;
        } else {
            m.ctrl = true;
        }
    }
    m
}

/// One `keybindings.json` entry as written.
#[derive(Clone, Debug, Deserialize)]
pub struct RawBinding {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default)]
    pub linux: Option<String>,
    #[serde(default)]
    pub win: Option<String>,
    pub command: String,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(default)]
    pub when: Option<String>,
}

/// A context key `when` clauses test (VS Code's names, as written in keybindings.json).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextKey {
    FindWidgetVisible,
    MultipleSelections,
    SuggestWidgetVisible,
    SuggestWidgetHasFocusedSuggestion,
    SuggestWidgetMultipleSuggestions,
    AcceptSuggestionOnEnter,
    SuggestionMakesTextEdit,
    InSnippetMode,
    HasNextTabstop,
    HasPrevTabstop,
}

impl ContextKey {
    const ALL: [ContextKey; 10] = [
        Self::FindWidgetVisible,
        Self::MultipleSelections,
        Self::SuggestWidgetVisible,
        Self::SuggestWidgetHasFocusedSuggestion,
        Self::SuggestWidgetMultipleSuggestions,
        Self::AcceptSuggestionOnEnter,
        Self::SuggestionMakesTextEdit,
        Self::InSnippetMode,
        Self::HasNextTabstop,
        Self::HasPrevTabstop,
    ];

    /// The VS Code name.
    pub fn name(self) -> &'static str {
        match self {
            Self::FindWidgetVisible => "findWidgetVisible",
            Self::MultipleSelections => "editorHasMultipleSelections",
            Self::SuggestWidgetVisible => "suggestWidgetVisible",
            Self::SuggestWidgetHasFocusedSuggestion => "suggestWidgetHasFocusedSuggestion",
            Self::SuggestWidgetMultipleSuggestions => "suggestWidgetMultipleSuggestions",
            Self::AcceptSuggestionOnEnter => "acceptSuggestionOnEnter",
            Self::SuggestionMakesTextEdit => "suggestionMakesTextEdit",
            Self::InSnippetMode => "inSnippetMode",
            Self::HasNextTabstop => "hasNextTabstop",
            Self::HasPrevTabstop => "hasPrevTabstop",
        }
    }

    fn from_name(n: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == n)
    }
}

/// A `when` clause: a conjunction (`&&`) of terms, each a name, `!name`, or a parenthesised
/// disjunction (`(a || !b)`). `editorTextFocus` and `textInputFocus` always hold (keys reach the
/// keymap only while the editor has focus).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct When {
    /// Every term must hold; a term holds when any of its literals does.
    pub all: Vec<Vec<(ContextKey, bool)>>,
}

impl When {
    /// Always.
    pub const ALWAYS: When = When { all: Vec::new() };

    /// Parses a clause.
    pub fn parse(s: Option<&str>) -> Result<Self, String> {
        let Some(s) = s.map(str::trim) else {
            return Ok(Self::default());
        };
        let mut all = Vec::new();
        for term in s.split("&&") {
            let term = term.trim();
            let inner = term
                .strip_prefix('(')
                .and_then(|t| t.strip_suffix(')'))
                .unwrap_or(term);
            let mut any = Vec::new();
            let mut always = false;
            for lit in inner.split("||") {
                let lit = lit.trim();
                let (neg, name) = match lit.strip_prefix('!') {
                    Some(rest) => (true, rest.trim()),
                    None => (false, lit),
                };
                match name {
                    "" | "editorTextFocus" | "textInputFocus" if !neg => always = true,
                    n => match ContextKey::from_name(n) {
                        Some(k) => any.push((k, !neg)),
                        None => return Err(format!("unsupported when clause {lit:?}")),
                    },
                }
            }
            if !always {
                all.push(any);
            }
        }
        Ok(When { all })
    }

    /// Whether this is [`When::ALWAYS`].
    pub fn is_always(&self) -> bool {
        self.all.is_empty()
    }

    /// Does the condition hold in `ctx`?
    pub fn holds(&self, ctx: &WhenContext) -> bool {
        self.all
            .iter()
            .all(|any| any.iter().any(|&(k, v)| ctx.get(k) == v))
    }
}

impl std::fmt::Display for When {
    /// The clause as keybindings.json writes it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lit = |(k, v): &(ContextKey, bool)| format!("{}{}", if *v { "" } else { "!" }, k.name());
        let terms: Vec<String> = self
            .all
            .iter()
            .map(|any| match &any[..] {
                [one] => lit(one),
                many => format!("({})", many.iter().map(lit).collect::<Vec<_>>().join(" || ")),
            })
            .collect();
        f.write_str(&terms.join(" && "))
    }
}

/// The state `when` clauses are evaluated against.
#[derive(Clone, Copy, Debug, Default)]
pub struct WhenContext {
    pub find_widget_visible: bool,
    pub multiple_selections: bool,
    pub suggest_widget_visible: bool,
    pub suggest_widget_has_focused_suggestion: bool,
    pub suggest_widget_multiple_suggestions: bool,
    pub accept_suggestion_on_enter: bool,
    pub suggestion_makes_text_edit: bool,
    pub in_snippet_mode: bool,
    pub has_next_tabstop: bool,
    pub has_prev_tabstop: bool,
}

impl WhenContext {
    /// A context key's value.
    pub fn get(&self, k: ContextKey) -> bool {
        match k {
            ContextKey::FindWidgetVisible => self.find_widget_visible,
            ContextKey::MultipleSelections => self.multiple_selections,
            ContextKey::SuggestWidgetVisible => self.suggest_widget_visible,
            ContextKey::SuggestWidgetHasFocusedSuggestion => {
                self.suggest_widget_has_focused_suggestion
            }
            ContextKey::SuggestWidgetMultipleSuggestions => self.suggest_widget_multiple_suggestions,
            ContextKey::AcceptSuggestionOnEnter => self.accept_suggestion_on_enter,
            ContextKey::SuggestionMakesTextEdit => self.suggestion_makes_text_edit,
            ContextKey::InSnippetMode => self.in_snippet_mode,
            ContextKey::HasNextTabstop => self.has_next_tabstop,
            ContextKey::HasPrevTabstop => self.has_prev_tabstop,
        }
    }

    /// Sets a context key.
    pub fn set(&mut self, k: ContextKey, v: bool) {
        let f = match k {
            ContextKey::FindWidgetVisible => &mut self.find_widget_visible,
            ContextKey::MultipleSelections => &mut self.multiple_selections,
            ContextKey::SuggestWidgetVisible => &mut self.suggest_widget_visible,
            ContextKey::SuggestWidgetHasFocusedSuggestion => {
                &mut self.suggest_widget_has_focused_suggestion
            }
            ContextKey::SuggestWidgetMultipleSuggestions => {
                &mut self.suggest_widget_multiple_suggestions
            }
            ContextKey::AcceptSuggestionOnEnter => &mut self.accept_suggestion_on_enter,
            ContextKey::SuggestionMakesTextEdit => &mut self.suggestion_makes_text_edit,
            ContextKey::InSnippetMode => &mut self.in_snippet_mode,
            ContextKey::HasNextTabstop => &mut self.has_next_tabstop,
            ContextKey::HasPrevTabstop => &mut self.has_prev_tabstop,
        };
        *f = v;
    }

    /// The default context with `when`'s first literal of every term made true (tests).
    pub fn satisfying(when: &When) -> Self {
        let mut c = WhenContext::default();
        for any in &when.all {
            if let Some(&(k, v)) = any.first() {
                c.set(k, v);
            }
        }
        c
    }
}

/// One resolved binding.
#[derive(Clone, Debug)]
pub struct Binding {
    /// Keys on Linux / Windows / macOS (None: not bound on that platform).
    pub linux: Option<KeySeq>,
    pub win: Option<KeySeq>,
    pub mac: Option<KeySeq>,
    pub when: When,
    pub command_id: String,
    pub action: KeyAction,
}

impl Binding {
    pub fn keys(&self, os: OperatingSystem) -> Option<KeySeq> {
        match os {
            OperatingSystem::Mac | OperatingSystem::IOS => self.mac,
            OperatingSystem::Windows => self.win,
            _ => self.linux,
        }
    }
}

/// A keymap: chords → actions, in file order (later entries win, as in VS Code).
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    // Shared, so the per-frame `KeyHandling` a widget is built with clones in O(1) (E1).
    bindings: std::sync::Arc<Vec<Binding>>,
    unresolved: std::sync::Arc<Vec<(String, String)>>,
}

impl Keymap {
    /// VS Code's default editor keymap (the commands quire implements).
    pub fn vscode_default() -> Self {
        Self::from_json(VSCODE_DEFAULT_JSON).expect("bundled keymap parses")
    }

    /// Parse a `keybindings.json` array. Malformed JSON or chords are errors; unknown commands
    /// are skipped and listed in [`Self::unresolved`].
    pub fn from_json(json: &str) -> Result<Self, String> {
        let raw: Vec<RawBinding> = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut map = Self::default();
        for r in raw {
            map.push_raw(r)?;
        }
        Ok(map)
    }

    /// Add one entry (later entries take precedence).
    pub fn push_raw(&mut self, r: RawBinding) -> Result<(), String> {
        let parse = |s: &Option<String>| s.as_deref().map(KeySeq::parse).transpose();
        let key = parse(&r.key)?;
        let mac = parse(&r.mac)?.or(key);
        let linux = parse(&r.linux)?.or(key);
        let win = parse(&r.win)?.or(key);
        let when = When::parse(r.when.as_deref())?;
        match KeyAction::resolve(&r.command, r.args.as_ref()) {
            Ok(action) => std::sync::Arc::make_mut(&mut self.bindings).push(Binding {
                linux,
                win,
                mac,
                when,
                command_id: r.command,
                action,
            }),
            Err(e) => std::sync::Arc::make_mut(&mut self.unresolved).push((r.command, e)),
        }
        Ok(())
    }

    /// Commands in the source that did not resolve, with the reason.
    pub fn unresolved(&self) -> &[(String, String)] {
        &self.unresolved
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// The one-chord action bound to `key` with `mods` on `os` when `ctx` holds (last matching
    /// entry wins). Two-chord bindings go through [`Self::lookup_seq`].
    pub fn lookup(
        &self,
        os: OperatingSystem,
        key: Key,
        mods: Modifiers,
        ctx: &WhenContext,
    ) -> Option<&Binding> {
        self.find_single(os, key, normalize(os, mods), ctx)
    }

    fn find_single(
        &self,
        os: OperatingSystem,
        key: Key,
        mods: Modifiers,
        ctx: &WhenContext,
    ) -> Option<&Binding> {
        self.bindings.iter().rev().find(|b| {
            b.keys(os)
                .is_some_and(|k| k.second.is_none() && k.first.matches(key, mods))
                && b.when.holds(ctx)
        })
    }

    /// Resolve a key press given the first chord of a pending two-chord sequence (if any).
    ///
    /// Without `pending`, a chord that starts a two-chord binding returns [`KeyLookup::Prefix`]
    /// (the prefix wins over a one-chord binding of the same keys, as in VS Code). With
    /// `pending`, only the second chords of bindings starting with it match; anything else is
    /// [`KeyLookup::Unbound`] (the sequence is cancelled).
    pub fn lookup_seq(
        &self,
        os: OperatingSystem,
        pending: Option<Chord>,
        key: Key,
        mods: Modifiers,
        ctx: &WhenContext,
    ) -> KeyLookup<'_> {
        let mods = normalize(os, mods);
        if let Some(p) = pending {
            return self
                .bindings
                .iter()
                .rev()
                .find(|b| {
                    b.keys(os).is_some_and(|k| {
                        k.first == p && k.second.is_some_and(|c| c.matches(key, mods))
                    }) && b.when.holds(ctx)
                })
                .map_or(KeyLookup::Unbound, KeyLookup::Bound);
        }
        let prefix = self.bindings.iter().find_map(|b| {
            b.keys(os)
                .filter(|k| k.second.is_some() && k.first.matches(key, mods) && b.when.holds(ctx))
                .map(|k| k.first)
        });
        if let Some(p) = prefix {
            return KeyLookup::Prefix(p);
        }
        self.find_single(os, key, mods, ctx)
            .map_or(KeyLookup::Unbound, KeyLookup::Bound)
    }

    /// The keys bound to a command id on `os` (first entry), for menus and tests.
    pub fn keys_for(&self, os: OperatingSystem, command_id: &str) -> Option<KeySeq> {
        self.bindings
            .iter()
            .find(|b| b.command_id == command_id)
            .and_then(|b| b.keys(os))
    }
}

/// What a key press resolves to.
#[derive(Clone, Copy, Debug)]
pub enum KeyLookup<'a> {
    /// A complete binding.
    Bound(&'a Binding),
    /// The first chord of a two-chord binding: wait for the second.
    Prefix(Chord),
    /// Nothing (with a pending first chord: the sequence is cancelled).
    Unbound,
}
