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
