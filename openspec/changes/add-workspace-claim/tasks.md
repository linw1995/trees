# Tasks

## 1. CLI and Target Selection

- [x] 1.1 Add claim arguments with positional path, `--workspace-id`, and `--json`; verify parser tests cover defaults, selector conflicts, malformed IDs, and rejection of `--workspace-dir` and `--claim-id`.
- [x] 1.2 Convert claim inputs into shared typed selectors without exposing other named options; verify exact-root, relative-path, nearest-ancestor, ID selection without current-directory access, and direct-input conflict tests.

Command dispatch was connected in task 4.1 after the workflow became available.

## 2. Admission and Structural Validation

- [x] 2.1 Add lease-owned claim orchestration and typed errors in the existing workspace module; verify manual, missing, removed, creating, failed, already-claimed, retained-lease, and unresolved-journal targets are rejected without fallback.
- [x] 2.2 Implement structural checks independent of clean/detached reuse checks; verify pool membership, source identity, worktree identity and registration, missing/prunable worktrees, and incomplete multi-repository layouts.
- [x] 2.3 Preserve dirty content, branches, and changed revisions while reconciling health; verify staged and unstaged diffs, untracked and ignored bytes, `HEAD` values, refs, and worktree associations remain unchanged on success and rejection.

## 3. Atomic Publication and Recovery

- [x] 3.1 Atomically publish the claim, access event, terminal success, and lease cleanup after final validation; verify transaction fault tests leave neither a partial claim nor a partial successful outcome and preserve stable workspace metadata.
- [x] 3.2 Preserve committed claims across output loss and support existing non-creation recovery for interrupted claims; verify interruption before publication, expired lease, lease takeover, output failure, and repeated claim behavior without Git mutation.
- [x] 3.3 Add deterministic multi-connection race tests for claim versus claim, automatic allocation, and GC/removal; verify exclusive admission, no duplicate claim, and no target fallback.

## 4. Output and Lifecycle Integration

- [x] 4.1 Add the dedicated result type, Bash-safe output, and JSON serialization without changing create output; verify all four fields, special-character paths, standard error diagnostics, and success output only after commit.
- [x] 4.2 Exercise claim, status, allocation/GC exclusion, and ordinary release end to end; verify dirty release retains the claim, clean release succeeds, and manual workspace and explicit forced-removal policies remain unchanged.

## 5. Documentation and Validation

- [ ] 5.1 Update README and workspace lifecycle documentation with exact CLI forms, in-place preservation, recovery limitations, output fields, and existing release semantics; verify examples against generated help.
- [ ] 5.2 Run targeted integration tests and repository checks (`nix develop --command prek -a`), plus the Rust test suite in the project environment; resolve failures and record results.
- [ ] 5.3 Run `openspec validate add-workspace-claim --strict` and review the final implementation against every delta scenario before marking tasks complete.
