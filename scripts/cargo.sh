#!/usr/bin/env bash
# Runs cargo in one of the machine's shared build slots (svelters' tools/dev/cargo-slot).
#   scripts/cargo.sh check -p quire-core
#   scripts/cargo.sh test -p quire-core --test changeset
# Every cargo command in this repo goes through here. Set CARGO_SLOT to override the slot
# script's path.
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
slot=${CARGO_SLOT:-$here/../svelters/tools/dev/cargo-slot}
if [ ! -x "$slot" ]; then
  # Worktrees live one level deeper (../quire-wt/<name>); look beside the main checkout too.
  slot=$HOME/projects/svelters/tools/dev/cargo-slot
fi
export RUSTC_WRAPPER=${RUSTC_WRAPPER:-$(command -v sccache || true)}
export SCCACHE_CACHE_SIZE=${SCCACHE_CACHE_SIZE:-20G}
cd "$here"
# Worktrees may share a target directory (CARGO_TARGET_DIR). Cargo can reuse a workspace crate's
# artifacts built from another worktree's sources (cadatomic, 2026-10-17: impossible "unresolved
# import" errors). If the previous build in this target came from another worktree, clean the
# quire packages first; dependency artifacts stay shared. (As cadatomic's cargo.sh.)
target=${CARGO_TARGET_DIR:-$here/target}
last=$target/.last-worktree
if [ "${1:-}" != "clean" ] && [ -f "$last" ] && [ "$(cat "$last")" != "$here" ]; then
  echo "cargo.sh: previous build was from $(cat "$last"); cleaning quire packages" >&2
  cargo clean -p quire-core -p quire-syntax -p egui_quire >/dev/null 2>&1 || true
fi
mkdir -p "$target" && echo "$here" > "$last"
# check/clippy/fmt and other non-building subcommands take no slot (user, 2026-10-22, as
# svelters' cargo-slot and cadatomic's cargo.sh): they run niced with few jobs so builds in the
# slots keep the CPU.
if [ -z "${BUILD_SLOT_HELD:-}" ] && [[ "${1:-}" =~ ^(check|clippy|fmt|metadata|tree|locate-project)$ ]]; then
  export CARGO_BUILD_JOBS="${CHECK_JOBS:-3}"
  exec nice -n 10 cargo "$@"
fi
if [ -x "$slot" ]; then
  exec "$slot" cargo "$@"
fi
echo "cargo.sh: no cargo-slot found (set CARGO_SLOT); running cargo without a slot" >&2
exec cargo "$@"
