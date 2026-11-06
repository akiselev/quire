# cradle-b11 quire gate: `scripts/check.sh` on `cradle-b10-quire`

Gate runner for the Cradle batch coordinator, 2026-11-06 (cloud session).

- **Sha tested:** `6e9cdd9142383486b1147e89b197beb490e1650f` (matches the expected tip, and
  `origin/cradle-b10-quire` pointed there too).
- **Disk before the run:** 31 GB free (the minimum is 25 GB).
- **Command:** `CARGO_PROFILE_DEV_DEBUG=0 scripts/check.sh`, the script unmodified. No cargo-slot exists
  on this machine, so it ran without a slot.
- **Result: FAIL (environment). The gate could not build anything, so the branch's code is not
  verified.**

## Steps

| Step | Result |
|---|---|
| fmt (`cargo fmt --all -- --check`) | **pass** |
| clippy (`--workspace --all-targets --all-features -D warnings`) | **fail: could not fetch a dependency** (see below) |
| tests (`test --workspace --all-features`) | not run (`set -e` stopped the script) |
| wasm32: quire-core | not run |
| wasm32: quire-text | not run |
| quire-text dependency, ropey and clock checks | not run |
| quire-core dependency and thread-spawn checks | not run |
| egui is exactly 0.37.0 | not run |

Test counts: none. No test was compiled.

## Failure

```
error: failed to get `skelter-base` as a dependency of package `skelter_quire v0.1.0 (crates/skelter_quire)`
  unable to update https://github.com/akiselev/svelters?branch=master#7e3c06d2
  revision 7e3c06d2a0818209199a3a664910e66edafaa6fc not found
  failed to receive HTTP 200 response: got 401; class=Net (12)
```

- The cause is this environment, not the branch. `akiselev/svelters` is private, and this cloud
  session's GitHub access covers only `akiselev/quire`. `git ls-remote` on svelters fails too
  ("could not read Username"). The request to attach svelters read-only to the session was denied.
- master has the same svelters git dependencies (`Cargo.toml` lines 26-28: skelter-base,
  skelter-core, skelter-text), so a master checkout fails here the same way. The branch changes only
  the lock: svelters goes from `cd2adc4` to `7e3c06d`, and the lock gains `svelters-abi` (used by
  skelter-base) and an edge from skelter-anim to svelters-style.
- `cargo metadata` fails as well. No later step can resolve the workspace in this environment.
- I made no code change: this is not a build break or test failure in `crates/skelter_quire`.

## Next

Rerun `scripts/check.sh` at `6e9cdd9` on a machine that can fetch `akiselev/svelters` at
`7e3c06d2` (the macbook, or a cloud session that has svelters attached). Check that the svelters
`master` branch still contains `7e3c06d2`. The lock pins it, and cargo said "revision not found"
only because of the 401, but confirm it there.
