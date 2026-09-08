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
- [x] 2.3 Render the versioned JSON envelope without non-JSON standard output

## 3. Verification and Documentation

- [x] 3.1 Add integration coverage for mixed modes, health states, claims,
  operations, repo states, ordering, missing database, and failure exits
- [x] 3.2 Prove status performs no database writes, lifecycle event appends,
  Git commands, filesystem observations, or operation recovery
- [x] 3.3 Document status semantics and the distinction from `gc --dry-run`
- [x] 3.4 Run complete Rust checks, repository hooks, strict OpenSpec
  validation, and Nix flake evaluation

## 4. Human Output Refinement

- [x] 4.1 Remove the operation column, render attached repo availability
  against total capacity, shorten UTC timestamps, and use emoji for management
  mode
- [x] 4.2 Update rendering and integration tests, refresh documentation, and
  rerun the complete quality gates

## 5. Repository Labels

- [x] 5.1 Append deterministic shortest-unique source-path suffixes to the
  availability and capacity summary
- [ ] 5.2 Cover base-name conflicts and recursive parent expansion, update
  documentation, and rerun the complete quality gates
