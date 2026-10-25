//! Key chords in VS Code's `keybindings.json` notation (`ctrl+shift+left`, `cmd+s`, `f3`), over
//! presto's keys (named as egui names them, so the notation and the bundled keymap carry over from
//! egui_quire unchanged).

use presto_base::event::Modifiers;
use presto_base::key::Key;

/// One key with exact physical modifiers. `ctrl` and `cmd` are separate: on macOS `cmd` is ⌘ and
/// `ctrl` is ⌃; elsewhere `cmd` (Windows/Super) is never bound by the default keymap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub cmd: bool,
}

impl Chord {
    /// Parse `ctrl+shift+left`. Modifier names: `ctrl`, `shift`, `alt` (`option`), `cmd`
    /// (`meta`, `win`, `super`). Key names: VS Code's (`left`, `pageup`, `backspace`, `f3`, `/`)
    /// or [`Key::from_name`]'s. Two-step chords (`ctrl+k ctrl+c`) are [`KeySeq`]s.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.contains(' ') {
            return Err(format!("two-step chord, use KeySeq::parse: {s:?}"));
        }
        let mut c = Self {
            key: Key::Escape,
            ctrl: false,
            shift: false,
            alt: false,
            cmd: false,
        };
        // Split on '+' but allow a trailing '+' key ("ctrl++" is not used by VS Code; "ctrl+=" is).
        let parts: Vec<&str> = s.split('+').collect();
        let (mods, key) = parts.split_at(parts.len() - 1);
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" | "option" => c.alt = true,
                "cmd" | "meta" | "win" | "super" => c.cmd = true,
                other => return Err(format!("unknown modifier {other:?} in {s:?}")),
            }
        }
        c.key = parse_key(key[0]).ok_or_else(|| format!("unknown key {:?} in {s:?}", key[0]))?;
        Ok(c)
    }

    /// Does a key press with `m` match this chord exactly? The physical flags are compared, so
    /// `ctrl+left` and `ctrl+shift+left` never shadow each other; AltGr is not Ctrl+Alt.
    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        key == self.key
            && m.contains(Modifiers::CTRL) == self.ctrl
            && m.contains(Modifiers::SHIFT) == self.shift
            && m.contains(Modifiers::ALT) == self.alt
            && m.contains(Modifiers::SUPER) == self.cmd
    }

    /// The modifiers of this chord (for tests and synthetic input).
    pub fn modifiers(&self) -> Modifiers {
        let mut m = Modifiers::default();
        for (on, bit) in [
            (self.ctrl, Modifiers::CTRL),
            (self.shift, Modifiers::SHIFT),
            (self.alt, Modifiers::ALT),
            (self.cmd, Modifiers::SUPER),
        ] {
            if on {
                m = m.union(bit);
            }
        }
        m
    }
}

/// A key binding: one chord, or two pressed one after the other (`ctrl+k ctrl+c`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeySeq {
    pub first: Chord,
    pub second: Option<Chord>,
}

impl KeySeq {
    /// Parse `ctrl+s` or `ctrl+k ctrl+c` (VS Code notation; at most two chords).
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut parts = s.split_whitespace();
        let first = Chord::parse(parts.next().ok_or_else(|| format!("empty key {s:?}"))?)?;
        let second = parts.next().map(Chord::parse).transpose()?;
        if parts.next().is_some() {
            return Err(format!("more than two chords in {s:?}"));
        }
        Ok(Self { first, second })
    }
}

impl From<Chord> for KeySeq {
    fn from(first: Chord) -> Self {
        Self {
            first,
            second: None,
        }
    }
}

fn parse_key(name: &str) -> Option<Key> {
    let lower = name.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "enter" => "Enter",
        "escape" => "Escape",
        "tab" => "Tab",
        "space" => "Space",
        "home" => "Home",
        "end" => "End",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        _ => "",
    };
    if !mapped.is_empty() {
        return Key::from_name(mapped);
    }
    if lower.len() > 1 && lower.starts_with('f') && lower[1..].chars().all(|c| c.is_ascii_digit()) {
        return Key::from_name(&lower.to_ascii_uppercase());
    }
    Key::from_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vscode_notation() {
        let c = Chord::parse("ctrl+shift+left").unwrap();
        assert_eq!(c.key, Key::ArrowLeft);
        assert!(c.ctrl && c.shift && !c.alt && !c.cmd);
        assert_eq!(Chord::parse("cmd+s").unwrap().key, Key::S);
        assert_eq!(Chord::parse("f3").unwrap().key, Key::F3);
        assert_eq!(Chord::parse("ctrl+/").unwrap().key, Key::Slash);
        assert_eq!(Chord::parse("pagedown").unwrap().key, Key::PageDown);
        assert!(Chord::parse("ctrl+k ctrl+c").is_err());
        let seq = KeySeq::parse("ctrl+k  ctrl+c").unwrap();
        assert_eq!(seq.first, Chord::parse("ctrl+k").unwrap());
        assert_eq!(seq.second, Some(Chord::parse("ctrl+c").unwrap()));
        assert_eq!(KeySeq::parse("f3").unwrap().second, None);
        assert!(KeySeq::parse("a b c").is_err());
        assert!(Chord::parse("hyper+a").is_err());
    }

    #[test]
    fn exact_match() {
        let c = Chord::parse("ctrl+left").unwrap();
        assert!(c.matches(Key::ArrowLeft, Modifiers::CTRL));
        assert!(!c.matches(Key::ArrowLeft, Modifiers::CTRL.union(Modifiers::SHIFT)));
        assert!(
            !c.matches(Key::ArrowLeft, Modifiers::ALT_GR),
            "AltGr is not Ctrl"
        );
        let mac = Chord::parse("cmd+s").unwrap();
        assert!(mac.matches(Key::S, mac.modifiers()));
    }
}
