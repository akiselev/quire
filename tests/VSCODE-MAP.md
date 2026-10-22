# VS Code test → quire test map

The map is generated into one file, [`crates/quire-core/tests/VSCODE-MAP.md`](../crates/quire-core/tests/VSCODE-MAP.md),
by `scripts/port-vscode-tests.sh` (node runs each VS Code test file under recording stubs, the
fixtures run in `crates/quire-core/tests/monaco.rs`, and `tests/port/map.mjs` writes the rows).
Every test in the ported suites has a row: the fixture that translates it, or `N/A: <why>`.
Status: `pass`, `fail (ours: …)` (a deliberate difference or a missing quire feature), or
`fail (bug: …)`.

The QE4 gate is ≥ 90 % `pass` over the rows that are not N/A (E11).

Source: microsoft/vscode at the sha named in the generated file. MIT, see `THIRD_PARTY_NOTICES.md`.
