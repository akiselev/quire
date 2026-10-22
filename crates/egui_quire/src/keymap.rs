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

use crate::chord::Chord;
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

/// Conditions understood in `when` (a single name, optionally negated with `!`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Always,
    FindWidgetVisible(bool),
    MultipleSelections(bool),
}

impl When {
    fn parse(s: Option<&str>) -> Result<Self, String> {
        let Some(s) = s.map(str::trim) else { return Ok(Self::Always) };
        let (neg, name) = match s.strip_prefix('!') {
            Some(rest) => (true, rest.trim()),
            None => (false, s),
        };
        match name {
            "" | "editorTextFocus" => Ok(Self::Always),
            "findWidgetVisible" => Ok(Self::FindWidgetVisible(!neg)),
            "editorHasMultipleSelections" => Ok(Self::MultipleSelections(!neg)),
            other => Err(format!("unsupported when clause {other:?}")),
        }
    }

    /// Does the condition hold in `ctx`?
    pub fn holds(self, ctx: &WhenContext) -> bool {
        match self {
            Self::Always => true,
            Self::FindWidgetVisible(v) => ctx.find_widget_visible == v,
            Self::MultipleSelections(v) => ctx.multiple_selections == v,
        }
    }
}

/// The state `when` clauses are evaluated against.
#[derive(Clone, Copy, Debug, Default)]
pub struct WhenContext {
    pub find_widget_visible: bool,
    pub multiple_selections: bool,
}

/// One resolved binding.
#[derive(Clone, Debug)]
pub struct Binding {
    /// Chord on Linux / Windows / macOS (None: not bound on that platform).
    pub linux: Option<Chord>,
    pub win: Option<Chord>,
    pub mac: Option<Chord>,
    pub when: When,
    pub command_id: String,
    pub action: KeyAction,
}

impl Binding {
    pub fn chord(&self, os: OperatingSystem) -> Option<Chord> {
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
    bindings: Vec<Binding>,
    unresolved: Vec<(String, String)>,
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
        let parse = |s: &Option<String>| s.as_deref().map(Chord::parse).transpose();
        let key = parse(&r.key)?;
        let mac = parse(&r.mac)?.or(key);
        let linux = parse(&r.linux)?.or(key);
        let win = parse(&r.win)?.or(key);
        let when = When::parse(r.when.as_deref())?;
        match KeyAction::resolve(&r.command, r.args.as_ref()) {
            Ok(action) => self.bindings.push(Binding { linux, win, mac, when, command_id: r.command, action }),
            Err(e) => self.unresolved.push((r.command, e)),
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

    /// The action bound to `key` with `mods` on `os` when `ctx` holds (last matching entry wins).
    pub fn lookup(&self, os: OperatingSystem, key: Key, mods: Modifiers, ctx: &WhenContext) -> Option<&Binding> {
        let mods = normalize(os, mods);
        self.bindings
            .iter()
            .rev()
            .find(|b| b.chord(os).is_some_and(|c| c.matches(key, mods)) && b.when.holds(ctx))
    }

    /// The chord bound to a command id on `os` (first entry), for menus and tests.
    pub fn chord_for(&self, os: OperatingSystem, command_id: &str) -> Option<Chord> {
        self.bindings.iter().find(|b| b.command_id == command_id).and_then(|b| b.chord(os))
    }
}
