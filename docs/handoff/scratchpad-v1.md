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
exact `(NodeId, Applied)` receipts through `QuireHost.committed`; the consuming host must drain it.

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
Full Quire check.sh, egui integration and master landing remain unperformed. Initial builds filled
/home; inactive caches were cleaned at the user's request. The root filesystem containing the default
target is still below the 25 GB full-check start requirement. Keep the lane on its branch until the
full gate and integration workflow can run. No source was changed in Svelters or Cadatomic.


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
