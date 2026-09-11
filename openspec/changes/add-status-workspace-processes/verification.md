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

## Remaining Platform Verification

Native Linux execution remains pending. The official `rust:1.98.0-slim` image
was downloaded, but automatic approval review rejected read-only source and
Cargo-cache mounts into the temporary network-disabled container. No project
source was mounted into that container. Explicit user authorization was requested.

Tasks 1.1, 1.3, and 4.1 remain unchecked because their native Linux verification
has not run. Their implementation and macOS verification are complete. No change
has been archived, pushed, or published.
