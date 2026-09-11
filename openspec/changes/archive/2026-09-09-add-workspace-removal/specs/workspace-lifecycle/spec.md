# Workspace Lifecycle Specification

## MODIFIED Requirements

### Requirement: Model Removed Lifecycle State

Workspace lifecycle state SHALL include `removed` in addition to `creating`,
`ready`, `degraded`, and `failed`. Repo-worktree lifecycle state SHALL include
`dirty` and `removed` in addition to `pending`, `attached`, `missing`,
`diverged`, and `failed`. A removed workspace and its removed repo-worktree
associations SHALL remain as immutable-history tombstones and SHALL not be
eligible for acquisition, ordinary workspace launch, garbage collection, or
explicit removal. GC and explicit removal SHALL write these tombstones only
after successful physical removal.

#### Scenario: Persist Successful Removal

- **WHEN** GC or explicit removal removes all managed worktrees and any
  remaining empty workspace directory
- **THEN** the workspace is `removed`, each removed repo-worktree association
  is `removed`, the removal timestamp is stored, and prior lifecycle
  events remain readable

#### Scenario: Preserve Partial Removal Failure

- **WHEN** GC or explicit removal removes only some physical worktrees before a
  later removal fails
- **THEN** the workspace is not marked `removed`, the partial states and
  failure details are persisted, and the failed operation remains auditable
