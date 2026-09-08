## 1. Status Model and Storage

- [x] 1.1 Add serializable status projection types for workspace, claim,
  current operation, and repo-worktree snapshots
- [x] 1.2 Load all projection inputs in one read-only transaction with batched,
  deterministic repository queries
- [x] 1.3 Classify operation leases against one snapshot timestamp and cover
  active, expired, inconsistent, and absent cases

## 2. CLI and Rendering

- [x] 2.1 Add `trees status [--all] [--json]` parsing and dispatch
- [x] 2.2 Render deterministic human summaries, including empty state and
  reclaimed filtering
- [x] 2.3 Render the versioned JSON envelope without non-JSON stdout output

## 3. Verification and Documentation

- [ ] 3.1 Add integration coverage for mixed modes, health states, claims,
  operations, repo states, ordering, missing database, and failure exits
- [ ] 3.2 Prove status performs no database writes, lifecycle event appends,
  Git commands, filesystem observations, or operation recovery
- [ ] 3.3 Document status semantics and the distinction from `gc --dry-run`
- [ ] 3.4 Run complete Rust checks, repository hooks, strict OpenSpec
  validation, and Nix flake evaluation
