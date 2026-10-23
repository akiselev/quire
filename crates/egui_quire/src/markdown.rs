//! A minimal Markdown renderer for hovers and parameter hints: paragraphs, headings (as strong
//! text), bullet and numbered lists, block quotes, horizontal rules, fenced and indented code
//! (monospace, no syntax colouring: the widget has no highlighter of its own), and inline code
//! spans, strong, emphasis, links and backslash escapes. Raw HTML is shown as text. A clicked
//! link is returned for the host (`EditorEvent::HoverLinkActivated`).

use egui::{Color32, FontId, RichText, Ui};

/// A block of the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    /// Inline text (lines joined with spaces), with a heading level or list marker.
    Para { text: String, kind: ParaKind },
    /// Code, with the fence's language tag.
    Code { lang: String, text: String },
    /// `---`, `***`, `___`.
    Rule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParaKind {
    Plain,
    Heading,
    Bullet(String),
    Quote,
}

/// An inline run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Inline {
    Text {
        text: String,
        strong: bool,
        emphasis: bool,
    },
    Code(String),
    Link {
        text: String,
        href: String,
    },
}

fn is_rule(t: &str) -> bool {
    let t: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3
        && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

fn list_marker(t: &str) -> Option<(String, &str)> {
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some(("•".into(), rest));
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((format!("{}.", &t[..digits]), r));
        }
    }
    None
}

/// Splits Markdown source into blocks.
pub(crate) fn blocks(src: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut para: Option<(String, ParaKind)> = None;
    let flush = |para: &mut Option<(String, ParaKind)>, out: &mut Vec<Block>| {
        if let Some((text, kind)) = para.take()
            && !text.trim().is_empty()
        {
            out.push(Block::Para {
                text: text.trim().to_string(),
                kind,
            });
        }
    };
    let mut lines = src.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        let fence = ["```", "~~~"].into_iter().find(|f| t.starts_with(f));
        if let Some(f) = fence {
            flush(&mut para, &mut out);
            let lang = t[f.len()..].trim().to_string();
            let mut code = Vec::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with(f) {
                    break;
                }
                code.push(l);
            }
            out.push(Block::Code {
                lang,
                text: code.join("\n"),
            });
            continue;
        }
        if t.is_empty() {
            flush(&mut para, &mut out);
            continue;
        }
        if para.is_none() && (line.starts_with("    ") || line.starts_with('\t')) {
            let mut code = vec![line.strip_prefix("    ").unwrap_or(&line[1..])];
            while let Some(l) = lines.peek() {
                if l.starts_with("    ") || l.starts_with('\t') {
                    code.push(l.strip_prefix("    ").unwrap_or(&l[1..]));
                    lines.next();
                } else {
                    break;
                }
            }
            out.push(Block::Code {
                lang: String::new(),
                text: code.join("\n"),
            });
            continue;
        }
        if is_rule(t) {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
            continue;
        }
        if t.starts_with('#') {
            let level = t.chars().take_while(|&c| c == '#').count();
            if level <= 6 && t[level..].starts_with(' ') {
                flush(&mut para, &mut out);
                out.push(Block::Para {
                    text: t[level..].trim().trim_end_matches('#').trim().to_string(),
                    kind: ParaKind::Heading,
                });
                continue;
            }
        }
        if let Some((marker, rest)) = list_marker(t) {
            flush(&mut para, &mut out);
            para = Some((rest.to_string(), ParaKind::Bullet(marker)));
            continue;
        }
        if let Some(rest) = t.strip_prefix('>') {
            if !matches!(para, Some((_, ParaKind::Quote))) {
                flush(&mut para, &mut out);
                para = Some((String::new(), ParaKind::Quote));
            }
            if let Some((p, _)) = &mut para {
                p.push(' ');
                p.push_str(rest.trim());
            }
            continue;
        }
        match &mut para {
            Some((p, _)) => {
                // A hard break (two trailing spaces or a backslash) keeps the line apart.
                if p.ends_with("  ") || p.ends_with('\\') {
                    let trimmed = p.trim_end_matches(['\\', ' ']).len();
                    p.truncate(trimmed);
                    p.push('\n');
                } else {
                    p.push(' ');
                }
                p.push_str(t);
            }
            None => para = Some((t.to_string(), ParaKind::Plain)),
        }
    }
    flush(&mut para, &mut out);
    out
}

/// Parses inline Markdown.
pub(crate) fn inlines(src: &str) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    let (mut strong, mut emphasis) = (false, false);
    let mut buf = String::new();
    let push = |out: &mut Vec<Inline>, buf: &mut String, strong: bool, emphasis: bool| {
        if !buf.is_empty() {
            out.push(Inline::Text {
                text: std::mem::take(buf),
                strong,
                emphasis,
            });
        }
    };
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() => {
                buf.push(chars[i + 1]);
                i += 2;
            }
            '`' => {
                let ticks = chars[i..].iter().take_while(|&&x| x == '`').count();
                let rest: String = chars[i + ticks..].iter().collect();
                let fence = "`".repeat(ticks);
                if let Some(end) = rest.find(&fence) {
                    push(&mut out, &mut buf, strong, emphasis);
                    let code = &rest[..end];
                    let code = if code.starts_with(' ') && code.ends_with(' ') && code.len() > 1 {
                        &code[1..code.len() - 1]
                    } else {
                        code
                    };
                    out.push(Inline::Code(code.to_string()));
                    i += ticks + rest[..end].chars().count() + ticks;
                } else {
                    buf.push_str(&fence);
                    i += ticks;
                }
            }
            '*' | '_' => {
                let double = i + 1 < chars.len() && chars[i + 1] == c;
                // `_` inside a word is literal (snake_case).
                let intraword = c == '_'
                    && i > 0
                    && chars[i - 1].is_alphanumeric()
                    && chars.get(i + if double { 2 } else { 1 }).is_some_and(|x| x.is_alphanumeric());
                if intraword {
                    buf.push(c);
                    i += 1;
                    continue;
                }
                push(&mut out, &mut buf, strong, emphasis);
                if double {
                    strong = !strong;
                    i += 2;
                } else {
                    emphasis = !emphasis;
                    i += 1;
                }
            }
            '[' => {
                let rest: String = chars[i + 1..].iter().collect();
                let link = rest.find("](").and_then(|close| {
                    let after = &rest[close + 2..];
                    after.find(')').map(|end| (close, end))
                });
                match link {
                    Some((close, end)) => {
                        push(&mut out, &mut buf, strong, emphasis);
                        let text = &rest[..close];
                        let target = rest[close + 2..close + 2 + end].trim();
                        // `[text](href "title")`: drop the title.
                        let href = target.split_whitespace().next().unwrap_or("");
                        out.push(Inline::Link {
                            text: text.to_string(),
                            href: href.trim_matches(['<', '>']).to_string(),
                        });
                        i += 1 + close + 2 + end + 1;
                    }
                    None => {
                        buf.push(c);
                        i += 1;
                    }
                }
            }
            _ => {
                buf.push(c);
                i += 1;
            }
        }
    }
    push(&mut out, &mut buf, strong, emphasis);
    out
}

/// Colours and fonts for rendering.
#[derive(Clone, Debug)]
pub(crate) struct Style {
    pub code_font: FontId,
    pub code_bg: Color32,
    pub dim: Color32,
}

/// Renders `src`; returns the href of a link clicked this frame.
pub(crate) fn show(ui: &mut Ui, src: &str, style: &Style) -> Option<String> {
    let mut clicked = None;
    for b in blocks(src) {
        match b {
            Block::Rule => {
                ui.separator();
            }
            Block::Code { text, .. } => {
                egui::Frame::new()
                    .fill(style.code_bg)
                    .inner_margin(4.0)
                    .corner_radius(3.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(text).font(style.code_font.clone()))
                                .extend(),
                        );
                    });
            }
            Block::Para { text, kind } => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    match &kind {
                        ParaKind::Bullet(m) => {
                            ui.label(format!("{m} "));
                        }
                        ParaKind::Quote => {
                            ui.label(RichText::new("▎ ").color(style.dim));
                        }
                        _ => {}
                    }
                    for run in inlines(&text) {
                        if let Some(h) = show_inline(ui, run, &kind, style) {
                            clicked = Some(h);
                        }
                    }
                });
            }
        }
    }
    clicked
}

fn show_inline(ui: &mut Ui, run: Inline, kind: &ParaKind, style: &Style) -> Option<String> {
    match run {
        Inline::Text {
            text,
            strong,
            emphasis,
        } => {
            let mut rt = RichText::new(text);
            if strong || *kind == ParaKind::Heading {
                rt = rt.strong();
            }
            if emphasis {
                rt = rt.italics();
            }
            if *kind == ParaKind::Quote {
                rt = rt.color(style.dim);
            }
            ui.label(rt);
            None
        }
        Inline::Code(c) => {
            ui.label(
                RichText::new(c)
                    .font(style.code_font.clone())
                    .background_color(style.code_bg),
            );
            None
        }
        Inline::Link { text, href } => {
            let r = ui.link(text).on_hover_text(href.clone());
            r.clicked().then_some(href)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_split() {
        let b = blocks("# Title\n\nsome *text*\ncontinued\n\n```rust\nfn x() {}\n```\n---\n- one\n- two");
        assert_eq!(
            b,
            vec![
                Block::Para { text: "Title".into(), kind: ParaKind::Heading },
                Block::Para { text: "some *text* continued".into(), kind: ParaKind::Plain },
                Block::Code { lang: "rust".into(), text: "fn x() {}".into() },
                Block::Rule,
                Block::Para { text: "one".into(), kind: ParaKind::Bullet("•".into()) },
                Block::Para { text: "two".into(), kind: ParaKind::Bullet("•".into()) },
            ]
        );
    }

    #[test]
    fn inline_runs() {
        let r = inlines("a `b` **c** _d_ snake_case [e](http://x \"t\") \\*f\\*");
        let t = |s: &str, strong, emphasis| Inline::Text { text: s.into(), strong, emphasis };
        assert_eq!(
            r,
            vec![
                t("a ", false, false),
                Inline::Code("b".into()),
                t(" ", false, false),
                t("c", true, false),
                t(" ", false, false),
                t("d", false, true),
                t(" snake_case ", false, false),
                Inline::Link { text: "e".into(), href: "http://x".into() },
                t(" *f*", false, false),
            ]
        );
    }

    #[test]
    fn unclosed_markers_are_text() {
        assert_eq!(
            inlines("a `b [c"),
            vec![Inline::Text { text: "a `b [c".into(), strong: false, emphasis: false }]
        );
    }
}
