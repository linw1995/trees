## Context

See `proposal.md` for motivation. Reconciliation currently observes branch, revision, identity, existence, and cleanliness, but release treats every non-ready observation as a rejection. Git mutations run outside short SQLite transactions under a renewable operation lease.

## Goals / Non-Goals

**Goals:**

- Preserve all staged, unstaged, and untracked claimant work by rejecting dirty release before any alignment.
- Make a successful release leave every managed worktree clean, detached, and at its origin repository's current `HEAD`.
- Keep claim release, idle timestamp, terminal operation state, and release history atomic after final reconciliation.

**Non-Goals:**

- Fetching remotes or selecting a remote-tracking branch.
- Deleting local branches or commits.
- Removing ignored files.
- Repairing missing, prunable, or identity-mismatched worktrees.

## Decisions

### Preflight Every Worktree Before Alignment

Release resolves each persisted origin repository's current `HEAD` and validates every associated worktree before mutating any worktree. A worktree must be present, non-prunable, identity-matched, and clean according to `git status --porcelain=v1 --untracked-files=all`. This prevents a later dirty worktree from being discovered after earlier worktrees have already moved.

### Detach Worktrees from Local Branches

For each validated worktree, release checks out the corresponding persisted source repository's current `HEAD` with `--detach`. This removes branch attachment and aligns the files and revision without deleting the branch or its commits. The local origin repository is authoritative; release does not fetch or infer a remote branch.

### Persist Alignment Steps and Reconcile Again

Each successful checkout records an aligned attached snapshot and refreshes the operation lease. A final lease-owned reconciliation proves that all worktrees match the new recorded heads before the existing atomic release transaction removes the claim. A Git, persistence, or final reconciliation failure records release rejection and retains the claim.

## Risks / Trade-Offs

- [The origin repository `HEAD` may move during a multi-worktree release] -> Resolve each target during preflight and rely on final reconciliation against the recorded target; a later origin movement affects the next release or allocation, not the in-flight snapshot.
- [An external process may dirty a worktree after preflight] -> Use non-forced checkout and final reconciliation; failure retains the claim.
- [A failure can leave a subset of clean worktrees aligned] -> Record each completed step and retain the claim so retry can safely converge without losing uncommitted work.
