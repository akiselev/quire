//! Injected text: text shown in a line that is not in the document (inlay hints, an IME
//! preedit), and how columns map between the document line and the line as shown, wrapped
//! rows included (VS Code `ModelLineProjectionData`, `editor/common/modelLineProjectionData.ts`
//! at microsoft/vscode `bdce63db587665a91d1f1237b807d43471f0d9bc`, translated literally).
//!
//! Offsets are UTF-8 bytes of the line (VS Code: UTF-16 units). The *input* is the document
//! line, the *output* the shown rows: the input with the injections spliced in, cut at the
//! break offsets, rows after the first indented by `wrapped_text_indent_length` columns.
//!
//! Translated from Visual Studio Code. Copyright (c) Microsoft Corporation. All rights reserved.
//! Licensed under the MIT License (see `THIRD_PARTY_NOTICES.md`).

/// Where the caret may rest next to injected text (VS Code `InjectedTextCursorStops`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum CursorStops {
    /// On both sides.
    #[default]
    Both,
    /// After it only.
    Right,
    /// Before it only.
    Left,
    /// On neither side (the caret skips over it).
    None,
}

impl CursorStops {
    fn right(self) -> bool {
        matches!(self, CursorStops::Right | CursorStops::Both)
    }

    fn left(self) -> bool {
        matches!(self, CursorStops::Left | CursorStops::Both)
    }
}

/// Which side a position at injected text or a row boundary prefers (VS Code
/// `PositionAffinity`).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum PositionAffinity {
    /// Before injected text at the position; a row boundary belongs to the next row.
    #[default]
    None,
    /// The previous row's end at a row boundary.
    Left,
    /// After injected text at the position.
    Right,
    /// Left of injected text, otherwise as `None`.
    LeftOfInjectedText,
    /// Right of injected text, otherwise as `None`.
    RightOfInjectedText,
}

/// One injection: its length in bytes and its cursor stops.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Injection {
    /// Where it goes (a byte offset of the document line).
    pub offset: usize,
    /// Its text's length in bytes.
    pub len: usize,
    /// Where the caret may rest next to it.
    pub cursor_stops: CursorStops,
}

/// A position in the shown rows: the row and the byte offset in it (indent included).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct OutputPosition {
    /// The row.
    pub line: usize,
    /// The offset in the row, the wrapping indent counted.
    pub offset: usize,
}

impl std::fmt::Display for OutputPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.offset)
    }
}

/// A document line's injections and row breaks (VS Code `ModelLineProjectionData`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct LineProjection {
    /// Injections sorted by offset (several may share one).
    pub injections: Vec<Injection>,
    /// Where each row ends, in offsets of the input with injections; the last is the length
    /// of the input with injections.
    pub break_offsets: Vec<usize>,
    /// The wrapping indent of rows after the first.
    pub wrapped_text_indent_length: usize,
}

impl LineProjection {
    /// A projection of a line of `len` bytes with `injections` and no wrapping.
    pub fn unwrapped(len: usize, injections: Vec<Injection>) -> Self {
        let total = len + injections.iter().map(|i| i.len).sum::<usize>();
        LineProjection {
            injections,
            break_offsets: vec![total],
            wrapped_text_indent_length: 0,
        }
    }

    /// `getOutputLineCount`.
    pub fn output_line_count(&self) -> usize {
        self.break_offsets.len()
    }

    /// `getMinOutputOffset`.
    pub fn min_output_offset(&self, line: usize) -> usize {
        if line > 0 {
            self.wrapped_text_indent_length
        } else {
            0
        }
    }

    /// `getLineLength`.
    pub fn line_length(&self, line: usize) -> usize {
        let start = if line > 0 {
            self.break_offsets[line - 1]
        } else {
            0
        };
        let mut len = self.break_offsets[line] - start;
        if line > 0 {
            len += self.wrapped_text_indent_length;
        }
        len
    }

    /// `getMaxOutputOffset`.
    pub fn max_output_offset(&self, line: usize) -> usize {
        self.line_length(line)
    }

    /// `translateToInputOffset`: the document offset shown at `offset` of row `line` (inside
    /// injected text: the injection's offset).
    pub fn translate_to_input_offset(&self, line: usize, mut offset: usize) -> usize {
        if line > 0 {
            offset = offset.saturating_sub(self.wrapped_text_indent_length);
        }
        let mut input = if line == 0 {
            offset
        } else {
            self.break_offsets[line - 1] + offset
        };
        for inj in &self.injections {
            if input > inj.offset {
                if input < inj.offset + inj.len {
                    input = inj.offset;
                } else {
                    input -= inj.len;
                }
            } else {
                break;
            }
        }
        input
    }

    /// `translateToOutputPosition`: where document offset `input` shows.
    pub fn translate_to_output_position(
        &self,
        input: usize,
        affinity: PositionAffinity,
    ) -> OutputPosition {
        let mut with = input;
        for inj in &self.injections {
            if input < inj.offset {
                break;
            }
            if affinity != PositionAffinity::Right && input == inj.offset {
                break;
            }
            with += inj.len;
        }
        self.offset_with_injections_to_output_position(with, affinity)
    }

    fn offset_with_injections_to_output_position(
        &self,
        with: usize,
        affinity: PositionAffinity,
    ) -> OutputPosition {
        let mut low: isize = 0;
        let mut high: isize = self.break_offsets.len() as isize - 1;
        let mut mid: isize = 0;
        let mut mid_start = 0usize;
        while low <= high {
            mid = low + (high - low) / 2;
            let mid_stop = self.break_offsets[mid as usize];
            mid_start = if mid > 0 {
                self.break_offsets[mid as usize - 1]
            } else {
                0
            };
            if affinity == PositionAffinity::Left {
                if with <= mid_start {
                    high = mid - 1;
                } else if with > mid_stop {
                    low = mid + 1;
                } else {
                    break;
                }
            } else if with < mid_start {
                high = mid - 1;
            } else if with >= mid_stop {
                low = mid + 1;
            } else {
                break;
            }
        }
        let mut offset = with as isize - mid_start as isize;
        if mid > 0 {
            offset += self.wrapped_text_indent_length as isize;
        }
        OutputPosition {
            line: mid as usize,
            offset: offset.max(0) as usize,
        }
    }

    /// `normalizeOutputPosition`: a shown position moved off injected text the way `affinity`
    /// and the cursor stops say, and across a row boundary for `Left`/`Right`.
    pub fn normalize_output_position(
        &self,
        line: usize,
        offset: usize,
        affinity: PositionAffinity,
    ) -> OutputPosition {
        if !self.injections.is_empty() {
            let with = self.output_position_to_offset_with_injections(line, offset);
            let normalized = self.normalize_around_injections(with, affinity);
            if normalized != with {
                return self.offset_with_injections_to_output_position(normalized, affinity);
            }
        }
        match affinity {
            PositionAffinity::Left if line > 0 && offset == self.min_output_offset(line) => {
                return OutputPosition {
                    line: line - 1,
                    offset: self.max_output_offset(line - 1),
                };
            }
            PositionAffinity::Right
                if line + 1 < self.output_line_count()
                    && offset == self.max_output_offset(line) =>
            {
                return OutputPosition {
                    line: line + 1,
                    offset: self.min_output_offset(line + 1),
                };
            }
            _ => {}
        }
        OutputPosition { line, offset }
    }

    fn output_position_to_offset_with_injections(&self, line: usize, mut offset: usize) -> usize {
        if line > 0 {
            offset = offset.saturating_sub(self.wrapped_text_indent_length);
        }
        (if line > 0 {
            self.break_offsets[line - 1]
        } else {
            0
        }) + offset
    }

    fn normalize_around_injections(&self, with: usize, affinity: PositionAffinity) -> usize {
        let Some((idx, start, len)) = self.injected_text_at(with) else {
            return with;
        };
        let inj = &self.injections;
        match affinity {
            PositionAffinity::None => {
                if with == start + len && inj[idx].cursor_stops.right() {
                    start + len
                } else {
                    let mut result = start;
                    if inj[idx].cursor_stops.left() {
                        return result;
                    }
                    let mut i = idx as isize - 1;
                    while i >= 0 && inj[i as usize].offset == inj[idx].offset {
                        if inj[i as usize].cursor_stops.right() {
                            break;
                        }
                        result -= inj[i as usize].len;
                        if inj[i as usize].cursor_stops.left() {
                            break;
                        }
                        i -= 1;
                    }
                    result
                }
            }
            PositionAffinity::Right | PositionAffinity::RightOfInjectedText => {
                let mut result = start + len;
                let mut i = idx;
                while i + 1 < inj.len() && inj[i + 1].offset == inj[i].offset {
                    result += inj[i + 1].len;
                    i += 1;
                }
                result
            }
            PositionAffinity::Left | PositionAffinity::LeftOfInjectedText => {
                let mut result = start;
                let mut i = idx;
                while i >= 1 && inj[i - 1].offset == inj[i].offset {
                    result -= inj[i - 1].len;
                    i -= 1;
                }
                result
            }
        }
    }

    /// The injection shown at `offset` of row `line` (`getInjectedText`), by index.
    pub fn injected_text(&self, line: usize, offset: usize) -> Option<usize> {
        let with = self.output_position_to_offset_with_injections(line, offset);
        self.injected_text_at(with).map(|(i, _, _)| i)
    }

    /// `getInjectedTextAtOffset`: (index, start in the input with injections, length).
    fn injected_text_at(&self, with: usize) -> Option<(usize, usize, usize)> {
        let mut before = 0;
        for (i, inj) in self.injections.iter().enumerate() {
            let start = inj.offset + before;
            let end = start + inj.len;
            if start > with {
                break;
            }
            if with <= end {
                return Some((i, start, inj.len));
            }
            before += inj.len;
        }
        None
    }

    /// The injection strictly inside which output offset `with` (of the input with injections)
    /// lies, by index: hit tests on injected text.
    pub fn injection_containing(&self, with: usize) -> Option<usize> {
        let mut before = 0;
        for (i, inj) in self.injections.iter().enumerate() {
            let start = inj.offset + before;
            if start > with {
                break;
            }
            if with < start + inj.len {
                return Some(i);
            }
            before += inj.len;
        }
        None
    }

    /// Where injection `i` starts in the input with injections.
    pub fn injection_start(&self, i: usize) -> usize {
        self.injections[i].offset + self.injections[..i].iter().map(|j| j.len).sum::<usize>()
    }
}
