# VS Code test → quire test map

Every test in the ported VS Code suites has a row: the quire test that covers it, or
`N/A: <why>`. Status: `pass`, `fail (ours)` (quire deliberately differs, with the reason), or
`fail (bug)`.

The QE4 gate is ≥ 90 % `pass` over the rows that are not N/A (E11).

Source: microsoft/vscode `main` at the sha named in each section. MIT, see
`THIRD_PARTY_NOTICES.md`.

| VS Code file | VS Code test | quire test | Status |
|---|---|---|---|
