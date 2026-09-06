## ADDED Requirements

### Requirement: Persist Workspace Management and GC Timestamps

The workspace snapshot SHALL persist a management mode with the values
`automatic` and `manual`, an absolute canonical workspace path, an optional
UUID-backed repository-set pool ID, the last successful release time, and, when
applicable, the reclamation time. A workspace slot root SHALL be derived from
`canonical_path.parent()` and SHALL NOT be persisted. An automatic workspace SHALL
reference a pool registry row whose non-unique indexed hash and exact sorted
`repository_ids` identify the repository set; pool identity SHALL be
independent of the derived slot root. A manual workspace MAY leave the pool ID
null. The pool registry SHALL maintain explicit relations to origin
repositories, and the source path SHALL be stored on the origin repository
record rather than copied into each pool relation or worktree row. The
`repository_ids` payload SHALL be used for exact collision verification and
SHALL NOT be indexed or constrained as a unique key. The
management mode and pool ID SHALL be independent from workspace health and
active workspace claims. Existing legacy explicit-path workspace rows SHALL be
filled in as `manual` when this schema is introduced.

#### Scenario: Track an Automatic Workspace Idle Time

- **WHEN** an automatic workspace is successfully released
- **THEN** its last successful release timestamp is updated atomically with
  the claim release and release lifecycle event

#### Scenario: Preserve Manual Retention Policy

- **WHEN** a workspace is recorded with `manual` management mode
- **THEN** its mode remains in the current snapshot and no GC operation can
  select it as a reclamation candidate

#### Scenario: Preserve Legacy Workspace Mode

- **WHEN** the migration adds management mode and pool metadata to an existing
  legacy explicit-path workspace
- **THEN** it records `manual` without changing Git state or creating a claim;
  the row is not eligible for automatic pool allocation or GC

### Requirement: Normalize Origin Repository Relationships

The lifecycle database SHALL persist each origin repository once by its Git
common-directory identity and canonical source path. A repository-set pool SHALL
reference origin repositories through an explicit relation, and each managed
repo-worktree SHALL reference its origin repository rather than duplicating the
identity and source path. The canonical sorted origin repository ID set and
indexed hash SHALL remain available on the pool registry for exact pool
matching and collision verification.

#### Scenario: Reuse an Origin Repository Record

- **WHEN** two workspaces or pools use the same origin repository identity
- **THEN** they reference one origin repository record and do not copy its
  source path into each relationship row

### Requirement: Model Reclaimed Lifecycle State

Workspace lifecycle state SHALL include `reclaimed` in addition to
`creating`, `ready`, `degraded`, and `failed`. Repo-worktree lifecycle state
SHALL include `dirty` and `reclaimed` in addition to `pending`, `attached`,
`missing`, `diverged`, and `failed`. A reclaimed workspace and its reclaimed
repo-worktree associations SHALL remain as immutable-history tombstones and
SHALL not be eligible for acquisition or ordinary workspace launch.

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

### Requirement: Persist Active Workspace Claims

In addition to the workspace health snapshot, the system SHALL persist the
current usage claim in a `workspace_claims` table. The table SHALL contain at
most one row for each workspace, with a UUID v7 claim identifier, workspace
foreign key, and claim timestamp. A workspace with no active
claim is unclaimed; a workspace with an active claim is claimed. The claim
records persistent usage state for the workspace. It is not a database
transaction or a database lock and remains until the caller releases it. Access
availability SHALL remain independent from
`WorkspaceState` so a degraded workspace cannot become an eligible reusable
workspace merely by having no claim.

#### Scenario: Create an Active Claim

- **WHEN** a reusable workspace is successfully acquired
- **THEN** SQLite contains exactly one active claim for its workspace ID and
  the claim records the returned claim identifier

#### Scenario: Release an Active Claim

- **WHEN** the supplied claim identifier successfully releases its workspace
- **THEN** the active claim row is removed atomically with the terminal
  operation and release event, while workspace and repo-worktree identities
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

Acquire and release SHALL use the existing per-workspace
operation serialization. A request SHALL NOT replace an active claim or run
concurrently with another non-terminal workspace operation. Claim changes,
operation lease changes, and access lifecycle events SHALL use the existing
short Diesel transaction boundaries. Git and filesystem work SHALL occur
outside those transactions.

#### Scenario: Reject Access During an Active Operation

- **WHEN** a workspace has a non-terminal operation or an active claim with a
  different claim identifier
- **THEN** the access request fails without changing Git or the active claim

### Requirement: Keep SQLite Critical Sections Short

Access and mutation workflows SHALL hold SQLite transactions only while
appending operation facts, changing claims or current operation leases,
updating snapshots, and appending lifecycle events. They MUST NOT invoke Git
commands or filesystem operations from inside those transactions. Automatic
creation SHALL append each step intent before the external Git operation and
append its result afterward. Short metadata transactions SHALL acquire their
SQLite write boundary before metadata reads; a transient `busy` or `locked`
result MAY be retried within a bounded short transaction, but waiting and
retries SHALL NOT span external work. Operation lease renewal SHALL be one
short `operation_leases` update and SHALL NOT span external work.

#### Scenario: Run External Work Outside SQLite Transactions

- **WHEN** automatic creation or reconciliation performs a Git or filesystem
  operation
- **THEN** no SQLite transaction remains open while the external operation is
  running, and the operation can be followed by a short result transaction

### Requirement: Keep Operation Facts Append-Only

The `operations` table SHALL be append-only. An operation row SHALL contain
only immutable operation identity, workspace identity, kind, intent, and start
time. It SHALL NOT be updated with the current leaseholder, lease expiry,
pending step, terminal state, finish time, or error details. Operation starts,
steps, recoveries, and terminal transitions SHALL be appended to
`lifecycle_events`; the latest operation event is the authoritative operation
state.

#### Scenario: Preserve an Operation Fact

- **WHEN** an operation reaches a new step or terminal state
- **THEN** Trees appends a lifecycle event without updating the original
  `operations` row

### Requirement: Manage Current Operation Leases Separately

The system SHALL persist current operation lease state in an
`operation_leases` table. Each lease SHALL reference exactly one operation
through a unique `operation_id` relation, and SHALL carry a uniquely
constrained `workspace_id`; at most one active lease SHALL exist for an
operation, and at most one active operation lease SHALL exist for a workspace.
The lease row's primary-key `id` SHALL be the opaque lease token and SHALL be
paired with an expiration time. Lease renewal and takeover SHALL update only
this current lease row and SHALL use an atomic lease ID and expiry check;
`operation_id` is retained for reverse lookup and relationship integrity. All
lease-owned step and terminal state persistence SHALL address the current lease
by `lease_id` and derive the operation identity from that lease row; callers
SHALL NOT supply a second operation identifier for current lease mutation.
Lease changes SHALL NOT create a workspace claim or mutate the immutable
`operations` row.

#### Scenario: Renew the Current Operation Lease

- **WHEN** the current leaseholder renews an unexpired operation lease
- **THEN** only the matching `operation_leases` row is updated and no
  heartbeat event is appended

#### Scenario: Expired Operation Lease Takeover

- **WHEN** a lease has expired and a later invocation provides the current
  lease token
- **THEN** the lease row ID is atomically replaced, external state is observed,
  and the recovery transition is appended to `lifecycle_events`. When external
  state proves creation completed, the recovery terminal event and lease
  cleanup SHALL be committed together with the ready snapshot in one short
  transaction. Recovery of a non-creation operation SHALL not remove an
  a managed worktree; it SHALL reconcile the current state and append a
  terminal recovery event instead.

### Requirement: Renew Operation Leases During External Work

A long-running Git or filesystem step MAY renew the current operation lease
through a lease-token-checked short transaction. Operation lease renewal SHALL
protect the in-flight mutation from premature recovery; it SHALL NOT create or
extend a workspace claim. An expired operation MAY be recovered only after an
atomic operation ID, lease-token, and expiry check followed by fresh
external-state observation.

#### Scenario: Keep a Long External Step Owned

- **WHEN** an external Git operation outlives the current operation lease
- **THEN** the current leaseholder can renew the `operation_leases` row with a
  short renewal transaction, and another process cannot recover that operation
  while the lease-token check still succeeds

### Requirement: Record Access Events with Existing Lifecycle Identity

Access events SHALL use `entity_type = workspace` and the stable workspace ID;
they SHALL NOT introduce a second mutable identity for a reused workspace.
Event details SHALL include claim identifiers, operation context, GC age/candidate
counts, and a `forced` marker when applicable. Event history SHALL remain
append-only under the existing immutable event constraints.

#### Scenario: Preserve Access History Across Reuse

- **WHEN** the same workspace is claimed, released, and claimed
  again
- **THEN** the event log contains the ordered access transitions and no prior
  event or workspace identity is overwritten
