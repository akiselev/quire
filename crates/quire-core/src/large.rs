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
    // Lines can only be long if the text is.
    if bytes > policy.max_line_len {
        let mut start = 0;
        for line in 0..lines {
            let end = if line + 1 < lines {
                text.line_to_byte(line + 1) - 1
            } else {
                bytes
            };
            if end - start > policy.max_line_len {
                reasons.push(DegradedReason::LongLine {
                    line,
                    len: end - start,
                });
                break;
            }
            start = end + 1;
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
}
