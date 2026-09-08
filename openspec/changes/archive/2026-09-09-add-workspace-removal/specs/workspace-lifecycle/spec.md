# Workspace Lifecycle Specification

## MODIFIED Requirements

### Requirement: Model Reclaimed Lifecycle State

Workspace lifecycle state SHALL include `reclaimed` in addition to `creating`,
`ready`, `degraded`, and `failed`. Repo-worktree lifecycle state SHALL include
`dirty` and `reclaimed` in addition to `pending`, `attached`, `missing`,
`diverged`, and `failed`. A reclaimed workspace and its reclaimed repo-worktree
associations SHALL remain as immutable-history tombstones and SHALL not be
eligible for acquisition, ordinary workspace launch, garbage collection, or
explicit removal. GC and explicit removal SHALL write these tombstones only
after successful physical removal.

#### Scenario: Persist Successful Reclamation

- **WHEN** GC or explicit removal removes all managed worktrees and any
  remaining empty workspace directory
- **THEN** the workspace is `reclaimed`, each removed repo-worktree association
  is `reclaimed`, the reclamation timestamp is stored, and prior lifecycle
  events remain readable

#### Scenario: Preserve Partial Reclamation Failure

- **WHEN** GC or explicit removal removes only some physical worktrees before a
  later removal fails
- **THEN** the workspace is not marked `reclaimed`, the partial states and
  failure details are persisted, and the failed operation remains auditable
