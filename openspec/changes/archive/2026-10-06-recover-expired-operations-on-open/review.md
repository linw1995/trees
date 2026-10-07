# Open Recovery Ablation Review

## Baseline and Method

The verified implementation was committed as `8b025ac` before experimentation.
Each ablation ran in a detached worktree at that commit, using the repository's
Nix Rust environment. Experimental changes were restored between runs. Only
validated simplifications were copied back to the implementation branch.

The original complete Rust suite and commit hooks passed. The focused open suite
contained eight tests. Results below come from actual compile and test runs;
a green suite after deleting a test does not establish redundant coverage.

## Experiments

| Ablation | Observed result | Decision |
| --- | --- | --- |
| Remove the access operation type guard. | The structural recovery regression failed at `assert!(!output.status.success())`: an expired create operation was recovered and the program started. | Keep the allowed operation types and reject structural or unknown kinds before lease takeover. |
| Remove the final lease admission check. | The concurrent admission regression failed at `assert!(!output.status.success())`: the program started with a competing lease present. | Keep the final consistent snapshot and lease check. |
| Replace the public policy enum and parameterized API with a named access entry point and a private mode argument. | All eight open tests passed. | Remove the public policy abstraction while retaining one shared recovery implementation and the existing lifecycle API. |
| Reuse the real workspace fixture and replace irrelevant Git setup with metadata fixtures. | All eight open tests passed. Real automatic workspace creation dropped from ten instances to one. | Apply the smaller fixtures. |
| Delete the CLI recovery persistence failure test. | The remaining seven tests passed, but none inject a terminal persistence failure through open. | Retain this unique error propagation and no-launch coverage. Remove its redundant internal lease identity and state assertions. |
| Repeat type-guard and admission ablations against the smaller tests. | Both regressions still failed at the program-launch assertion. | The simplified tests retain their safety detection. |

The commands used `cargo test --test workspace_open` and these focused filters:

- `open_preserves_expired_structural_and_unknown_operations`
- `open_rechecks_operation_admission_after_recovery`

## Test Coverage Review

The retained positive test verifies every allowed operation kind, terminal audit
recording, lease retirement, and preservation of the branch, HEAD, staged and
unstaged state, file contents, and original claim. The fixture is shared, while
preservation is still checked after each operation so a later recovery cannot
hide an earlier change.

Removed assertions covered exact recovery log wording, a selected workspace
health state, and internal recovery lease identity. Those details do not define
successful program handoff. Existing reconciliation and storage tests retain
coverage for observation state and lease ownership. The failure test still
checks the source diagnostic, empty program output, and retained operation.

The named selector rotation was removed from this recovery test because
`named_selectors_reach_the_same_workspace_across_commands` already verifies ID,
path, and claim resolution through open. The new regression retains the
positional ID route, which needs its outer transaction closed before recovery.

## Final Review

Review covers operation classification before takeover, compare-and-swap lease
ownership, typed error propagation, transaction boundaries, final removal and
admission checks, and claim and file preservation. No schema, status behavior,
existing public lifecycle API, or remote operation changes are included.

Final review found no remaining implementation blockers.

- `nix develop --command cargo test --all-targets --all-features --quiet`
  passed all 423 Rust tests.
- Repository checks passed for the changed code and documents, including
  formatting, Clippy, cargo check, static diagnostics, Markdown lint, prose lint,
  third-party notices, and runtime SQL boundaries.
- `openspec validate recover-expired-operations-on-open --strict --no-interactive`
  passed. The modified requirements preserve the existing scenario names and
  place the new recovery contract in a separate requirement.
- `git diff --check` passed.

Strict validation of all main specs also ran against the baseline: 11 of 12
specs reported existing requirement-length advisories. This change shortens the
edited workspace-open requirements; unrelated specifications are unchanged.
Post-archive `workspace-open` strict validation passed. Regular validation of
all 12 main specifications also passed. The change was archived as
`2026-10-06-recover-expired-operations-on-open` after implementation review and
complete test validation, and its requirements were synchronized into the main
specification. Archive tasks are complete. The final refinement and archive are
included in the follow-up commit.
