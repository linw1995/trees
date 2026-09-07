## Why

Automatic workspace release currently rejects clean worktrees after normal branch or commit activity because their Git state no longer matches the allocation snapshot. Claim holders should not need to restore that reusable state manually, while release must still protect uncommitted work.

## What Changes

- Reject release before mutation when any managed worktree has staged, unstaged, or untracked changes.
- Align every clean managed worktree to its origin repository's current `HEAD` in detached mode during release.
- Reconcile the aligned worktrees before atomically releasing the claim.
- Retain the claim when validation, alignment, or final reconciliation fails.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-reuse`: Change release from requiring an already reusable snapshot to producing one from clean managed worktrees.

## Impact

Release behavior, Git worktree helpers, lifecycle events, tests, README documentation, and the workspace reuse contract change. No database migration or external dependency is required.
