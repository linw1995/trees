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
- [x] 5.2 Cover base-name conflicts and recursive parent expansion, update
  documentation, and rerun the complete quality gates

## 6. Human Path Placement

- [x] 6.1 Keep canonical workspace paths out of the pool view and retain them
  only in the explicit workspace detail view
- [x] 6.2 Update output tests and documentation for the view-specific path
  contract

## 7. Pool Allocation View

- [x] 7.1 Add automatic repository-pool allocation, availability, and capacity
  projections alongside the existing workspace projection
- [x] 7.2 Add `--view pools|workspaces`, default to pools, and restrict `--all`
  to the workspace view
- [x] 7.3 Emit versioned JSON matching the selected pool or workspace view
- [x] 7.4 Update integration tests and documentation, then rerun the complete
  quality gates

## 8. Capacity Presentation

- [x] 8.1 Replace allocated and available-capacity columns with one
  available-total-abnormal capacity value and terminal-aware colors
- [x] 8.2 Update pool JSON, tests, and documentation, then rerun the complete
  quality gates

## 9. Workspace Repo Colors

- [x] 9.1 Color ready and total repo counts in the workspace view under the
  existing terminal-aware color policy
- [x] 9.2 Update tests and documentation, then rerun the complete quality gates

## 10. Workspace Status and Repo Labels

- [x] 10.1 Merge workspace state and usage into one status column
- [x] 10.2 Color repo labels by state and append friendly suffixes to non-ready
  repositories
- [ ] 10.3 Update tests and documentation, then rerun the complete quality gates

## 11. Workspace Claim Indicator

- [x] 11.1 Replace claimed and unclaimed text with an optional `🔒` claim marker
- [ ] 11.2 Update tests and documentation, then rerun the complete quality gates
