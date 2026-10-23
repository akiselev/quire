//! Editor theme and host-supplied text.
//!
//! Plain structs: the host fills them from its own tokens and catalog; the defaults derive from
//! `egui::Visuals`. Token colours are resolved at paint time (from the token style), so a theme
//! switch repaints without re-tokenising.

use egui::{Color32, FontId, Visuals};

use crate::galley_cache::hash_one;

/// Colours and font of one editor.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorTheme {
    pub font: FontId,
    pub background: Color32,
    pub foreground: Color32,
    pub gutter_background: Color32,
    pub line_number: Color32,
    pub line_number_active: Color32,
    pub current_line: Color32,
    pub selection: Color32,
    pub selection_unfocused: Color32,
    pub caret: Color32,
    pub caret_width: f32,
    pub find_match: Color32,
    pub find_match_current: Color32,
    pub preedit_underline: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub info: Color32,
    pub hint: Color32,
    /// Foreground per token type (index = the token type of the style; see `quire_core` tokens).
    /// Missing entries use `foreground`.
    pub token_colors: Vec<Color32>,
    /// Caret blink on/off durations in seconds; blinking stops after `blink_idle_stop` seconds idle.
    pub blink_on: f32,
    pub blink_off: f32,
    pub blink_idle_stop: f32,
}

impl EditorTheme {
    /// Defaults derived from egui visuals.
    pub fn from_visuals(v: &Visuals) -> Self {
        let fg = v.text_color();
        Self {
            font: FontId::monospace(13.0),
            background: v.extreme_bg_color,
            foreground: fg,
            gutter_background: v.extreme_bg_color,
            line_number: v.weak_text_color(),
            line_number_active: fg,
            current_line: v.faint_bg_color,
            selection: v.selection.bg_fill,
            selection_unfocused: v.selection.bg_fill.gamma_multiply(0.5),
            caret: v.text_cursor.stroke.color,
            caret_width: v.text_cursor.stroke.width.max(1.0),
            find_match: Color32::from_rgba_unmultiplied(234, 92, 0, 90),
            find_match_current: Color32::from_rgba_unmultiplied(234, 92, 0, 170),
            preedit_underline: fg,
            error: v.error_fg_color,
            warning: v.warn_fg_color,
            info: Color32::from_rgb(55, 148, 255),
            hint: v.weak_text_color(),
            token_colors: default_token_colors(v.dark_mode, fg),
            blink_on: v.text_cursor.on_duration,
            blink_off: v.text_cursor.off_duration,
            blink_idle_stop: 5.0,
        }
    }

    /// Colour of a token type.
    pub fn token_color(&self, token_type: usize) -> Color32 {
        self.token_colors
            .get(token_type)
            .copied()
            .unwrap_or(self.foreground)
    }

    /// Hash of everything that affects line galleys (font is keyed separately).
    pub(crate) fn galley_hash(&self) -> u64 {
        hash_one((self.foreground, &self.token_colors))
    }
}

/// A VS Code Dark+/Light+-like palette indexed by `quire_core::syntax::TokenType`.
fn default_token_colors(dark: bool, fg: Color32) -> Vec<Color32> {
    use quire_core::syntax::TokenType as T;
    let rgb = |d: (u8, u8, u8), l: (u8, u8, u8)| {
        let (r, g, b) = if dark { d } else { l };
        Color32::from_rgb(r, g, b)
    };
    let keyword = rgb((86, 156, 214), (0, 0, 255));
    let control = rgb((197, 134, 192), (175, 0, 219));
    let ty = rgb((78, 201, 176), (38, 127, 153));
    let func = rgb((220, 220, 170), (121, 94, 38));
    let var = rgb((156, 220, 254), (0, 16, 128));
    let string = rgb((206, 145, 120), (163, 21, 21));
    let number = rgb((181, 206, 168), (9, 134, 88));
    let comment = rgb((106, 153, 85), (0, 128, 0));
    let constant = rgb((79, 193, 255), (0, 112, 193));
    let tag = rgb((86, 156, 214), (128, 0, 0));
    let attr = rgb((156, 220, 254), (229, 0, 0));
    let punct = rgb((212, 212, 212), (0, 0, 0));
    let error = rgb((244, 71, 71), (205, 49, 49));
    T::ALL
        .iter()
        .map(|t| match t {
            T::Text | T::Embedded => fg,
            T::Namespace
            | T::Type
            | T::Class
            | T::Enum
            | T::Interface
            | T::Struct
            | T::TypeParameter => ty,
            T::Constructor => ty,
            T::Parameter | T::Variable | T::Property => var,
            T::EnumMember | T::Constant | T::Boolean => constant,
            T::Event | T::Function | T::Method | T::Macro | T::Decorator => func,
            T::Keyword => keyword,
            T::Modifier => keyword,
            T::Label => control,
            T::Comment => comment,
            T::String | T::StringSpecial => string,
            T::StringEscape | T::Regexp => rgb((215, 186, 125), (238, 0, 0)),
            T::Number => number,
            T::Operator | T::Punctuation | T::PunctuationBracket | T::PunctuationDelimiter => punct,
            T::Tag => tag,
            T::Attribute => attr,
            T::Error => error,
        })
        .collect()
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self::from_visuals(&Visuals::dark())
    }
}

/// User-visible strings of the widget (find bar, accessibility labels). The host fills them from
/// its catalog; the defaults are English.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorText {
    pub find_placeholder: String,
    pub replace_placeholder: String,
    pub find_label: String,
    pub replace_label: String,
    pub match_case: String,
    pub whole_word: String,
    pub regex: String,
    pub previous_match: String,
    pub next_match: String,
    pub replace_one: String,
    pub replace_all: String,
    pub close: String,
    /// `{current}` and `{total}` are substituted.
    pub match_count: String,
    pub no_results: String,
    /// Announced (AccessKit polite live region) when the accessibility page shifts to follow
    /// the caret; `{line}` is the 1-based line number.
    pub line_announcement: String,
    /// The suggest list's accessible name (VS Code "Suggest").
    pub suggest_label: String,
    /// Shown while a manual request waits (VS Code "Loading...").
    pub suggest_loading: String,
    /// Shown when a manual request found nothing (VS Code "No suggestions.").
    pub suggest_empty: String,
    /// The hover's "Loading..." (VS Code `modesContentHover.loading`).
    pub hover_loading: String,
    /// The hover parts' copy button (VS Code "Copy").
    pub hover_copy: String,
    /// The parameter hints' announcement, `{0}` the parameter (VS Code "{0}, hint").
    pub parameter_hint: String,
    /// The parameter hints' arrows (VS Code "Previous signature" / "Next signature").
    pub previous_signature: String,
    pub next_signature: String,
}

impl Default for EditorText {
    fn default() -> Self {
        Self {
            find_placeholder: "Find".into(),
            replace_placeholder: "Replace".into(),
            find_label: "Find".into(),
            replace_label: "Replace".into(),
            match_case: "Match Case".into(),
            whole_word: "Match Whole Word".into(),
            regex: "Use Regular Expression".into(),
            previous_match: "Previous Match".into(),
            next_match: "Next Match".into(),
            replace_one: "Replace".into(),
            replace_all: "Replace All".into(),
            close: "Close".into(),
            match_count: "{current} of {total}".into(),
            no_results: "No results".into(),
            line_announcement: "Line {line}".into(),
            suggest_label: "Suggest".into(),
            suggest_loading: "Loading...".into(),
            suggest_empty: "No suggestions.".into(),
            hover_loading: "Loading...".into(),
            hover_copy: "Copy".into(),
            parameter_hint: "{0}, hint".into(),
            previous_signature: "Previous signature".into(),
            next_signature: "Next signature".into(),
        }
    }
}
