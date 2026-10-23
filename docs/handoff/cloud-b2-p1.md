# Cloud lane brief: quire P1 (cadatomic batch 2, lane quire)

Written 2026-10-23 by the batch coordinator (cloud session `session_01SU3fk4hy1uy9Ye3RWmxkN2`). The
user named it coordinator for cadatomic and ecadabra batches 1 and 2, in place of main-cadatomic,
which is offline. The user said yes to running quire P1 in the cloud.

## Where you work

- **Your branch** is your session's `outcome_branch`, `claude/b2-quire-p1`. Push only there. Never
  push to `master` or `p1-hover`. The coordinator fast-forwards master after reading your report.
- **Base:** quire master `819d32b` plus this brief.
- **`CLAUDE.md` binds,** except where this brief says the cloud differs. Read `STATUS.md`'s handoff
  and `docs/handoff/p1-hover.md` on branch `p1-hover`.

## Work, in order

1. **Land P1 part 2** (hover and signature help). Branch `p1-hover` is at `7fb2955` on base
   `3b4b15a`; its filtered tests pass, and `check.sh` was never run because the MacBook was out of
   disk.
   - Bring its commits onto your branch, rebased on master `819d32b`.
   - Run `check.sh` once and record the counts.
   - Fix what fails. The decisions in `STATUS.md` for P1 part 2 stand.
   - Push.
2. **`SnippetSession.merge`:** the 14 VSCODE-MAP rows marked "ours (queued)". `STATUS.md` says P1c's
   merge-vs-replace choice is "pending approval". If the handoff doesn't record an answer, build
   on the documented recommendation, mark it in your report as a question for the coordinator, and
   keep it easy to reverse.
3. **P1 part 3: inlay hints as injected text**, with the host surface (ARCH-CRATES §3.11 in svelters
   `plans/research/ARCH-CRATES.md`, public). If you can't read that, build from quire's own design
   docs and say so.
4. **P1 part 4: semantic tokens**, with the host surface, as far as the lane gets.

Each item needs tests, VSCODE-MAP rows where VS Code has a matching suite, and one commit or more
per item, pushed as you go.

## Cloud notes (from cadatomic batch 1)

- `hostname`, `nproc`, `free -g`, `df -h .`: record them. Stop if `/mnt/c` exists. The container has
  about 39 GB writable. `CLAUDE.md`'s disk rules ask for 25 GB free for `check.sh`; check before
  running it.
- If the sandbox refuses `git config --global`, set environment variables for each command instead.
- Run `check.sh` once per item that changes code, not more. Timings are "cloud, not a gate".
- The sandbox may reject compound shell commands. If so, use one plain command per call.

## Report

Commit and push `docs/handoff/cloud-b2-p1-report.md` as your last action. It gives:
- the branch and final sha;
- per item: its state, the public API it added, and test counts with the exact commands;
- each `check.sh` result;
- VSCODE-MAP totals before and after;
- questions for the coordinator, each with a recommendation;
- what cadatomic must change to use the new API (`<code-editor>`, K1's `EditorEvent` uses). That
  goes with the next quire pin advance in cadatomic;
- the setup as it actually ran.

Also update `STATUS.md`'s milestones table on your branch. The coordinator replaces the handoff
block when it merges.

Near 400k tokens of context, stop, commit, and write the handoff into the report.
