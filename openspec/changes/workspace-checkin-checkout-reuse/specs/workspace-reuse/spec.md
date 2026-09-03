## Purpose

This capability defines how automatically managed workspaces are borrowed,
returned, and eventually reclaimed without allowing manual workspaces or
unsafe Git state to be affected by automation.

## ADDED Requirements

### Requirement: Resolve the Automatic Workspace Directory

The system SHALL resolve a configurable Trees-managed `workspaces_dir` and
SHALL not require a concrete workspace path for automatic creation. The CLI
SHALL provide `trees config set workspaces-dir <path>` and SHALL persist the
normalized absolute path in the Trees configuration. If no value is
configured, the first version SHALL use `$XDG_DATA_HOME/trees/workspaces`,
with `~/.local/share/trees/workspaces` as the Linux fallback,
`~/Library/Application Support/trees/workspaces` on macOS, and
`%LOCALAPPDATA%\\trees\\workspaces` on Windows. The lifecycle database SHALL
remain in the existing platform state directory. Trees SHALL create the
resolved directory lazily and generate each automatic workspace below it from
the workspace UUID. Persisted workspace paths and workspace-root namespaces
SHALL be absolute.

#### Scenario: Generate an Automatic Workspace Path

- **WHEN** automatic create cannot find a reusable pool slot
- **THEN** Trees creates a new path below the resolved `workspaces_dir` and
  returns it without requiring a caller-supplied workspace path

#### Scenario: Keep Workspace Content Separate from Lifecycle State

- **WHEN** automatic allocation creates a workspace
- **THEN** its Git worktrees are stored below `workspaces_dir`, while
  lifecycle records remain in the shared `db.sqlite` state location

#### Scenario: Normalize a Configured Workspace Directory

- **WHEN** a caller configures `workspaces_dir` with a relative path
- **THEN** Trees resolves it according to the configuration-file rules and
  persists the resulting absolute path before using it for pool allocation

### Requirement: Distinguish Automatic and Manual Workspaces

The system SHALL persist a workspace management mode with the values
`automatic` and `manual`. The mode SHALL be inferred from the creation shape:
a request without a positional workspace path is `automatic`, while a request
with an explicit workspace path is `manual`. The selected mode SHALL be
preserved in the lifecycle snapshot. Only automatic workspaces SHALL
participate in repository-set pool allocation, checkout leasing, checkin, or
garbage collection. Manual workspaces SHALL remain available to existing
direct workspace consumers but SHALL never be selected or deleted by
automated retention.

#### Scenario: Create an Automatic Workspace by Default

- **WHEN** a caller uses automatic create with repository arguments and no
  positional workspace path
- **THEN** the workspace is recorded as `automatic` and can later be checked
  out and considered by GC after its idle threshold is reached

#### Scenario: Infer Manual Mode from an Explicit Path

- **WHEN** a caller creates a workspace with an explicit path
- **THEN** the workspace is recorded as `manual` and is excluded from pool
  allocation and GC

#### Scenario: Preserve a Manual Workspace

- **WHEN** a caller creates a workspace with an explicit path and later runs
  automatic create or GC
- **THEN** the workspace is excluded from pool allocation and skipped by GC,
  and its worktrees and files remain untouched

#### Scenario: Preserve Legacy Workspace Records

- **WHEN** the lifecycle migration adds the management mode to existing Trees
  workspaces created through the legacy explicit-path form
- **THEN** existing rows are backfilled as `manual` without changing Git
  state, deleting files, or creating an active lease

### Requirement: Allocate an Automatic Workspace from a Repository Pool

The CLI SHALL provide automatic creation as `trees create --repo
<repository-path>...` without a positional workspace path. The command
SHALL canonicalize the repositories, resolve a UUID-backed pool using its
indexed hash and exact canonical JSON set of Git common-directory identities,
and search for an idle automatic workspace referencing that pool. It SHALL
reconcile candidates before selection,
acquire a checkout lease for a reusable candidate, and provision a new
automatic workspace below the Trees-managed workspace root when no safe
candidate exists. The successful result SHALL include the allocated workspace
path and checkout identifier.

#### Scenario: Allocate a Reusable Pool Slot

- **WHEN** automatic create receives repositories matching an idle automatic
  workspace whose worktrees are clean, detached, present, non-prunable,
  identity-matched, and at their recorded revisions
- **THEN** the command leases that existing workspace, returns its path and a
  checkout identifier, and does not create another workspace or worktree

#### Scenario: Select the Least Recently Checked-In Slot

- **WHEN** multiple safe automatic workspaces have the exact repository-set
  pool key
- **THEN** allocation selects the oldest `last_checked_in_at`, falls back to
  `created_at` for never-checked-in workspaces, and uses workspace UUID order
  as the deterministic tie breaker

#### Scenario: Provision When No Safe Slot Exists

- **WHEN** no idle automatic workspace matches the exact repository set
- **THEN** Trees generates a path below its managed workspace root, creates
  direct-child detached worktrees from each repository's current `HEAD`,
  records the pool UUID, and returns the new workspace already checked out

#### Scenario: Retry a Pool Race

- **WHEN** another process acquires a candidate lease after it was observed
  but before the allocation transaction commits
- **THEN** the command skips that candidate and retries the next matching
  candidate or provisions a new slot without creating a duplicate lease

#### Scenario: Reject an Unsafe Pool

- **WHEN** all matching workspaces are manual, leased, active, degraded,
  failed, reclaimed, dirty, missing, prunable, diverged, or otherwise not
  reusable
- **THEN** automatic create provisions a new slot without mutating or cleaning
  any existing unsafe workspace

### Requirement: Persist One Active Checkout Lease

The system SHALL persist at most one active checkout lease for each workspace.
The lease SHALL contain a UUID v7 checkout identifier, the workspace ID, an
owner identity, acquisition time, lease expiry, and last heartbeat time. The
workspace ID SHALL be unique in the active lease table. The checkout
identifier SHALL be required to renew or check in the lease and SHALL be
treated as a local coordination token rather than a security credential.

#### Scenario: Serialize Concurrent Checkout Calls

- **WHEN** two processes attempt automatic create for the same reusable pool
  slot
- **THEN** at most one process receives a successful lease and the other
  receives a busy or transaction-conflict error without a second lease row

#### Scenario: Preserve Workspace Identity Across Reuse

- **WHEN** a workspace is checked in and later checked out again
- **THEN** its workspace ID, repo-worktree IDs, canonical paths, and Git
  worktree associations remain unchanged while a new checkout identifier may
  be issued

### Requirement: Require a Reusable Worktree Snapshot

A managed repo worktree SHALL be reusable only when its source repository
identity and canonical worktree path match the persisted association, the
worktree is present and not prunable, the worktree is detached, its `HEAD`
matches `last_head`, and `git status --porcelain=v1 --untracked-files=all`
reports no staged, unstaged, or untracked changes. Ignored files SHALL NOT
make a worktree dirty. Checkout and checkin SHALL reconcile this predicate
against Git's authoritative metadata before returning success.

#### Scenario: Reject a Dirty Worktree

- **WHEN** a managed worktree contains staged, unstaged, or untracked changes
- **THEN** reconciliation records the worktree as `dirty`, marks the workspace
  `degraded`, and checkout or checkin does not make the workspace available

#### Scenario: Reject a Changed Detached Revision

- **WHEN** a caller commits in a managed detached worktree so its `HEAD`
  differs from the recorded `last_head`
- **THEN** reconciliation records the worktree as `diverged`, marks the
  workspace `degraded`, and no new checkout lease is issued

#### Scenario: Leave External Changes for the Current Owner

- **WHEN** checkin finds a dirty, missing, prunable, diverged, or failed
  worktree
- **THEN** checkin records a rejection, retains the current checkout lease,
  and performs no reset, clean, branch change, or worktree removal

### Requirement: Check In Without Destroying Git State

The CLI SHALL provide `trees checkin <workspace-path> --checkout-id
<checkout-id>`. Checkin SHALL require the active lease identifier, reconcile
the workspace while retaining the lease, and release the lease only when all
managed worktrees satisfy the reusable snapshot requirement. A successful
checkin SHALL leave the workspace directory, worktree files, source
repositories, and worktree associations unchanged.

#### Scenario: Check In a Reusable Workspace

- **WHEN** the supplied checkout identifier owns the active lease and all
  managed worktrees pass the final reconciliation
- **THEN** the active lease is removed atomically, a checkin operation and
  immutable checkin event are recorded, and the workspace can be checked out
  again

#### Scenario: Reject an Unknown Checkout Identifier

- **WHEN** the path has no active lease or the supplied identifier does not
  match the active lease
- **THEN** checkin fails without releasing another caller's lease or changing
  Git state

### Requirement: Renew and Recover Checkout Leases

The CLI SHALL accept the current checkout identifier on automatic `trees create
--repo <repository-path>... --checkout-id <checkout-id>` to renew the lease for
the matching repository set. Renewal SHALL extend the
lease by the configured checkout duration, whose default SHALL be 24 hours,
and SHALL not mutate Git. An expired lease SHALL be reclaimable only after
reconciliation proves the workspace reusable; an unexpired lease SHALL never
be force-reclaimed by this capability.

#### Scenario: Renew an Owned Lease

- **WHEN** the supplied checkout identifier matches the active lease
- **THEN** checkout succeeds with the same identifier and a later expiry,
  without creating a second lease or changing any worktree

#### Scenario: Reclaim a Safe Expired Lease

- **WHEN** an active lease is expired and reconciliation finds a reusable
  workspace
- **THEN** the old lease expiry and the new checkout acquisition are recorded
  atomically, the old lease is removed, and the caller receives a new active
  lease

#### Scenario: Refuse an Unsafe Expired Lease

- **WHEN** an expired lease exists but reconciliation finds dirty, missing,
  prunable, diverged, or failed worktrees
- **THEN** the expired lease is recorded and removed, the workspace remains
  degraded and unavailable for checkout, and no new lease is returned

### Requirement: Reclaim Idle Automatic Workspaces

The CLI SHALL provide `trees gc --older-than <duration> [--dry-run] [--yes]
[--force]`. GC SHALL calculate a UTC cutoff from the current time minus the
supplied duration and consider only `automatic` workspaces in the current
workspace-root namespace. The idle timestamp SHALL be the last successful
checkin time, or `created_at` when the workspace has never been checked in.
GC SHALL report the total automatic workspaces, the number currently not
checked out, the number currently checked out, the number older than the
cutoff, and the number selected for reclamation. A normal non-dry-run SHALL
request confirmation before mutation unless `--yes` or `--force` is supplied.
`--yes` SHALL skip only the confirmation and SHALL retain the normal safety
filter. A dry run SHALL perform read-only inspection and SHALL NOT modify Git,
SQLite, or the filesystem.

Without `--force`, GC SHALL select only workspaces with no active operation or
lease, `ready` health, clean reusable worktrees, and no unexpected root
content. `--force` SHALL imply `--yes` and permit cleanup of age-qualified
automatic workspaces with dirty, diverged, missing, prunable, or unexpected
content. `--force` SHALL still refuse manual workspaces, young workspaces,
unexpired leases, active operations, and paths whose source repository
identity cannot be verified. Forced cleanup SHALL record that it was forced.

#### Scenario: Preview GC Candidates Safely

- **WHEN** a caller runs `trees gc --older-than 30d --dry-run`
- **THEN** the command reports automatic, not-checked-out, checked-out,
  age-qualified, selected, and skipped counts with workspace paths and
  reasons, without removing worktrees, directories, leases, rows, or events

#### Scenario: Confirm a Normal GC Run

- **WHEN** a caller runs a non-dry-run GC without `--yes` or `--force` and the
  scan finds reclaimable automatic workspaces
- **THEN** the command displays how many workspaces are currently not checked
  out and how many will be reclaimed, and performs no mutation until the
  caller confirms

#### Scenario: Skip Confirmation Without Forcing Cleanup

- **WHEN** a caller runs `trees gc --older-than 30d --yes`
- **THEN** the command displays the same counts, performs no interactive
  prompt, and reclaims only the normal clean automatic candidates

#### Scenario: Require Explicit Non-Interactive Authorization

- **WHEN** a non-interactive caller runs a non-dry-run GC without `--yes` or
  `--force`
- **THEN** the command fails before mutation and instructs the caller to use
  `--dry-run`, `--yes`, or `--force`

#### Scenario: Never Collect a Manual Workspace

- **WHEN** an automatic GC scan finds an idle manual workspace older than the
  cutoff
- **THEN** it reports the workspace as skipped and does not invoke any Git or
  filesystem removal for that workspace

#### Scenario: Reclaim an Idle Workspace

- **WHEN** an automatic workspace is older than the cutoff, has no active
  lease or operation, and final reconciliation confirms every worktree is
  clean, detached, present, non-prunable, identity-matched, and at its
  recorded revision
- **THEN** after confirmation GC removes each worktree without a force flag,
  removes the empty workspace directory, marks the workspace and
  repo-worktree snapshots as `reclaimed`, and records a successful GC
  operation and reclamation event

#### Scenario: Preserve an Unsafe GC Candidate

- **WHEN** a candidate is dirty, diverged, missing, prunable, leased, or the
  workspace root contains an unexpected entry
- **THEN** normal GC skips or fails that candidate, reports the reason, and
  leaves every remaining file and worktree association in place

#### Scenario: Force Reclaim an Unsafe Automatic Workspace

- **WHEN** a caller runs `trees gc --older-than 30d --force` and an
  age-qualified automatic workspace has dirty or diverged worktrees, or
  unexpected content below its managed root
- **THEN** GC may use forced worktree removal and remove that workspace's
  unexpected content, records `forced: true`, and marks the workspace
  `reclaimed` only after all requested physical removal succeeds

#### Scenario: Keep Force Within Ownership and Identity Boundaries

- **WHEN** a forced GC scan encounters a manual workspace, an unexpired lease,
  an active operation, a young workspace, or a path whose source repository
  identity cannot be verified
- **THEN** it skips the workspace, reports the reason, and does not remove its
  files or worktree metadata

#### Scenario: Record a Partial GC Failure

- **WHEN** one worktree is removed but a later worktree or the empty workspace
  root cannot be removed
- **THEN** GC stops, records the partial removal and failure details, marks
  the workspace degraded or failed as appropriate, and does not report the
  workspace as successfully reclaimed

### Requirement: Record Workspace Access and Reclamation Lifecycle

Every successful checkout, renewal, checkin, rejected checkin, expired lease
recovery, and GC attempt that reaches a per-workspace operation SHALL be
represented by an operation and an immutable lifecycle event. A `--dry-run` GC
inspection and a read-only candidate skip SHALL NOT create operations or
events. Access and GC events SHALL use the stable workspace entity identity
and SHALL include management mode, checkout identifiers, owner identities,
timestamps, age cutoff, not-checked-out/checked-out counts, and relevant
reconciliation or failure details as canonical JSON. Forced GC events SHALL
include `forced: true`. Lease or filesystem snapshot changes and terminal
operation state SHALL be committed atomically with their event in a short
SQLite transaction; physical GC removal SHALL be completed before a workspace
is marked `reclaimed`.

#### Scenario: Audit a Checkin Rejection

- **WHEN** checkin is rejected because a worktree is dirty or diverged
- **THEN** the operation is failed, the workspace and worktree snapshot
  reflects the observed health, the active lease remains, and the event log
  contains the rejection reason and checkout identifier

#### Scenario: Audit Repeated Reconciliation

- **WHEN** checkout or checkin observes no change from the stored reusable
  snapshot
- **THEN** no duplicate external-change event is appended, while the access
  operation still records its own successful transition

#### Scenario: Preserve Reclamation History

- **WHEN** GC successfully removes an automatic workspace
- **THEN** the workspace and repo-worktree rows remain as `reclaimed`
  tombstones, prior lifecycle events remain readable, and a later checkout
  cannot reuse the reclaimed record
