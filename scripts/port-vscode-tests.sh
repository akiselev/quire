#!/usr/bin/env bash
# Regenerates fixtures/monaco/*.fixture and crates/quire-core/tests/VSCODE-MAP.md from VS Code's
# test sources at the pinned sha. Needs node >= 23 (type stripping) and network (raw.githubusercontent.com).
#   scripts/port-vscode-tests.sh
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
SHA=bdce63db587665a91d1f1237b807d43471f0d9bc
src=$(mktemp -d)
port=$here/crates/quire-core/tests/port
fx=$here/fixtures/monaco
SUITES=(
  src/vs/editor/test/browser/controller/cursor.test.ts
  src/vs/editor/contrib/linesOperations/test/browser/copyLinesCommand.test.ts
  src/vs/editor/contrib/linesOperations/test/browser/moveLinesCommand.test.ts
  src/vs/editor/contrib/linesOperations/test/browser/linesOperations.test.ts
  src/vs/editor/contrib/multicursor/test/browser/multicursor.test.ts
)
# PORT_ONLY=<substring> regenerates only matching suites (the others keep their fixtures).
for rel in "${SUITES[@]}"; do
  [[ -n "${PORT_ONLY:-}" && "$rel" != *"$PORT_ONLY"* ]] && continue
  mkdir -p "$src/$(dirname "$rel")"
  curl -sfL "https://raw.githubusercontent.com/microsoft/vscode/$SHA/$rel" -o "$src/$rel"
  name=$(basename "$rel" .ts)
  node "$port/port.mjs" "$src" "$rel" "$fx/$name.fixture" "$SHA"
done
report=$(mktemp)
MONACO_REPORT=$report "$here/scripts/cargo.sh" test -p quire-core --test monaco -- --nocapture >/dev/null 2>&1 || true
node "$port/map.mjs" "$fx" "$report" "$port/triage.json" "$here/crates/quire-core/tests/VSCODE-MAP.md" "$SHA"
rm -rf "$src" "$report"
