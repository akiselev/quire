//! The ASCII fast path with a ligature code font (Q-E1): JetBrains Mono 2.304 (OFL-1.1; an ASCII
//! subset with its `calt` lookups, `tests/fonts/`). With ligatures off every character draws its own
//! glyph on the fast path; with them on the probe refuses the table and `<=` shapes as a ligature.

use std::sync::Arc;

use skelter_quire::LIGATURES_OFF;
use skelter_quire::shape::{LineShaper, MonoTable};
use skelter_text::{FontConfig, FontStore, RichText, TextStyle, TextSystem};

const JETBRAINS_MONO: &[u8] = include_bytes!("fonts/JetBrainsMono-Regular.subset.ttf");

fn fonts() -> (Arc<FontStore>, String) {
    let fonts = FontStore::new(FontConfig::testing());
    let families = fonts.register_font(JETBRAINS_MONO.to_vec());
    let family = families
        .into_iter()
        .find(|f| f.contains("JetBrains Mono"))
        .expect("JetBrains Mono registers");
    (fonts, family)
}

fn style(family: &str, features: &str) -> TextStyle {
    TextStyle {
        family: family.to_owned(),
        size: 13.0,
        features: features.to_owned(),
        wrap: false,
        ..TextStyle::default()
    }
}

/// The glyph ids `s` shapes to through skelter-text (the general path).
fn glyph_ids(text: &mut TextSystem, st: &TextStyle, s: &str) -> Vec<u32> {
    let layout = text.layout(&RichText::plain(s, st.clone()), None);
    layout
        .runs()
        .iter()
        .flat_map(|r| r.run.glyphs.iter().map(|g| g.id))
        .collect()
}

#[test]
fn ligatures_off_shape_one_glyph_per_ascii_character_on_the_fast_path() {
    let (fonts, family) = fonts();
    let mut text = TextSystem::new(fonts.clone());
    let st = style(&family, LIGATURES_OFF);
    let table = MonoTable::probe(&mut text, &st).expect("monospace with ligatures off");
    // every character's glyph is the one it shapes to alone
    for c in b' '..=b'~' {
        let alone = glyph_ids(&mut text, &st, &char::from(c).to_string());
        assert_eq!(
            alone,
            vec![table.glyphs[(c - b' ') as usize]],
            "{:?}",
            char::from(c)
        );
    }
    // and a line full of would-be ligatures takes the fast path, one glyph per character
    let line = "a <=> b || c |} ~~> d -> e != f";
    let mut sh = LineShaper::new(fonts, &family, 13.0);
    sh.set_features(LIGATURES_OFF);
    assert!(sh.mono().is_some());
    let s = sh.shape(line, &[]);
    assert!(s.fast);
    let ids: Vec<u32> = s
        .runs
        .iter()
        .flat_map(|r| r.run.glyphs.iter().map(|g| g.id))
        .collect();
    let want: Vec<u32> = line
        .bytes()
        .filter(|&b| b != b' ')
        .map(|b| table.glyphs[(b - b' ') as usize])
        .collect();
    assert_eq!(ids, want);
    // the general path agrees with features off
    assert_eq!(
        glyph_ids(&mut text, &st, "<=>"),
        b"<=>"
            .iter()
            .map(|&b| table.glyphs[(b - b' ') as usize])
            .collect::<Vec<_>>()
    );
}

#[test]
fn ligatures_on_refuse_the_table_and_draw_the_ligature() {
    let (fonts, family) = fonts();
    let mut text = TextSystem::new(fonts.clone());
    let off = MonoTable::probe(&mut text, &style(&family, LIGATURES_OFF)).unwrap();
    let on = style(&family, "");
    // the bug: shaped as one string, the 95 characters already contain ligature pieces
    let all: String = (b' '..=b'~').map(char::from).collect();
    let together = glyph_ids(&mut text, &on, &all);
    assert_eq!(together.len(), 95, "same advances: the old probe accepted it");
    assert_ne!(
        together,
        off.glyphs.to_vec(),
        "calt rewrites some of the 95 in order"
    );
    // now the probe refuses, and lines take the general path with the font's ligatures
    assert!(MonoTable::probe(&mut text, &on).is_none());
    let mut sh = LineShaper::new(fonts, &family, 13.0);
    sh.set_features("");
    assert!(sh.mono().is_none());
    let s = sh.shape("x <= y", &[]);
    assert!(!s.fast);
    let lig = glyph_ids(&mut text, &on, "<=");
    let plain = vec![
        off.glyphs[(b'<' - b' ') as usize],
        off.glyphs[(b'=' - b' ') as usize],
    ];
    assert_ne!(lig, plain, "<= is a ligature with the font's features");
    let drawn: Vec<u32> = s
        .runs
        .iter()
        .flat_map(|r| r.run.glyphs.iter().map(|g| g.id))
        .collect();
    assert!(
        lig.iter().all(|g| drawn.contains(g)),
        "{drawn:?} draws {lig:?}"
    );
    // caret stops stay per character on the ligature
    assert_eq!(s.stops.len(), "x <= y".len() + 1);
    assert!(s.stops.windows(2).all(|w| w[0].1 < w[1].1), "{:?}", s.stops);
    // switching features back re-probes
    sh.set_features(LIGATURES_OFF);
    assert!(sh.mono().is_some());
    assert!(sh.shape("x <= y", &[]).fast);
}
