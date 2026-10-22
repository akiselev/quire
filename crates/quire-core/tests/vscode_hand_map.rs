//! `tests/port/hand.json` lists the hand-ported VS Code suites for `tests/VSCODE-MAP.md`: every
//! row that claims a passing quire test must name a test function that exists in its file.

#[test]
fn every_hand_mapped_test_exists() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let hand: serde_json::Value =
        serde_json::from_str(include_str!("port/hand.json")).expect("hand.json parses");
    let mut missing = Vec::new();
    for s in hand["suites"].as_array().unwrap() {
        let file = s["file"].as_str().unwrap();
        let src = std::fs::read_to_string(format!("{root}/{file}")).expect(file);
        for row in s["tests"].as_array().unwrap() {
            if let Some(t) = row[1]["test"].as_str()
                && !src.contains(&format!("fn {t}()"))
            {
                missing.push(format!("{file}: {t}"));
            }
        }
    }
    assert!(missing.is_empty(), "missing tests: {missing:?}");
}
