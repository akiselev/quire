#!/usr/bin/env bash
# The local CI (R12): format, clippy with warnings denied, tests, and wasm32 checks of
# quire-core and quire-text. Run once at the end of a milestone, never on unchanged code.
set -euo pipefail
cd "$(dirname "$0")/.."
# The whole run holds one machine build slot (svelters' cargo-slot protocol). cargo-slot sets
# BUILD_SLOT_HELD, so the nested scripts/cargo.sh calls below run inside it without taking
# another slot, including the clippy and check steps that would otherwise run unslotted.
if [ -z "${BUILD_SLOT_HELD:-}" ]; then
  slot=${CARGO_SLOT:-$PWD/../svelters/tools/dev/cargo-slot}
  [ -x "$slot" ] || slot=$HOME/projects/svelters/tools/dev/cargo-slot
  if [ -x "$slot" ]; then
    exec "$slot" "$PWD/scripts/check.sh" "$@"
  fi
  echo "check.sh: no cargo-slot found (set CARGO_SLOT); running without a slot" >&2
fi
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

step "wasm32: quire-text"
$c check -p quire-text --target wasm32-unknown-unknown

step "quire-text has no egui, svelters or tokio; ropey only in text.rs; no clock"
if $c tree -p quire-text -e normal --prefix none 2>/dev/null | grep -E '^(egui|eframe|epaint|winit|wgpu|tokio|svelters[a-z_-]*) ' ; then
  echo "quire-text depends on a forbidden crate" >&2; exit 1
fi
if grep -rln 'ropey' crates/quire-text/src | grep -v '^crates/quire-text/src/text.rs$' ; then
  echo "ropey is used outside quire-text/src/text.rs" >&2; exit 1
fi
if grep -rn 'std::time' crates/quire-text/src ; then
  echo "quire-text reads no clock" >&2; exit 1
fi

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
