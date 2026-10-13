# GC Table Review

## Scope and Guidelines

The review compares `d9bbc5b` and `ee3cbe5` with the implementation before this
feature at `686231f`. Experiments ran in an isolated detached worktree; each
omission was restored before the next experiment.

- Errors remain typed Snafu errors, with transparent propagation for display
  cache decoding and context for database results. Source errors stay chained.
- Only the existing table and repository summary helpers gain shared visibility
  through the status module. No facade or file split is introduced.
- Repository rows are consumed through the existing status conversion. The
  additional storage-row `Clone` implementation is removed.
- Core GC candidates retain their original fields and errors. Cached size and
  repository display rows belong to the read-only CLI preview.
- Existing claim, confirmation, lease, identity, and physical removal checks
  remain unchanged. Removed history is excluded before counting candidates.

## Ablation Results

| Experiment | Observed result | Decision |
| --- | --- | --- |
| Move repository rows and cached sizes out of core candidates; remove the added cache error and row cloning | All 237 library tests, 40 workspace reuse tests, and both size integration tests pass. | Retain the simpler CLI projection. |
| Remove repeated partial, unavailable, and unknown CLI cases, event counting, and file-preservation setup | The smaller CLI regression and existing size-formatting unit test pass. | Remove repeated assertions; existing formatting and lifecycle tests cover those behaviors. |
| Remove tombstone exclusion | The core scan regression fails: five automatic rows instead of four. | Retain exclusion before counts. |
| Replace the CLI size projection with an unknown value | The smaller CLI regression fails at the expected cached size. | Retain cached size wiring and its CLI regression. |
| Delete the CLI regression while the size projection is broken | The remaining size integration test passes despite the incorrect GC output. | Retain the smaller CLI test. |
| Make the shared table and repository summary functions private | The binary fails to compile with private-function errors. | Retain the two shared functions rather than duplicate their implementations. |

## Test Responsibilities

The CLI regression protects repository display, cached-size wiring, path
omission, removed-row exclusion, execution totals, and empty inventory output.
`renders_exact_binary_units_and_compact_incomplete_cells` protects all cached
size display states. `dry_run_scan_uses_a_read_only_connection_and_preserves_state`
and the existing lifecycle regressions protect read-only scanning and retained
workspace content. A green suite after deleting a test alone is not evidence
that the test is redundant.

## Final Validation

- All 434 tests pass with `cargo test --all-targets --all-features`.
- The complete repository hook suite passes after correcting two documentation
  wording issues; formatting, Clippy, diagnostics, and SQL checks pass.
- `nix flake check --no-build` passes for the current host.
- Strict change validation passes without findings. Standard validation of the
  synchronized `workspace-reuse` specification passes, and its fourteen existing
  requirement-length warnings remain unchanged after archive.
- `git diff --check` passes. The implementation and test simplification removes
  36 more lines than it adds; no new production module or type is introduced.
