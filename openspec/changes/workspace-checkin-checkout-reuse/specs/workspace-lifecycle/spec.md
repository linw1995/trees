## ADDED Requirements

### Requirement: Persist Workspace Management and GC Timestamps

The workspace snapshot SHALL persist a management mode with the values
`automatic` and `manual`, an optional canonical repository-set pool key, an
optional absolute workspace-root namespace, the last successful checkin time,
and, when applicable, the reclamation time. An automatic workspace SHALL have
a pool key derived from the sorted Git common-directory identities and an
absolute workspace-root namespace; a manual workspace MAY leave both fields
null. The management mode, pool key, and workspace-root namespace SHALL be
independent from workspace health and active checkout leases. Existing legacy
explicit-path workspace rows SHALL be backfilled as `manual` when this schema
is introduced.

#### Scenario: Track an Automatic Workspace Idle Time

- **WHEN** an automatic workspace is successfully checked in
- **THEN** its last successful checkin timestamp is updated atomically with
  the lease release and checkin lifecycle event

#### Scenario: Preserve Manual Retention Policy

- **WHEN** a workspace is recorded with `manual` management mode
- **THEN** its mode remains in the current snapshot and no GC operation can
  select it as a reclamation candidate

#### Scenario: Preserve Legacy Workspace Mode

- **WHEN** the migration adds management mode and pool metadata to an existing
  legacy explicit-path workspace
- **THEN** it records `manual` without changing Git state or creating a lease;
  the row is not eligible for automatic pool allocation or GC

### Requirement: Model Reclaimed Lifecycle State

Workspace lifecycle state SHALL include `reclaimed` in addition to
`creating`, `ready`, `degraded`, and `failed`. Repo-worktree lifecycle state
SHALL include `dirty` and `reclaimed` in addition to `pending`, `attached`,
`missing`, `diverged`, and `failed`. A reclaimed workspace and its reclaimed
repo-worktree associations SHALL remain as immutable-history tombstones and
SHALL not be eligible for checkout or ordinary workspace launch.

#### Scenario: Persist Successful Reclamation

- **WHEN** GC removes all managed worktrees and the empty workspace directory
- **THEN** the workspace is `reclaimed`, each removed repo-worktree association
  is `reclaimed`, the reclamation timestamp is stored, and prior lifecycle
  events remain readable

#### Scenario: Preserve Partial Reclamation Failure

- **WHEN** GC removes only some physical worktrees before a later removal
  fails
- **THEN** the workspace is not marked `reclaimed`, the partial states and
  failure details are persisted, and the failed GC operation remains
  auditable

### Requirement: Persist Active Workspace Checkout Leases

In addition to the workspace health snapshot, the system SHALL persist the
current checkout claim in a `workspace_leases` table. The table SHALL contain
one row at most for each workspace, with a UUID v7 checkout identifier,
workspace foreign key, owner identity, checkout time, lease expiry, and last
heartbeat time. A workspace with no active lease is unclaimed; a workspace
with an active lease is checked out. Access availability SHALL remain
independent from `WorkspaceState` so a degraded workspace cannot become an
eligible reusable workspace merely by having no lease.

#### Scenario: Create an Active Lease

- **WHEN** a reusable workspace is successfully checked out
- **THEN** SQLite contains exactly one active lease for its workspace ID and
  the lease records the returned checkout identifier and expiry

#### Scenario: Release an Active Lease

- **WHEN** its owning checkout identifier successfully checks in a workspace
- **THEN** the active lease row is removed atomically with the terminal
  operation and checkin event, while workspace and repo-worktree identities
  remain intact

### Requirement: Observe Dirty Worktrees in Lifecycle State

The repo-worktree lifecycle state SHALL include `dirty` in addition to
`pending`, `attached`, `missing`, `diverged`, and `failed`. Reconciliation
SHALL use a read-only Git status observation to distinguish a present worktree
with local changes from an attached clean worktree. A `dirty` worktree SHALL
make its workspace `degraded`, and repeated observations of the same dirty
fingerprint SHALL be idempotent.

#### Scenario: Persist a Dirty Observation

- **WHEN** reconciliation sees local staged, unstaged, or untracked changes
  in a present managed worktree
- **THEN** the repo-worktree snapshot is `dirty`, the workspace snapshot is
  `degraded`, and an external-change event contains the observed cleanliness
  details

### Requirement: Serialize Access Operations with Workspace Operations

Checkout, renewal, checkin, and expired-lease recovery SHALL use the existing
per-workspace operation serialization. A request SHALL NOT replace an
unexpired lease or run concurrently with another non-terminal workspace
operation. Lease changes, operation transitions, and access lifecycle events
SHALL use the existing short Diesel transaction boundaries.

#### Scenario: Reject Access During an Active Operation

- **WHEN** a workspace has a non-terminal operation or an unexpired checkout
  lease owned by another checkout identifier
- **THEN** the access request fails without changing Git or the active lease

### Requirement: Record Access Events with Existing Lifecycle Identity

Access events SHALL use `entity_type = workspace` and the stable workspace ID;
they SHALL NOT introduce a second mutable identity for a reused workspace.
Event details SHALL include lease-specific identifiers, operation context, GC
age/candidate counts, and a `forced` marker when applicable. Event history
SHALL remain append-only under the existing immutable event constraints.

#### Scenario: Preserve Access History Across Reuse

- **WHEN** the same workspace is checked out, checked in, and checked out
  again
- **THEN** the event log contains the ordered access transitions and no prior
  event or workspace identity is overwritten
