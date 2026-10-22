//! Visual columns: VS Code's `CursorColumns.visibleColumnFromColumn` and
//! `columnFromVisibleColumn` as quire counts them. A tab advances to the next multiple of
//! `tab_size`, a wide East Asian character or emoji takes two columns, anything else one; each
//! grapheme cluster is one step, sized by its first char. Box selection, `ColumnSelect` and the
//! widget's hit testing use these, so the widget and the commands agree.

use unicode_segmentation::UnicodeSegmentation;

/// Whether a char takes two columns (CJK wide/fullwidth and most emoji; an approximation of
/// VS Code's `isFullWidthCharacter || isEmojiImprecise`).
pub(crate) fn is_wide(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF |
        0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 |
        0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
}

/// The visual column after a cluster starting with `c` at visual column `vis`.
pub(crate) fn next_visible(c: char, vis: u32, tab: u32) -> u32 {
    if c == '\t' {
        (vis / tab + 1) * tab
    } else if is_wide(c) {
        vis + 2
    } else {
        vis + 1
    }
}

/// The visual column of byte offset `byte` in `line` (a line without its line break). Only the
/// clusters that end at or before `byte` count; `byte` past the end gives the line's width.
pub fn visual_col(line: &str, byte: usize, tab_size: u32) -> u32 {
    let tab = tab_size.max(1);
    let mut v = 0;
    for (b, g) in line.grapheme_indices(true) {
        if b + g.len() > byte {
            break;
        }
        v = next_visible(g.chars().next().unwrap_or(' '), v, tab);
    }
    v
}

/// The byte offset in `line` nearest to visual column `col`, on a grapheme cluster boundary
/// (ties go left; a column inside a tab or wide character snaps to the nearer side). A column
/// past the end gives `line.len()`. The inverse of [`visual_col`] on cluster boundaries.
pub fn byte_at_visual_col(line: &str, col: u32, tab_size: u32) -> usize {
    let tab = tab_size.max(1);
    if col == 0 {
        return 0;
    }
    let (mut before_vis, mut before_byte) = (0u32, 0usize);
    for (b, g) in line.grapheme_indices(true) {
        let after_vis = next_visible(g.chars().next().unwrap_or(' '), before_vis, tab);
        if after_vis >= col {
            return if after_vis - col < col - before_vis {
                b + g.len()
            } else {
                before_byte
            };
        }
        before_vis = after_vis;
        before_byte = b + g.len();
    }
    line.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_round_trip() {
        let line = "\tab\u{4e2d}e\u{301}x";
        // tab to 4, a, b, wide char (2), e + combining accent (one cluster), x
        let cols: Vec<u32> = [0, 1, 2, 3, 6, 9, 10]
            .iter()
            .map(|&b| visual_col(line, b, 4))
            .collect();
        assert_eq!(cols, [0, 4, 5, 6, 8, 9, 10]);
        for (b, c) in [(0, 0), (1, 4), (2, 5), (3, 6), (6, 8), (9, 9), (10, 10)] {
            assert_eq!(byte_at_visual_col(line, c, 4), b, "col {c}");
        }
        // Inside a tab: nearer side, ties left. Past the end: the line's end.
        assert_eq!(byte_at_visual_col(line, 1, 4), 0);
        assert_eq!(byte_at_visual_col(line, 2, 4), 0);
        assert_eq!(byte_at_visual_col(line, 3, 4), 1);
        assert_eq!(byte_at_visual_col(line, 7, 4), 3);
        assert_eq!(byte_at_visual_col(line, 99, 4), line.len());
        // Inside a cluster: the cluster's start.
        assert_eq!(visual_col(line, 7, 4), 8);
    }
}
