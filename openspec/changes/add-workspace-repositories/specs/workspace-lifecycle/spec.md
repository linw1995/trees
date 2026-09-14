## ADDED Requirements

### Requirement: Persist Add Operations and Their Mutation Journal

Every admitted addition attempt SHALL append an immutable `operations` record with `kind = add`, a
versioned request intent, and a lifecycle event with running state under a workspace lease. The record SHALL
identify the workspace, requested inputs, offline policy, captured claim, and prior pool. The
execution plan SHALL be appended before workspace mutation and SHALL include origin identities, new
worktree IDs, selected revisions, prior membership and planned membership, destination paths, and any directory structure
relocation and staging ownership evidence. Each Git or filesystem mutation and each compensation
SHALL have a durable step intent before execution and an immutable observed-result event afterward.
Progress SHALL NOT rewrite the operation fact. Lease renewal SHALL protect long external steps, and
external work SHALL occur outside database transactions.

#### Scenario: Audit a Repeated Addition

- **WHEN** an admitted `add` finds that every requested repository is already present
- **THEN** it retains an addition operation and a successful terminal event describing the no-op

#### Scenario: Audit a Rejected Admitted Addition

- **WHEN** source resolution or preflight fails after operation admission
- **THEN** a failed addition operation remains auditable with the failure reason and retained-origin details

#### Scenario: Reject Mutations Without Records

- **WHEN** persisting the complete plan or next mutation intent fails
- **THEN** that external mutation is not attempted

#### Scenario: Record Directory Structure Promotion

- **WHEN** `add` relocates the original root worktree through staging to a named child
- **THEN** the journal identifies the original worktree, all relevant paths, and the intent and result of each move and container creation

### Requirement: Publish Add Membership and Terminal Events Atomically

After validating the complete resulting directory structure under the lease, `add` SHALL commit new associations,
relocated association paths, observed health, pool migration, an immutable workspace addition event,
a succeeded operation event, and lease removal in one short transaction. It SHALL check again the
captured claim and retain existing entity IDs. Results SHALL NOT report success before this
transaction commits. A dirty original worktree SHALL remain truthfully dirty rather than being
overwritten with ready health. Read-only observations during an unfinished `add` SHALL distinguish the
last committed snapshot from the in-flight operation.

#### Scenario: Fail Final Persistence

- **WHEN** all Git steps complete but the final membership transaction fails
- **THEN** no partial membership or pool migration is committed and the operation journal remains sufficient for compensation or recovery

#### Scenario: Lose Output After Commit

- **WHEN** successful publication commits but the caller does not receive standard output
- **THEN** a retry observes committed membership and succeeds without duplication without duplicating associations

### Requirement: Compensate Only Changes Owned by an Addition

An addition failure before publication SHALL record its failure and compensation intent, then
attempt to remove only newly created worktrees and restore any relocated original worktree. It SHALL
preserve the original worktree content and identity, prior membership, pool, and claim. Proven
complete compensation SHALL terminate as `rolled_back`; failures without workspace mutation SHALL
terminate as `failed`. A cleanup that cannot verify ownership or would discard user modifications
SHALL preserve files, record residual associations and actionable failure details, and mark the
workspace degraded without reporting successful compensation. Unresolved additions SHALL block further
membership mutation, release, and reuse until recovery or repair establishes consistent state. A
retry of a terminal failed addition SHALL use a new lease-owned recovery operation linked to the
original journal, rather than rewriting or reopening its terminal history. Published origin clones
and fetched refs SHALL NOT be represented as rolled back.

#### Scenario: Roll Back a Later Repository Failure

- **WHEN** one new worktree succeeds and a later addition fails
- **THEN** safe new worktrees are removed, any promoted original worktree is restored, and failure plus compensation events remain readable

#### Scenario: Preserve Newly Modified Content

- **WHEN** a new worktree contains user changes or its ownership cannot be verified during compensation
- **THEN** cleanup preserves it, records a failed residual association and degraded workspace, and retains the original claim and pool for repair

#### Scenario: Retry After Complete Compensation

- **WHEN** a prior addition has fully rolled back and the same repositories are requested again
- **THEN** retained audit records or tombstones do not prevent a new valid addition

### Requirement: Recover Interrupted Additions by Their Recorded Plan

Expired addition operations SHALL have addition-specific recovery under atomic lease takeover.
Recovery SHALL inspect intended and staging paths and Git identities before deciding an outcome,
including when the registered workspace path is temporarily absent during promotion. If all planned
final worktrees are valid, new worktrees match the selected revisions and are clean, and
compensation has not begun, recovery SHALL finish atomic publication and record `succeeded`. Partial
execution SHALL select compensation; a recorded compensation decision SHALL be resumed rather than
changed to success. Unsafe residual state SHALL be preserved and recorded as failed and degraded.
Every recovery outcome SHALL append an immutable recovery event without replaying unrelated create,
release, or removal behavior.

#### Scenario: Recover Git Completion Before Publication

- **WHEN** all intended final worktrees exist but the process stopped before final SQLite publication
- **THEN** recovery validates the recorded plan and atomically publishes the expanded membership and pool with a successful recovery event

#### Scenario: Recover a Missing Root During Promotion

- **WHEN** the original worktree is at recorded staging and the registered root is absent
- **THEN** an invocation targeting the persisted workspace can recover the operation and restore the original directory structure without losing the worktree

#### Scenario: Resume Interrupted Compensation

- **WHEN** the journal records compensation intent and the process stops during compensation
- **THEN** recovery continues safe compensation and records the resulting terminal outcome

#### Scenario: Reject Recovery While the Lease Is Active

- **WHEN** an addition lease is still valid or another process wins expired-lease takeover
- **THEN** the caller does not mutate the operation's worktrees or append a competing terminal outcome

## MODIFIED Requirements

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
  transaction. Recovery of an addition operation SHALL follow its durable addition plan and
  compensation decision, removing only safely verified worktrees created by
  that `add` and restoring any original-worktree relocation without deleting
  the original worktree. Recovery of other non-creation operations SHALL not
  remove a managed worktree; it SHALL reconcile the current state and append
  a terminal recovery event instead.

#### Scenario: Preserve Non-Add Recovery Boundaries

- **WHEN** an expired access, integration, or removal operation is recovered
- **THEN** recovery does not apply `add` compensation or delete existing managed worktrees
