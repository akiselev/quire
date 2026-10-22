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
