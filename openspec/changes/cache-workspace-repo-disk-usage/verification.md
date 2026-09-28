# Verification

## Functional Checks

- `cargo fmt --check`, Clippy with all targets and features, and strict OpenSpec
  validation passed.
- The full test suite passed on macOS ARM64 with Rust 1.98.0. The test command
  placed the system Bash before the Nix shell's Bash because the latter lacks
  the `complete` built-in used by existing shell completion tests:

  ```sh
  direnv exec . sh -c 'PATH=/run/current-system/sw/bin:$PATH cargo test --quiet'
  ```

- Repository-wide `prek -a` checks passed. The hook cache required access
  outside the workspace sandbox.
- Migration tests cover both new migrations, rollback, legacy records, and
  automatic invalidation when workspaces or worktrees become removed.
- CLI tests confirm cached workspace, worktree, and origin sizes after create,
  add, and release. They confirm that removal clears old values while claim
  and open leave measurement times unchanged. The CLI exposes no size refresh
  command.
- Status tests confirm unknown values before measurement, stored partial and
  unavailable results, a source path that disappears after measurement, and
  unchanged database and workspace files during status.

## Status Cost

A temporary synthetic benchmark measured one workspace containing 6,000 files
and another installation with sixteen workspaces containing 200 files each.
Each file contained 1,024 bytes. Measurements ran on macOS ARM64 with five
warmed status invocations per case:

| Case | Lifecycle measurement | Cached status median |
| --- | ---: | ---: |
| One workspace, 6,000 files | 15.7 milliseconds | 3.82 milliseconds |
| Sixteen workspaces, 200 files each | 10.6 milliseconds total | 4.01 milliseconds |

After measurement, the benchmark moved every workspace directory away from
its registered path. Status still returned each saved complete observation;
database bytes and an example file's contents stayed unchanged. This checks
that status reads the cache without walking those directories. Timings are
warm-cache measurements of synthetic data, not a bound for real filesystems.

## Limits

Filesystem edits made outside Trees do not update the stored amount. Each
observation includes its measurement time, and existing rows show `unknown`
until an affected lifecycle command measures them. Linux behavior was not
measured in this local run.
