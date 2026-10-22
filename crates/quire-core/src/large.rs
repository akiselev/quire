//! The large-file policy (ARCH-QUIRE §3.4): past Monaco's thresholds, tokenisation, folding and
//! bracket matching turn off and the document reports [`Degraded`] so the host can say so.
//!
//! As in Monaco (`TextModel._isTooLargeForTokenization`), the decision is taken when the text is
//! loaded (and on a reload), not on every edit.

use crate::text::Text;

/// Thresholds; the defaults are Monaco's (`LARGE_FILE_SIZE_THRESHOLD`,
/// `LARGE_FILE_LINE_COUNT_THRESHOLD`, `stopRenderingLineAfter`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LargeFilePolicy {
    /// Bytes above which the document is degraded (20 MB).
    pub max_bytes: usize,
    /// Lines above which the document is degraded (300 000).
    pub max_lines: usize,
    /// A line longer than this (bytes) is reported; the widget renders a prefix (10 000).
    pub max_line_len: usize,
}

impl Default for LargeFilePolicy {
    fn default() -> Self {
        LargeFilePolicy {
            max_bytes: 20 * 1024 * 1024,
            max_lines: 300_000,
            max_line_len: 10_000,
        }
    }
}

/// Why a document is degraded.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DegradedReason {
    /// The text is larger than [`LargeFilePolicy::max_bytes`].
    FileSize {
        /// Its size.
        bytes: usize,
    },
    /// The text has more lines than [`LargeFilePolicy::max_lines`].
    LineCount {
        /// Its line count.
        lines: usize,
    },
    /// The first line longer than [`LargeFilePolicy::max_line_len`].
    LongLine {
        /// The line (0-based).
        line: usize,
        /// Its length in bytes.
        len: usize,
    },
}

/// A document past the large-file thresholds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Degraded {
    /// The reasons, in the order above.
    pub reasons: Vec<DegradedReason>,
}

impl Degraded {
    /// Whether syntax features (tokenisation, folding, brackets) are off: true for file size and
    /// line count; a long line alone only limits rendering of that line.
    pub fn syntax_off(&self) -> bool {
        self.reasons
            .iter()
            .any(|r| !matches!(r, DegradedReason::LongLine { .. }))
    }
}

/// Checks `text` against `policy`.
pub fn check(text: &Text, policy: &LargeFilePolicy) -> Option<Degraded> {
    let mut reasons = Vec::new();
    let bytes = text.len_bytes();
    if bytes > policy.max_bytes {
        reasons.push(DegradedReason::FileSize { bytes });
    }
    let lines = text.len_lines();
    if lines > policy.max_lines {
        reasons.push(DegradedReason::LineCount { lines });
    }
    // Lines can only be long if the text is. One pass over the chunks (a rope lookup per line
    // costs about 0.5 µs, 50 ms at 100k lines; E1).
    if bytes > policy.max_line_len {
        let (mut line, mut len) = (0usize, 0usize);
        let mut found = None;
        'chunks: for chunk in text.chunks() {
            let mut rest = chunk;
            while let Some(i) = rest.find('\n') {
                len += i;
                if len > policy.max_line_len {
                    found = Some((line, len));
                    break 'chunks;
                }
                line += 1;
                len = 0;
                rest = &rest[i + 1..];
            }
            len += rest.len();
        }
        if found.is_none() && len > policy.max_line_len {
            found = Some((line, len));
        }
        if let Some((line, len)) = found {
            reasons.push(DegradedReason::LongLine { line, len });
        }
    }
    (!reasons.is_empty()).then_some(Degraded { reasons })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds() {
        let p = LargeFilePolicy {
            max_bytes: 100,
            max_lines: 3,
            max_line_len: 10,
        };
        assert_eq!(check(&Text::from_file("a\nb"), &p), None);
        let d = check(&Text::from_file("a\nb\nc\nd"), &p).unwrap();
        assert_eq!(d.reasons, vec![DegradedReason::LineCount { lines: 4 }]);
        assert!(d.syntax_off());
        let d = check(&Text::from_file("short\n0123456789abc"), &p).unwrap();
        assert_eq!(
            d.reasons,
            vec![DegradedReason::LongLine { line: 1, len: 13 }]
        );
        assert!(!d.syntax_off());
        let big = "x\n".repeat(60);
        let d = check(&Text::from_file(&big), &p).unwrap();
        assert!(matches!(
            d.reasons[0],
            DegradedReason::FileSize { bytes: 120 }
        ));
    }

    #[test]
    fn long_line_across_chunks_is_found_and_measured() {
        let p = LargeFilePolicy::default();
        let mut src = "short line\n".repeat(5000);
        let at = src.matches('\n').count();
        src.push_str(&"é".repeat(6000)); // 12 000 bytes, spans rope chunks
        src.push('\n');
        src.push_str(&"y".repeat(20_000)); // a later, longer one is not reported
        let d = check(&Text::from_file(&src), &p).unwrap();
        assert_eq!(
            d.reasons,
            vec![DegradedReason::LongLine {
                line: at,
                len: 12_000
            }]
        );
        // Exactly at the limit is fine; the last line (no break after it) counts too.
        let ok = format!("a\n{}", "z".repeat(10_000));
        assert_eq!(check(&Text::from_file(&ok), &p), None);
        let long_last = format!("a\n{}", "z".repeat(10_001));
        assert_eq!(
            check(&Text::from_file(&long_last), &p).unwrap().reasons,
            vec![DegradedReason::LongLine {
                line: 1,
                len: 10_001
            }]
        );
    }
}
