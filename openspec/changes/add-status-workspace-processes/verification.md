## Implemented Behavior

- Native process collection uses `libproc` on macOS and `/proc` on Linux.
- Effective user filtering, current-directory ownership, nearest registered
  boundaries, observer exclusion, and stable process ordering are implemented.
- Device and `inode` checks prevent deleted or replaced working directories from
  being attributed through misleading path text.
- Database snapshots and boundary queries remain consistent; observation starts
  after the owned database connection is closed.
- Human summaries and all version-2 JSON views include target process observations
  with explicit completeness and independent timestamps.

## Verification Results

On macOS ARM64:

- `cargo test --all-targets`: 298 tests passed before directory identity hardening.
- After directory identity hardening, `cargo test status:: --lib`: 33 tests passed,
  and `cargo test --test status`: 8 tests passed.
- Real synchronized child tests cover symbolic links, deleted working directories,
  process exits, explicit and inferred targets, all three views, nested removed
  boundaries, similar path prefixes, and exclusion of the observer.
- Linux process metadata parsers and a simulated process filesystem are compiled
  and tested on macOS, including effective user IDs, command delimiters, malformed
  metadata, missing working directories, and exited processes.
- `prek run --all-files`: passed. Each implementation commit also runs the
  applicable commit hooks, including formatting, Clippy, compiler checks,
  rust-analyzer, notices, SQL boundaries, and documentation checks.
- `nix flake check --no-build`: passed for the local platform; incompatible
  platforms were omitted by Nix.
- `openspec validate add-status-workspace-processes --strict`: passed.

On Linux ARM64:

- `cargo test --offline --all-targets`: 299 tests passed against source commit
  `d45dac3`, including native process collection and all CLI integration tests.
- The temporary test image uses official `rust:1.98.0-slim` with Git installed
  before mounting any project source or Cargo cache. The initial slim image lacked
  Git, which caused existing Git-dependent tests to fail; installing Git resolved
  those environment failures without source changes.
- Tests ran as user `1000:1000`, with networking disabled, all Linux capabilities
  dropped, and privilege escalation disabled. The source archive and Cargo cache
  were mounted read-only after explicit user authorization. Compilation used
  container-local temporary files and cached dependencies only.
- Real child tests passed for physical working directories, symbolic links,
  deleted directories with misleading replacement paths, and process exits.
  CLI tests passed for all views, target selection modes, nested removed
  boundaries, observer exclusion, and unchanged persisted state.
- The temporary test container was automatically removed after completion.

## Completion

All implementation tasks and required native platform verification are complete.
The process adapters were tested on Linux ARM64 and macOS ARM64. Native x86_64
execution was not performed in this workspace. No change has been archived,
pushed, or published.
