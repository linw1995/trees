## Implementation

The change adds optional supervised automatic creation through
`--open[=<PROGRAM>] --release-on-exit`. It retains the original claim identity,
uses ordinary release safety checks, opens an interactive recovery shell after
release failure, and preserves the initial program outcome. Each task has a
separate local commit following the initial planning commit.

Ownership uses the globally unique claim ID and canonical workspace path;
checks resolve the associated workspace from the claim. Release opens only an
existing database, preventing a missing database from being recreated and
misinterpreted as a released claim. Process signal handlers remain installed
through both release and recovery, with lifecycle work outside signal handlers.

## Validation

| Check | Result |
| --- | --- |
| macOS native `cargo test` | 334 passed; no failures |
| Linux native `cargo test --offline` | 334 passed; no failures |
| macOS `scripts/run-cov.sh` | 334 passed; no skipped tests; coverage artifacts generated |
| `scripts/run-crap.sh` | 659 functions analyzed; no violations above threshold 30 |
| `cargo fmt --check` | Passed |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| `prek run --all-files` | All repository hooks passed |
| `nix build --no-link .#trees --print-build-logs` | Passed, including release tests and packaging |
| `nix flake check --no-build` | Passed for the local system |
| `openspec validate add-create-release-on-exit --strict` | Passed |
| `git diff --check` | Passed |

## Platform Coverage

macOS tests ran natively on Apple Silicon. Linux tests ran as a non-root user in
an isolated local container with Rust 1.98.0 and Git 2.47.3, using a read-only
repository mount and a separate build volume. An initial root-container run
failed a GC permission test because root bypassed its file permissions;
the full non-root run passed without changing that test.

The first package build was interrupted by approximately 15 minutes of host
sleep. That exceeded existing five-minute operation leases and caused session
test failures. A complete rerun passed after the interruption; lease safety was
not relaxed. Test helpers use `PATH` resolution for `true` and `pwd` instead of
assuming fixed system directories.

The 19 session integration tests cover argument validation before mutation.
They exercise both program selection forms and compatibility without the new
flag. They also cover allocation, reuse, dirty claims, multiple repositories,
lifecycle events, manual release, and replacement claims. Failure cases include missing
storage, unavailable shells, missing workspace directories, noninteractive
recovery, launch failures, and exit status precedence. Both native platforms
exercised real pseudo-terminals for terminal interrupts, supervisor termination
forwarding, suspension, foreground resumption, repeated recovery, and shell job
control.

Unit tests cover preserved source errors, interrupted waits, signal exit codes,
and refusal to invoke release when child termination is uncertain. The existing
replacement-before-admission test also passes in the full suites.

## Limits

Windows execution was not exercised. Cleanup after abrupt supervisor termination
or detached descendants remains outside the guarantee. Recovery shells do not
hold exclusive workspace leases; an external manual release can change ownership
while a shell is running, and subsequent attempts always retain the original
claim identity. These limits are documented in `docs/workspaces.md`.
