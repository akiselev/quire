//! Runs the translated VS Code / Monaco editor tests in `fixtures/monaco/*.fixture`.
//!
//! Fixture format (one or more cases per file, `#` lines are comments):
//!
//! ```text
//! === <case name, usually the VS Code test title>
//! status: pass | fail-ours <reason> | fail-bug <reason>      (optional, default pass)
//! config: {"tabSize": 4, ...}                                 (EditorConfig as JSON, optional)
//! text: "first line\nsecond"                                  (JSON string)
//! moveTo 1 5 [select]                    VS Code 1-based line, UTF-16 column
//! setSelections 1 1 1 5; 2 1 2 1         selectionStart line col, position line col; ...
//! cmd cursorRight                        a VS Code command id (EditorCommand::from_vscode_id)
//! json {"id":"type","text":"x"}          an EditorCommand as JSON
//! type "abc"                             shorthand for the `type` command (keyboard: interceptors)
//! typeRaw "abc"                          replace selections without interceptors (VS Code `type(text)`)
//! time 1000                              host clock for the following commands (undo grouping)
//! expectText "..."
//! expectLine 2 "..."
//! expectSel 1 1 1 5; 2 1 2 1             selections in order (sorted by start)
//! expectCursor 1 5                       the primary selection is a cursor here
//! expectPrimarySel 1 1 1 5               the primary selection
//! expectHead 1 5                         the primary head
//! expectLineCount 3
//! repeat cursorWordLeft position pos 1 1   repeat a command, recording the head (or `deleted`:
//!                                        the deleted length) until a position (or `empty`)
//! expectStops "|a |b"                    the recorded stops as pipes in the original text
//! expectFindAll {query} => 1 1 1 4; …   all matches (VS Code TextModelSearch.findMatches)
//! expectFindNext 1 1 {query} => r|none   the match found from a position (`end`: text end)
//! expectFindPrev end {query} => r|none
//! setEOL crlf                            no-op (the in-memory text is LF)
//! columnSelect 4 15 [drag]             VS Code mouse column select to (line, mouseColumn)
//! setValue "..."                         replace the text (a reload)
//! expectMaxColumn 2 5                    VS Code getLineMaxColumn
//! expectSelCount 2                       number of selections
//! expectRanges 1 1 1 4; 2 1 2 4          ranges regardless of direction, sorted by start
//! wordSelect 1 5                         double-click word selection at a position
//! closeUndoGroup                         VS Code pushStackElement / pushUndoStop
//! ```
//!
//! A case with `status: fail-*` is expected to fail; if it passes, the harness says so (update
//! the fixture and `tests/VSCODE-MAP.md`).

use quire_core::{Document, EditorCommand, EditorConfig, EditorView, SelRange, Selection, Text};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
struct Case {
    file: String,
    name: String,
    status: String,
    config: Option<String>,
    text: String,
    steps: Vec<(usize, String)>,
}

fn parse_file(path: &Path) -> Vec<Case> {
    let src = std::fs::read_to_string(path).expect("read fixture");
    let mut cases = Vec::new();
    let mut cur: Option<Case> = None;
    for (i, line) in src.lines().enumerate() {
        let line_no = i + 1;
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(name) = t.strip_prefix("===") {
            if let Some(c) = cur.take() {
                cases.push(c);
            }
            cur = Some(Case {
                file: path.file_name().unwrap().to_string_lossy().into(),
                name: name.trim().to_string(),
                status: "pass".into(),
                ..Default::default()
            });
            continue;
        }
        let c = cur
            .as_mut()
            .unwrap_or_else(|| panic!("{}:{line_no}: step before ===", path.display()));
        if let Some(s) = t.strip_prefix("status:") {
            c.status = s.trim().to_string();
        } else if let Some(s) = t.strip_prefix("config:") {
            c.config = Some(s.trim().to_string());
        } else if let Some(s) = t.strip_prefix("text:") {
            c.text = serde_json::from_str(s.trim())
                .unwrap_or_else(|e| panic!("{}:{line_no}: text: {e}", path.display()));
        } else {
            c.steps.push((line_no, t.to_string()));
        }
    }
    if let Some(c) = cur.take() {
        cases.push(c);
    }
    cases
}

fn pos(text: &Text, line: usize, col: usize) -> usize {
    text.utf16_to_byte(line.saturating_sub(1), col.saturating_sub(1))
}

fn fmt_pos(text: &Text, b: usize) -> String {
    let (l, c) = text.byte_to_utf16(b);
    format!("{} {}", l + 1, c + 1)
}

fn fmt_sel(text: &Text, s: &Selection) -> String {
    s.ranges()
        .iter()
        .map(|r| format!("{} {}", fmt_pos(text, r.anchor), fmt_pos(text, r.head)))
        .collect::<Vec<_>>()
        .join("; ")
}

fn nums(s: &str) -> Vec<usize> {
    s.split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect()
}

fn parse_sels(text: &Text, s: &str) -> Vec<SelRange> {
    s.split(';')
        .map(|part| {
            let n = nums(part);
            assert_eq!(n.len(), 4, "selection needs 4 numbers: {part}");
            SelRange::new(pos(text, n[0], n[1]), pos(text, n[2], n[3]))
        })
        .collect()
}

fn run_case(c: &Case) -> Result<(), String> {
    let config: EditorConfig = match &c.config {
        Some(j) => serde_json::from_str(j).map_err(|e| format!("config: {e}"))?,
        None => EditorConfig::default(),
    };
    let mut doc = Document::new(&c.text, config);
    let mut view = EditorView::new(&doc);
    view.set_selection(&doc, Selection::point(0));
    let mut time: Option<u64> = None;
    // `repeat` (VS Code `testRepeatedActionAndExtractPositions`): command, record kind, stop.
    let mut repeat: Option<(String, String, String)> = None;
    for (line_no, step) in &c.steps {
        let (op, rest) = step.split_once(' ').unwrap_or((step.as_str(), ""));
        let rest = rest.trim();
        let err = |m: String| format!("line {line_no} `{step}`: {m}");
        let exec =
            |doc: &mut Document, view: &mut EditorView, cmd: EditorCommand, time: Option<u64>| {
                doc.execute(view, &cmd, time)
                    .map(|_| ())
                    .map_err(|e| err(format!("refused: {e}")))
            };
        match op {
            "moveTo" => {
                let n = nums(rest);
                let p = pos(doc.text(), n[0], n[1]);
                let sel = if rest.ends_with("select") {
                    let r = view.selection().primary();
                    Selection::single(r.anchor, p)
                } else {
                    Selection::point(p)
                };
                exec(
                    &mut doc,
                    &mut view,
                    EditorCommand::SetSelection { selection: sel },
                    time,
                )?;
            }
            "setSelections" => {
                let ranges = parse_sels(doc.text(), rest);
                let sel = Selection::new(ranges, 0);
                exec(
                    &mut doc,
                    &mut view,
                    EditorCommand::SetSelection { selection: sel },
                    time,
                )?;
            }
            "cmd" => {
                let cmd = EditorCommand::from_vscode_id(rest, 10)
                    .ok_or_else(|| err("unknown command id".into()))?;
                exec(&mut doc, &mut view, cmd, time)?;
            }
            "json" => {
                let cmd: EditorCommand =
                    serde_json::from_str(rest).map_err(|e| err(format!("json: {e}")))?;
                exec(&mut doc, &mut view, cmd, time)?;
            }
            "type" => {
                let s: String =
                    serde_json::from_str(rest).map_err(|e| err(format!("string: {e}")))?;
                exec(&mut doc, &mut view, EditorCommand::Type { text: s }, time)?;
            }
            "typeRaw" => {
                // VS Code `viewModel.type(text)` without the keyboard source: no interceptors.
                let s: String =
                    serde_json::from_str(rest).map_err(|e| err(format!("string: {e}")))?;
                view.sync(&doc);
                let ranges: Vec<_> = view.selection().ranges().to_vec();
                let changes = quire_core::ChangeSet::replace(
                    doc.text().len_bytes(),
                    ranges.iter().map(|r| (r.range(), s.as_str())),
                )
                .map_err(|e| err(format!("{e}")))?;
                let sel = view.selection().map(&changes, quire_core::Bias::After);
                let tr = quire_core::Transaction::new(doc.version(), changes)
                    .with_selection(sel)
                    .with_origin(quire_core::Origin::Input(quire_core::InputKind::Typing));
                doc.apply(tr, &mut view)
                    .map_err(|e| err(format!("refused: {e}")))?;
            }
            "time" => time = rest.parse().ok(),
            "closeUndoGroup" => {
                doc.close_undo_group();
            }
            "expectText" => {
                let s: String =
                    serde_json::from_str(rest).map_err(|e| err(format!("string: {e}")))?;
                let got = doc.text().to_string();
                if got != s {
                    return Err(err(format!("text: expected {s:?}, got {got:?}")));
                }
            }
            "expectLine" => {
                let (n, s) = rest
                    .split_once(' ')
                    .ok_or_else(|| err("expectLine N \"..\"".into()))?;
                let n: usize = n.parse().map_err(|_| err("line number".into()))?;
                let s: String = serde_json::from_str(s).map_err(|e| err(format!("string: {e}")))?;
                let got = doc.text().line(n - 1).to_string();
                if got != s {
                    return Err(err(format!("line {n}: expected {s:?}, got {got:?}")));
                }
            }
            "expectSel" => {
                let want = Selection::new(parse_sels(doc.text(), rest), 0);
                let got = view.selection();
                let same = want.len() == got.len()
                    && want
                        .ranges()
                        .iter()
                        .zip(got.ranges())
                        .all(|(a, b)| a.anchor == b.anchor && a.head == b.head);
                if !same {
                    return Err(err(format!(
                        "selection: expected {}, got {}",
                        fmt_sel(doc.text(), &want),
                        fmt_sel(doc.text(), got)
                    )));
                }
            }
            "setEOL" => {
                // The in-memory text is always LF; the file line ending does not change positions.
            }
            "expectFindAll" | "expectFindNext" | "expectFindPrev" => {
                let (lhs, want) = rest
                    .split_once(" => ")
                    .ok_or_else(|| err("missing =>".into()))?;
                let (pos_part, json) = match op {
                    "expectFindAll" => ("", lhs),
                    _ => {
                        let i = lhs.find('{').ok_or_else(|| err("query".into()))?;
                        (lhs[..i].trim(), &lhs[i..])
                    }
                };
                let q: serde_json::Value =
                    serde_json::from_str(json).map_err(|e| err(format!("query: {e}")))?;
                let seps = q["wordSeparators"].as_str();
                let query = quire_core::FindQuery {
                    search_string: q["searchString"].as_str().unwrap_or("").into(),
                    is_regex: q["isRegex"].as_bool().unwrap_or(false),
                    match_case: q["matchCase"].as_bool().unwrap_or(false),
                    whole_word: seps.is_some(),
                };
                let text = doc.text().clone();
                let want_ranges: Vec<(usize, usize)> =
                    if want.trim() == "none" || want.trim().is_empty() {
                        Vec::new()
                    } else {
                        parse_sels(&text, want)
                            .iter()
                            .map(|r| (r.from(), r.to()))
                            .collect()
                    };
                let fmt = |v: &[(usize, usize)]| {
                    v.iter()
                        .map(|(a, b)| format!("{} {}", fmt_pos(&text, *a), fmt_pos(&text, *b)))
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                let got: Vec<(usize, usize)> = if op == "expectFindAll" {
                    quire_core::find::find_all(&query, &text, seps.unwrap_or(""))
                        .map_err(|e| err(format!("{e}")))?
                        .into_iter()
                        .map(|r| (r.start, r.end))
                        .collect()
                } else {
                    let p = if pos_part == "end" {
                        text.len_bytes()
                    } else {
                        let n = nums(pos_part);
                        pos(&text, n[0], n[1])
                    };
                    let mut cfg = doc.config().clone();
                    if let Some(s) = seps {
                        cfg.word_separators = s.into();
                    }
                    let mut d = Document::new(&text.to_string(), cfg);
                    let mut v = EditorView::new(&d);
                    v.set_selection(&d, Selection::point(p));
                    d.execute(&mut v, &EditorCommand::Find { query }, None)
                        .map_err(|e| err(format!("{e}")))?;
                    if op == "expectFindPrev" {
                        v.set_selection(&d, Selection::point(p));
                        d.execute(&mut v, &EditorCommand::FindPrevious, None)
                            .map_err(|e| err(format!("{e}")))?;
                    }
                    let r = v.selection().primary();
                    vec![(r.from(), r.to())]
                };
                // No match: the cursor stays where it was.
                let want_ranges = if op != "expectFindAll" && want.trim() == "none" {
                    let n = if pos_part == "end" {
                        text.len_bytes()
                    } else {
                        let n = nums(pos_part);
                        pos(&text, n[0], n[1])
                    };
                    vec![(n, n)]
                } else {
                    want_ranges
                };
                if got != want_ranges {
                    return Err(err(format!(
                        "matches: expected {}, got {}",
                        fmt(&want_ranges),
                        fmt(&got)
                    )));
                }
            }
            "repeat" => {
                let mut it = rest.splitn(3, ' ');
                let cmd = it.next().unwrap_or_default().to_string();
                let rec = it.next().unwrap_or_default().to_string();
                let until = it.next().unwrap_or_default().to_string();
                repeat = Some((cmd, rec, until));
            }
            "expectStops" => {
                let want: String =
                    serde_json::from_str(rest).map_err(|e| err(format!("string: {e}")))?;
                let (cmd_id, rec, until) = repeat.take().ok_or_else(|| err("no repeat".into()))?;
                let cmd = EditorCommand::from_vscode_id(&cmd_id, 10)
                    .ok_or_else(|| err(format!("unknown command id {cmd_id}")))?;
                let original = doc.text().clone();
                let utf16_len = |t: &Text| t.to_string().encode_utf16().count();
                let mut stops: Vec<usize> = Vec::new(); // byte offsets in `original`
                for _ in 0..1000 {
                    exec(&mut doc, &mut view, cmd.clone(), time)?;
                    let stop_pos = match rec.as_str() {
                        "position" => {
                            let (l, c) = doc.text().byte_to_utf16(view.selection().primary().head);
                            original.utf16_to_byte(l, c)
                        }
                        _ => {
                            let deleted = utf16_len(&original) - utf16_len(doc.text());
                            original.utf16_to_byte(0, deleted)
                        }
                    };
                    stops.push(stop_pos);
                    let done = if until == "empty" {
                        doc.text().is_empty()
                    } else {
                        let n = nums(&until);
                        view.selection().primary().head == pos(doc.text(), n[0], n[1])
                    };
                    if done {
                        break;
                    }
                }
                stops.sort_unstable();
                let src = original.to_string();
                let mut got = String::new();
                let mut last = 0;
                for p in &stops {
                    got.push_str(&src[last..*p]);
                    got.push('|');
                    last = *p;
                }
                got.push_str(&src[last..]);
                if got != want {
                    return Err(err(format!("stops: expected {want:?}, got {got:?}")));
                }
            }
            "columnSelect" => {
                // VS Code `ColumnSelect` (mouse): to (line, mouseColumn); with `drag` the anchor
                // is the current column-select start, i.e. the primary's anchor.
                let n = nums(rest);
                let (hl, hc) = (n[0] - 1, n[1] as u32 - 1);
                let (al, ac) = if rest.ends_with("drag") {
                    let a = view.selection().primary().anchor;
                    let t = doc.text();
                    let line = t.byte_to_line(a);
                    let tab = doc.config().tab_size.max(1);
                    let mut vis = 0u32;
                    for c in t.slice(t.line_to_byte(line)..a).to_string().chars() {
                        vis = if c == '\t' {
                            (vis / tab + 1) * tab
                        } else {
                            vis + 1
                        };
                    }
                    (line, vis)
                } else {
                    (hl, hc)
                };
                exec(
                    &mut doc,
                    &mut view,
                    EditorCommand::ColumnSelect {
                        anchor_line: al,
                        anchor_column: ac,
                        head_line: hl,
                        head_column: hc,
                    },
                    time,
                )?;
            }
            "setValue" => {
                // `model.setValue` as a reload: a diff against the new text.
                let s: String =
                    serde_json::from_str(rest).map_err(|e| err(format!("string: {e}")))?;
                let new = Text::from_file(&s);
                let changes = quire_core::ChangeSet::diff(doc.text(), &new);
                doc.apply_remote(doc.version(), changes, quire_core::Origin::Reload)
                    .map_err(|e| err(format!("refused: {e}")))?;
                view.sync(&doc);
            }
            "expectMaxColumn" => {
                let n = nums(rest);
                let got = doc
                    .text()
                    .byte_to_utf16(doc.text().line_range(n[0] - 1).end)
                    .1
                    + 1;
                if got != n[1] {
                    return Err(err(format!("max column: expected {}, got {got}", n[1])));
                }
            }
            "expectSelCount" => {
                let n: usize = rest.parse().map_err(|_| err("number".into()))?;
                if view.selection().len() != n {
                    return Err(err(format!(
                        "selection count: expected {n}, got {}",
                        view.selection().len()
                    )));
                }
            }
            "expectRanges" => {
                // Direction-agnostic ranges (VS Code `fromRange`), sorted by start.
                let want: Vec<(usize, usize)> = parse_sels(doc.text(), rest)
                    .iter()
                    .map(|r| (r.from(), r.to()))
                    .collect();
                let got: Vec<(usize, usize)> = view
                    .selection()
                    .ranges()
                    .iter()
                    .map(|r| (r.from(), r.to()))
                    .collect();
                if want != got {
                    return Err(err(format!(
                        "ranges: expected {rest}, got {}",
                        fmt_sel(doc.text(), view.selection())
                    )));
                }
            }
            "expectLineCount" => {
                let n: usize = rest.parse().map_err(|_| err("number".into()))?;
                if doc.text().len_lines() != n {
                    return Err(err(format!(
                        "line count: expected {n}, got {}",
                        doc.text().len_lines()
                    )));
                }
            }
            "expectPrimarySel" => {
                let want = parse_sels(doc.text(), rest)[0];
                let got = view.selection().primary();
                if got.anchor != want.anchor || got.head != want.head {
                    return Err(err(format!(
                        "primary selection: expected {rest}, got {} {}",
                        fmt_pos(doc.text(), got.anchor),
                        fmt_pos(doc.text(), got.head)
                    )));
                }
            }
            "expectHead" => {
                let n = nums(rest);
                let p = pos(doc.text(), n[0], n[1]);
                let got = view.selection().primary().head;
                if got != p {
                    return Err(err(format!(
                        "head: expected {rest}, got {}",
                        fmt_pos(doc.text(), got)
                    )));
                }
            }
            "wordSelect" => {
                let n = nums(rest);
                let p = pos(doc.text(), n[0], n[1]);
                let cls = quire_core::words::WordClassifier::new(&doc.config().word_separators);
                let r = quire_core::words::word_range_at(doc.text(), p, &cls);
                exec(
                    &mut doc,
                    &mut view,
                    EditorCommand::SetSelection {
                        selection: Selection::single(r.start, r.end),
                    },
                    time,
                )?;
            }
            "expectCursor" => {
                let n = nums(rest);
                let p = pos(doc.text(), n[0], n[1]);
                let got = view.selection().primary();
                if got.anchor != p || got.head != p {
                    return Err(err(format!(
                        "cursor: expected {} {}, got {}",
                        n[0],
                        n[1],
                        fmt_sel(doc.text(), view.selection())
                    )));
                }
            }
            other => return Err(err(format!("unknown step {other}"))),
        }
    }
    Ok(())
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/monaco")
}

#[test]
fn monaco_fixtures() {
    let filter = std::env::var("MONACO_FILTER").ok();
    let mut files: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures/monaco")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "fixture"))
        .collect();
    files.sort();
    let (mut pass, mut expected_fail, mut unexpected_fail, mut unexpected_pass) = (0, 0, 0, 0);
    let mut report = String::new();
    let mut tsv = String::new();
    for f in &files {
        for c in parse_file(f) {
            if let Some(flt) = &filter
                && !c.name.contains(flt.as_str())
                && !c.file.contains(flt.as_str())
            {
                continue;
            }
            let result = run_case(&c);
            let expect_fail = c.status.starts_with("fail");
            tsv.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                c.file,
                c.name,
                if result.is_ok() { "pass" } else { "fail" },
                result
                    .as_ref()
                    .err()
                    .map_or(String::new(), |e| e.replace(['\t', '\n'], " "))
            ));
            match (result, expect_fail) {
                (Ok(()), false) => pass += 1,
                (Ok(()), true) => {
                    unexpected_pass += 1;
                    report.push_str(&format!("UNEXPECTED PASS {} :: {}\n", c.file, c.name));
                }
                (Err(_), true) => expected_fail += 1,
                (Err(e), false) => {
                    unexpected_fail += 1;
                    report.push_str(&format!("FAIL {} :: {}\n    {e}\n", c.file, c.name));
                }
            }
        }
    }
    if let Ok(path) = std::env::var("MONACO_REPORT") {
        std::fs::write(path, &tsv).expect("write MONACO_REPORT");
    }
    println!("{report}");
    println!(
        "monaco fixtures: {pass} pass, {expected_fail} expected failures, {unexpected_fail} unexpected failures, {unexpected_pass} unexpected passes"
    );
    assert_eq!(unexpected_fail, 0, "{report}");
}
