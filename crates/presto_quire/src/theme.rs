//! Editor theme: colours, the editor font and caret timing, in presto's types.
//!
//! A plain struct the host fills from its own tokens. Token colours are resolved at record time
//! from a line's token styles (the shaped runs carry only a paint id), so a theme switch
//! re-records without reshaping.

use presto_base::color::Color;
use presto_base::time::Span;

/// Colours and font of one editor.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorTheme {
    /// CSS `font-family` list of the editor font.
    pub font_family: String,
    /// Points.
    pub font_size: f32,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    pub background: Color,
    pub foreground: Color,
    pub gutter_background: Color,
    pub line_number: Color,
    pub line_number_active: Color,
    pub current_line: Color,
    pub selection: Color,
    pub selection_unfocused: Color,
    pub caret: Color,
    pub caret_width: f32,
    pub find_match: Color,
    pub find_match_current: Color,
    pub preedit_underline: Color,
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    pub hint: Color,
    pub inlay_hint_foreground: Color,
    pub inlay_hint_background: Color,
    pub popup_background: Color,
    pub popup_border: Color,
    pub popup_foreground: Color,
    pub popup_selected: Color,
    pub scrollbar: Color,
    /// Foreground per token type (index = the token type of the style, `quire_core::syntax`).
    /// Missing entries use `foreground`.
    pub token_colors: Vec<Color>,
    /// Caret blink on and off durations; blinking stops after `blink_idle_stop` without input.
    pub blink_on: Span,
    pub blink_off: Span,
    pub blink_idle_stop: Span,
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

impl EditorTheme {
    /// A VS Code Dark+-like theme (`dark`) or Light+-like theme.
    pub fn new(dark: bool) -> Self {
        let pick = |d: Color, l: Color| if dark { d } else { l };
        let fg = pick(rgb(212, 212, 212), rgb(0, 0, 0));
        EditorTheme {
            font_family: "monospace".into(),
            font_size: 13.0,
            line_height: 1.4,
            background: pick(rgb(30, 30, 30), rgb(255, 255, 255)),
            foreground: fg,
            gutter_background: pick(rgb(30, 30, 30), rgb(255, 255, 255)),
            line_number: pick(rgb(133, 133, 133), rgb(35, 120, 147)),
            line_number_active: pick(rgb(198, 198, 198), rgb(11, 33, 108)),
            current_line: pick(rgba(255, 255, 255, 10), rgba(0, 0, 0, 10)),
            selection: pick(rgb(38, 79, 120), rgb(173, 214, 255)),
            selection_unfocused: pick(rgb(58, 61, 65), rgb(229, 235, 241)),
            caret: pick(rgb(174, 175, 173), rgb(0, 0, 0)),
            caret_width: 2.0,
            find_match: rgba(234, 92, 0, 90),
            find_match_current: rgba(234, 92, 0, 170),
            preedit_underline: fg,
            error: pick(rgb(241, 76, 76), rgb(229, 20, 0)),
            warning: pick(rgb(204, 167, 0), rgb(191, 136, 3)),
            info: rgb(55, 148, 255),
            hint: pick(rgb(150, 150, 150), rgb(110, 110, 110)),
            inlay_hint_foreground: rgb(150, 150, 150),
            inlay_hint_background: pick(rgba(77, 77, 77, 26), rgba(196, 196, 196, 26)),
            popup_background: pick(rgb(37, 37, 38), rgb(243, 243, 243)),
            popup_border: pick(rgb(69, 69, 69), rgb(200, 200, 200)),
            popup_foreground: fg,
            popup_selected: pick(rgb(4, 57, 94), rgb(214, 235, 255)),
            scrollbar: pick(rgba(121, 121, 121, 102), rgba(100, 100, 100, 102)),
            token_colors: default_token_colors(dark, fg),
            blink_on: Span::from_millis(500),
            blink_off: Span::from_millis(500),
            blink_idle_stop: Span::from_millis(5000),
        }
    }

    /// Colour of a token type.
    pub fn token_color(&self, token_type: usize) -> Color {
        self.token_colors
            .get(token_type)
            .copied()
            .unwrap_or(self.foreground)
    }

    /// Colour of a paint id recorded in a shaped line: 0 is the foreground, `t + 1` token type `t`.
    pub fn paint_color(&self, paint: u32) -> Color {
        match paint {
            0 => self.foreground,
            t => self.token_color(t as usize - 1),
        }
    }

    /// Rows are this tall (points).
    pub fn row_height(&self) -> f32 {
        (self.font_size * self.line_height).round()
    }

    /// (foreground, background) of an inlay hint.
    pub fn inlay_hint_colors(&self) -> (Color, Color) {
        (self.inlay_hint_foreground, self.inlay_hint_background)
    }
}

impl Default for EditorTheme {
    fn default() -> Self {
        EditorTheme::new(true)
    }
}

/// A VS Code Dark+/Light+-like palette indexed by `quire_core::syntax::TokenType` (the palette of
/// egui_quire's theme).
fn default_token_colors(dark: bool, fg: Color) -> Vec<Color> {
    use quire_core::syntax::TokenType as T;
    let c = |d: (u8, u8, u8), l: (u8, u8, u8)| {
        let (r, g, b) = if dark { d } else { l };
        rgb(r, g, b)
    };
    let keyword = c((86, 156, 214), (0, 0, 255));
    let control = c((197, 134, 192), (175, 0, 219));
    let ty = c((78, 201, 176), (38, 127, 153));
    let func = c((220, 220, 170), (121, 94, 38));
    let var = c((156, 220, 254), (0, 16, 128));
    let string = c((206, 145, 120), (163, 21, 21));
    let number = c((181, 206, 168), (9, 134, 88));
    let comment = c((106, 153, 85), (0, 128, 0));
    let constant = c((79, 193, 255), (0, 112, 193));
    let tag = c((86, 156, 214), (128, 0, 0));
    let attr = c((156, 220, 254), (229, 0, 0));
    let punct = c((212, 212, 212), (0, 0, 0));
    let error = c((244, 71, 71), (205, 49, 49));
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
            T::Keyword | T::Modifier => keyword,
            T::Label => control,
            T::Comment => comment,
            T::String | T::StringSpecial => string,
            T::StringEscape | T::Regexp => c((215, 186, 125), (238, 0, 0)),
            T::Number => number,
            T::Operator | T::Punctuation | T::PunctuationBracket | T::PunctuationDelimiter => punct,
            T::Tag => tag,
            T::Attribute => attr,
            T::Error => error,
        })
        .collect()
}
