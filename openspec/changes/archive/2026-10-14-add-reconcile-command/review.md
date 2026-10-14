# Reconciliation Command Review

## Scope and Guidelines

The review compares the command with the repository at `5da5e59`. Experiments
used an isolated source copy, restoring each omission before the next run.

- Errors remain typed Snafu errors. Transparent propagation preserves existing
  errors; failed observation and finalization retain both typed errors, with the
  primary observation failure in the source chain. Audit snapshots alone use text.
- The existing reconciliation module exposes the command result and execution
  function. Existing visibility is preserved, without another module or facade.
- The shared locator and completion registry provide selection and completion.
  Manual and automatic targets remain supported, including active claims.
- Metadata admission uses a short immediate transaction. Recovery and Git work
  run outside it. Admission validates the target again and refuses retained leases.
- Git observation, claim handling, and disk usage behavior remain unchanged.

## Ablation Results

| Experiment | Observed result | Decision |
| --- | --- | --- |
| Remove the active-operation guard immediately after recovery | All five original CLI tests pass; the final immediate transaction still rejects active leases. | Remove the duplicate guard. |
| Remove repeated CLI event-count assertions and force duplicate observation events | The existing core idempotency test fails, reporting one changed worktree instead of zero. | Remove the repeated CLI assertions and counting helper; retain the core test. |
| Replace workspace observation with an unchanged ready summary | The dirty-worktree CLI regression fails: ready instead of degraded. | Retain observation and its CLI regression. |
| Remove interrupted reconciliation from access recovery | The CLI recovery regression fails with a lifecycle-recovery error for the expired check. | Retain recovery admission and its regression. |
| Replace the new selector mode loop with the existing cross-command selector suite | All six final command tests and both locator tests pass. | Reuse shared named-selector coverage and retain the current-directory regression. |

The first core experiment lacked a compile-time documentation input and did not
provide a valid test result. After copying that input, the repeated-event
experiment reached the expected assertion failure. Shared build artifacts were
refreshed afterward to restore repository version metadata before final checks.

## Test Responsibilities

Command regressions protect observation wiring and current-directory selection.
They also verify output formats and claim preservation. Failure cases cover
active operations, expired leases, removed targets, and finalization errors. Existing core tests
protect observation classification and event idempotency. Shared parser,
locator, completion, and open tests cover their existing contracts with the new
command or operation kind added.

## Final Validation

- All 440 Rust tests pass with `cargo test --all-targets --all-features`.
- Flake checks pass for the host system.
- Strict delta validation passes without warnings. The reviewed change is
  synchronized to the lifecycle and open specifications and archived.
- Repository hooks cover the final implementation and archived documents.
