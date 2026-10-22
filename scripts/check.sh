#!/usr/bin/env bash
# The local CI (R12): format, clippy with warnings denied, tests, and a wasm32 check of
# quire-core. Run once at the end of a milestone, never on unchanged code.
set -euo pipefail
cd "$(dirname "$0")/.."
c=scripts/cargo.sh
step() { printf '\n== %s\n' "$*"; }

step "fmt"
cargo fmt --all -- --check

step "clippy"
$c clippy --workspace --all-targets --all-features -- -D warnings

step "tests"
$c test --workspace --all-features

step "wasm32: quire-core"
$c check -p quire-core --target wasm32-unknown-unknown

step "quire-core has no egui, tokio or thread spawning"
if $c tree -p quire-core -e normal --prefix none 2>/dev/null | grep -E '^(egui|eframe|epaint|winit|wgpu|tokio) ' ; then
  echo "quire-core depends on a forbidden crate" >&2; exit 1
fi
if grep -rn 'std::thread::spawn\|thread::spawn(' crates/*/src ; then
  echo "crates must not spawn threads (use a host Spawner)" >&2; exit 1
fi

step "egui is exactly 0.37.0"
$c tree -p egui_quire -e normal --prefix none -i egui 2>/dev/null | head -1 | grep -q '^egui v0.37.0' \
  || { echo "egui_quire must use egui =0.37.0" >&2; exit 1; }

printf '\ncheck.sh: all passed\n'
