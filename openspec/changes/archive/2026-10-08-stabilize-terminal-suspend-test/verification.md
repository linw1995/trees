# Verification

## Diagnosis

- Main run `37778442446` timed out after `Ctrl-Z` while waiting for
  `SESSION_SUSPENDED_OK`; its Nix package build passed the same test.
- A Linux `PTY` reproduction printed the actual input consumed by the descendant
  after resume. That input was the shell's `printf` marker command. The helper
  finished without receiving another line.
- Disabling signal-related terminal flushing with `NOFLSH` did not eliminate
  the failure. The input consumer, rather than terminal flushing, must change.

## Ablation

| Experiment | Observed result | Decision |
| --- | --- | --- |
| Restore a descendant terminal read | Linux reproduction consumed the shell marker command and timed out. | Remove terminal input from the suspension helper mode. |
| Use thread-local signal blocking and `sigwait` | The actual macOS suspension test timed out after `fg`. | Remove that design; a process-directed signal can reach another harness thread. |
| Remove the continue handler | A stopped and resumed Rust helper never acknowledged completion. | Retain the local handler. |
| Deliver `SIGCONT` before installing the handler | The Rust helper lost the signal and timed out. | Install before announcing readiness. |
| Deliver `SIGCONT` after readiness but before `pause` | The final Rust helper printed completion and exited successfully. | Keep handler-driven exit to avoid a lost wakeup. |
| Remove the helper wait | The Rust helper exited before acknowledging resume. | Retain the wait until the signal arrives. |
| Remove a separate helper environment setting | The existing selector's `continue` value passed the native tests. | Reuse one selector; add no extra configuration. |
| Assess overlap with interrupt and recovery tests | Those tests do not exercise `Ctrl-Z` and `fg` through the outer shell's supervisor job. | Keep this existing test; add no duplicate fixture or permanent ablation tests. |

Helper ablations extracted the actual Rust helper into temporary executables
linked against the same `libc` build. The driver checked stopped status with
`waitpid`, resumed with `SIGCONT`, and bounded negative cases with a timeout.
All experimental files remained outside the repository.

## Checks

- macOS: `nix develop --command cargo nextest run --locked --workspace` passed
  all 425 tests.
- macOS: the actual suspension test passed 100 consecutive runs.
- Linux: all 425 tests passed with CI coverage compiler flags under a non-root user.
- Linux: the actual coverage-instrumented suspension test passed 200 consecutive runs.
- `nix develop --command prek -a` passed all repository hooks.
- Semantic title and previous release selection checks passed all 13 Python tests.
- Strict validation of the tooling change passed with `skip_specs: true`.

Linux validation uses Rust 1.98.1, the CI coverage compiler flags, an absolute
external `LLVM_PROFILE_FILE`, and a non-root user. A root container bypasses the
existing permission-failure test; a default profile filename pollutes fixture
worktrees. These environment differences must be corrected before assessing
the full suite.

## Repository Rule Review

- Code, comments, artifact text, branch name, and commit message are English.
- The change touches only private test helpers and introduces no public API,
  visibility, error channel, dependency, or production signal behavior changes.
- The handler uses only async-signal-safe `write` and `_exit`; it does not use
  Rust locks, allocation, or cleanup from signal context.
- Existing helpers still read terminal input for interrupt and database
  transaction coverage. Only the suspension invocation selects continue mode.
- The test still sends `Ctrl-Z` through the `PTY`, executes a shell command while
  the job is suspended, resumes it through `fg`, and waits for completion.
- No retry policy, fixed sleep, terminal flag, signal facade, extra fixture, or
  new permanent test was needed.
- Runtime requirements are unchanged. Archive this tooling change with spec
  synchronization skipped rather than inventing a new product requirement.

The final rule review found no remaining issues. Standard validation of all
12 existing specifications passed; their existing length warnings are unchanged.

The reviewed tooling change was archived as
`2026-10-08-stabilize-terminal-suspend-test` with spec synchronization skipped.
All existing specifications remained unchanged.
