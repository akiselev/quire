# quire: instructions for agents

quire is a standalone code-editor crate set for egui 0.37: `quire-core` (headless model),
`quire-syntax` (tree-sitter highlighters) and `egui_quire` (the widget). It is owned by the
`macbook-cadatomic` session; the consumer is cadatomic (and svelters' `<code-editor>` kind), but
quire depends on neither.

## Read first

1. `STATUS.md`: what is landed, what is next, the handoff block.
2. `docs/design/SEAM-CHANGES.md`: decisions that override the design text.
3. `docs/design/ARCH-QUIRE.md`: the design (§3 is quire, §4.2 the host seam).

## Hard rules

- `quire-core` has no egui, eframe, winit, wgpu or tokio dependency, spawns no threads, and builds
  for `wasm32-unknown-unknown`.
- No crate spawns threads: background work goes through the host's `Spawner`.
- egui is exactly `=0.37.0` (egui_kittest `=0.37.0`).
- `unsafe_code = "forbid"` (workspace lint).
- No dependency on svelters, cadatomic, cadabra or any kernel crate.
- The rope type (ropey 1.6) never appears in the public API; everything goes through `Text`.
- Positions are UTF-8 byte offsets, valid only with their `Version`.
- One mutation path: `Document::apply(Transaction)` (and `apply_remote`). Commands are pure:
  command → transaction.
- Licences: MIT OR Apache-2.0. VS Code/Monaco (MIT): literal translation allowed, keep
  Microsoft's notice in each translated file and in `THIRD_PARTY_NOTICES.md`. CodeMirror 6
  (MIT): design, notice if code is translated. Zed `text`/`rope`/`editor` (GPL): designs only,
  never copy. Helix/tree-house (MPL-2.0): depend freely; copied query files keep their header and
  stay separate files.

## Builds (shared machine slots)

- Every cargo command goes through `scripts/cargo.sh`. Builds and tests take a machine build slot;
  check, clippy and fmt run niced with 3 jobs and no slot.
- After a branch merges, delete its worktree's `target/`. At most two worktrees have their own
  target; others share one by exporting `CARGO_TARGET_DIR=<dir>` and
  `CARGO_BUILD_BUILD_DIR=<dir>/build`. Below 20 GB free disk, stop building and tell the coordinator.
- While writing: `scripts/cargo.sh check -p <crate>` and only the touched tests
  (`scripts/cargo.sh test -p <crate> --test <name>` or a name filter).
- `scripts/check.sh` once per milestone, never on unchanged code.
- No hosted CI.

## Git

- `master` on github.com/akiselev/quire is the integration branch. Work in a worktree on your
  own branch (`git worktree add ../quire-wt/<name> -b <name>` from `~/projects/quire`).
- Commit early. To land: `git fetch && git rebase origin/master`, run the touched crate's
  tests, then `git push origin HEAD:master` (fast-forward only; never force-push master).
- Commit messages say what changed and which milestone (QE0, QE1, QE2, QE4).
- **Master stays green:** only work that passes the full `scripts/check.sh` lands on master.
  Unfinished code stays on your branch.

## Handoff

An agent past about 400k tokens of context, or at the end of its task, writes
`docs/handoff/<branch>.md` (what is done, what is next, open problems, test counts) and commits
it, so a fresh agent can continue.

## Questions and approvals

- **Needs approval before it lands on master** (main-cadatomic takes it to the user): a new
  user-facing command, or behaviour that differs from VS Code or `docs/design/ARCH-QUIRE.md` §3.
  Send the proposal (signature, rationale, recommendation) to the coordinator, keep it on your
  branch, and keep working meanwhile.
- **The coordinator decides** small public helpers, types and signature changes that stay inside
  the agreed design and VS Code behaviour (main-cadatomic, 2026-10-22); they are listed in its
  reports. Tell the coordinator about them before landing.
- Internal representation and test-port mechanics need no approval.

Workers send questions to the coordinator (macbook-cadatomic) with a recommendation and keep
working on something else meanwhile. Nobody asks the user.
