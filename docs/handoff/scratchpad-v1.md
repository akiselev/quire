# Scratchpad v1 projection lane

Based on f2208478faa1466061af03374639ed40e9842eff. Branch: scratchpad-v1.

`quire-core::projection` owns versioned source-byte replacements, unique IDs, Unicode boundary and
overlap checks, source/display affinity mapping and live change mapping. Edits intersecting a
replacement invalidate it. Untouched ranges survive the short history window. `Document` owns the
projection set and maps it at the single apply seam; no source is rewritten by presentation.

Skelter's DisplayLine shapes same-line replacements and maps hit/caret positions through them.
A focused selection reveals the whole source line, including hidden tokens; preedit and inlay hint
composition fall back to the established source path. Layout keys include projection generation,
so answer-only updates invalidate rendering without fake source edits. The generic kind exposes
exact `(NodeId, Applied)` receipts through `QuireHost.committed`. Capture is opt-in: its default
is `None`, so existing event-only hosts retain no transaction records. Set `Some(Vec::new())`
before editing, then drain the vector at each checkpoint. Capturing never reapplies a transaction.

This is an opt-in inline replacement seam. It does not implement arbitrary-height embedded blocks,
a cross-block selection engine, or an egui projection renderer. Multi-line replacements remain data
only and the Skelter view retains source there. These are future generic work, outside scalar v1.

Validation: named-package rustfmt; 396 passing tests across quire-core and skelter_quire, including
all their integration and doc-test targets. Command from this worktree:

```sh
CARGO_SLOT=/projects/svelters/tools/dev/cargo-slot \
CARGO_TARGET_DIR=/projects/scratchpad/target \
CARGO_BUILD_BUILD_DIR=/home/kiselev/.cargo/build-cache \
scripts/cargo.sh test -p quire-core -p skelter_quire \
  --config 'patch."https://github.com/akiselev/svelters".skelter-base.path="/projects/svelters/crates/skelter-base"' \
  --config 'patch."https://github.com/akiselev/svelters".skelter-core.path="/projects/svelters/crates/skelter-core"' \
  --config 'patch."https://github.com/akiselev/svelters".skelter-text.path="/projects/svelters/crates/skelter-text"'
```

The local lockfile changes caused by path overrides were reversed after tests; no dependency changed.
The final integration gate below supersedes the earlier incomplete full-check status. No source
was changed in Svelters or Cadatomic, and the recorded dependency lockfile remains unchanged.


## Optimized provider scheduling repair

Inactive hover/inlay providers could report pending work or a zero deadline in the optimized native
harness, keeping the host commit barrier busy. Timer aggregation now matches explicit optional timer
states and only schedules real deadlines. An inactive-provider regression runs in both build profiles;
Scratchpad's optimized native input/undo harness now settles and passes. A compiler root cause was not
established; do not infer one from the release/debug difference.

Additional validation: 75 optimized tests (inactive deadline, hover, inlay hints and parameter hints),
and all-target Clippy with warnings denied for quire-core and skelter_quire. The complete named-package
suite above passed again with 396 tests. Scratchpad carries a source patch from the base revision for
reconstructing this private integration lane; it does not depend on a master landing.


## Integration gate and local landing (2026-11-04)

The complete, unmodified `scripts/check.sh` passed on frozen source commit `4bffa55`:

- rustfmt and workspace/all-target/all-feature Clippy with warnings denied;
- **609 tests passed, 0 failed, 6 ignored**, including the egui and Skelter renderer suites;
- wasm32 compilation of quire-core and quire-text;
- headless dependency, no thread-spawn, rope encapsulation and no-clock checks;
- egui exactly 0.37.0.

The ignored tests are the existing manual `enter_at_100k_lines`, `backtrack_survey`, `dump_sexp`,
`e5_timing`, `ranged_query_cost` and `root_reparse_cost` probes. No test was disabled to pass this gate.

The root target filesystem had less than the required 25 GiB, so both Cargo output locations used
/home. The final successful cached rerun began with 27.57 GiB free and ended with about 28 GiB free.
The first long build/test attempt was interrupted by SIGTERM (exit 143) without an assertion failure;
its cause was not established. The unchanged rerun completed every step with exit 0. Logs remain at
`/tmp/scratchpad-quire-full-gate.log` and `/tmp/scratchpad-quire-full-gate-interrupted.log`.

```sh
CARGO_SLOT=/projects/svelters/tools/dev/cargo-slot \
CARGO_TARGET_DIR=/home/kiselev/.cache/scratchpad-quire-gate \
CARGO_BUILD_BUILD_DIR=/home/kiselev/.cargo/build-cache \
CARGO_BUILD_JOBS=3 CARGO_INCREMENTAL=0 scripts/check.sh
```

This is the stock locked-dependency gate, including Svelters `cd2adc4fb247262c589f3f8b07f53edd84a352c4`,
on Rust 1.97.0. The prior Scratchpad integration tests used its local Svelters path overrides; those
are a separate consumer configuration. The coordinating Scratchpad session owns its final compatibility
check and regeneration of the source patch after this landing.

The integration audit found that unconditional receipt collection would retain edits in existing hosts
that drain only events (including the Svelters gallery). Commit `4bffa55` makes receipt capture opt-in,
with Kind-dispatch tests proving exact node/version/change delivery, no redelivery, preserved JS signals
and disabled legacy behavior. Additional validation proves invalid overlap/duplicate/empty/out-of-bounds
projection updates leave the previously accepted projections and their generation intact.

The coordinating Scratchpad session (`/root`) reviewed and approved the source-backed projection,
Document/apply and DisplayLine seams and the opt-in receipt API under the user's explicit request to
complete Quire integration. No new default user-facing command was introduced. After the green gate,
local master is fast-forwarded from the clean `f220847` base to this lane, including this handoff.
The user subsequently requested publishing the Quire changes. A fresh fetch confirmed origin/master
still at `f220847`; `git rebase origin/master` reported no changes needed. The publication handoff
changes documentation only; the previously green source and dependency lockfile are unchanged.
No native Windows/macOS or real IME/screen-reader acceptance is implied by these Rust regression checks.
