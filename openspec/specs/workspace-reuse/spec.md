# Workspace Reuse Specification

## Purpose

This capability defines how automatically managed workspaces are borrowed,
returned, and eventually reclaimed without allowing manual workspaces or
unsafe Git state to be affected by automation.

## Requirements

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
the workspace UUID. Persisted workspace paths SHALL be absolute. A slot root
SHALL be derived from `canonical_path.parent()` and SHALL not participate in
repository-set pool matching.

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
participate in repository-set pool allocation, workspace claim acquisition and
release, or garbage collection. Manual workspaces SHALL remain available to
existing direct workspace consumers but SHALL never be selected or deleted by
automated retention.

#### Scenario: Create an Automatic Workspace by Default

- **WHEN** a caller uses automatic create with repository arguments and no
  positional workspace path
- **THEN** the workspace is recorded as `automatic` and can later be acquired
  and considered by GC after its idle threshold is reached

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
- **THEN** existing rows are filled in as `manual` without changing Git
  state, deleting files, or creating an active claim

### Requirement: Allocate an Automatic Workspace from a Repository Pool

The CLI SHALL provide automatic creation as `trees create --repo
<repository-path>...` without a positional workspace path. The command
SHALL canonicalize the repositories, resolve their origin repository IDs,
derive the non-unique indexed hash and exact sorted `repository_ids` set, then
resolve a UUID-backed pool independent of workspace root. It SHALL search for
an idle automatic workspace referencing that pool, reconcile candidates before
selection, acquire a workspace claim for a reusable candidate, and provision a
new automatic workspace below the currently configured workspace root when no
safe candidate exists. The successful result SHALL include the allocated
workspace path and claim identifier.

#### Scenario: Allocate a Reusable Pool Slot

- **WHEN** automatic create receives repositories matching an idle automatic
  workspace whose worktrees are clean, detached, present, non-prunable,
  identity-matched, and at their recorded revisions
- **THEN** the command aligns that existing workspace to every repository's
  upstream `HEAD`, claims it, returns its path and a claim identifier, and does
  not create another workspace or worktree

#### Scenario: Select the Oldest Idle Slot

- **WHEN** multiple safe automatic workspaces have the exact repository-set
  pool ID
- **THEN** allocation selects the oldest `last_released_at`, falls back to
  `created_at` for never-released workspaces, and uses workspace UUID order
  as the deterministic tiebreaker

#### Scenario: Provision a New Slot

- **WHEN** no idle automatic workspace matches the exact repository set
- **THEN** Trees generates a path below its managed workspace root
- **AND** it creates detached worktrees using the repository-count-based layout
  at each upstream repository's current local `HEAD`
- **AND** upstream repositories and linked workspace repo inputs use the same
  revision resolution
- **AND** Trees records the pool UUID and returns the new workspace already
  claimed

#### Scenario: Retry a Pool Race

- **WHEN** another process claims a candidate after it was observed
  but before the allocation transaction commits
- **THEN** the command skips that candidate and retries the next matching
  candidate or provisions a new slot without creating a duplicate claim

#### Scenario: Reject an Unsafe Pool

- **WHEN** all matching workspaces are manual, claimed, active, degraded,
  failed, reclaimed, dirty, missing, prunable, diverged, or otherwise not
  reusable
- **THEN** automatic create provisions a new slot without mutating or cleaning
  any existing unsafe workspace

### Requirement: Persist One Active Workspace Claim

The system SHALL persist at most one active workspace claim for each
workspace. The claim SHALL contain a UUID v7 claim identifier, the workspace
ID, and a claim timestamp. The workspace ID SHALL be unique
in the active claim table. A workspace with no active claim is unclaimed; a
workspace with an active claim is unavailable for another acquisition. The
claim records persistent usage state for the workspace. It is not a database
transaction or a database lock and SHALL remain until the caller releases it.
Release SHALL identify the active claim from an explicit workspace-directory
or claim-identifier target, or from the current directory when neither target
is supplied. The claim identifier SHALL be treated as a local coordination
token rather than a security credential. This capability SHALL NOT infer an
abandoned claim from process liveness or replace it automatically.

Access-boundary reconciliation SHALL require the current operation lease. The
capability SHALL not expose a lease-free access-boundary entry point; general
lease-free reconciliation remains available only for non-access observation
contexts.

#### Scenario: Serialize Concurrent Acquisitions

- **WHEN** two processes attempt automatic create for the same reusable pool
  slot
- **THEN** at most one process receives a successful claim and the other
  receives a busy or transaction-conflict error without a second claim row

#### Scenario: Preserve Workspace Identity Across Reuse

- **WHEN** a workspace is released and later acquired again
- **THEN** its workspace ID, repo-worktree IDs, canonical paths, and Git
  worktree associations remain unchanged while a new claim identifier may be
  issued

### Requirement: Require a Reusable Worktree Snapshot

A managed repo worktree SHALL satisfy all these conditions to be reusable. Its
source repository identity and canonical path SHALL match the persisted
association. The worktree SHALL be present, not prunable, and detached. Its
`HEAD` SHALL match `last_head`. The command `git status
--porcelain=v1 --untracked-files=all` SHALL report no staged, unstaged, or
untracked changes. Ignored files SHALL NOT make a worktree dirty. Acquire SHALL
reconcile this predicate against Git's authoritative metadata before returning
success. Release SHALL establish this predicate through the release alignment
requirement before making the workspace available.

#### Scenario: Reject a Dirty Worktree

- **WHEN** a managed worktree contains staged, unstaged, or untracked changes
- **THEN** reconciliation records the worktree as `dirty`, marks the workspace
  as `degraded`, and acquire does not issue a claim

#### Scenario: Reject a Changed Detached Revision

- **WHEN** acquisition observes a detached managed worktree whose `HEAD`
  differs from `last_head`
- **THEN** reconciliation records the worktree as `diverged`, marks the
  workspace as `degraded`, and no new workspace claim is issued

#### Scenario: Leave External Changes for the Current Claim Holder

- **WHEN** release finds a dirty, missing, prunable, identity-mismatched, or
  failed worktree
- **THEN** release records a rejection, retains the current workspace claim,
  and does not align any managed worktree

### Requirement: Release Without Destroying Git State

The CLI SHALL accept `trees release [<workspace-dir>]` or `trees release
--claim-id <claim-id>`. The positional workspace directory and claim identifier
SHALL be mutually exclusive. An explicit workspace directory MAY be absolute
or relative; release SHALL resolve a relative directory against the process
current directory and select that exact managed workspace. When neither input
is supplied, release SHALL select the nearest managed workspace containing the
canonical current directory. A claim identifier SHALL select its active claim
and associated workspace.

Release SHALL snapshot the active claim and reconcile the workspace while
retaining that claim. Before changing any worktree, release SHALL verify that
every managed worktree is present, non-prunable, identity-matched, and reports
no staged, unstaged, or untracked changes. If every worktree passes preflight,
release SHALL align each worktree to its persisted origin repository's current
local `HEAD` in detached mode, update the recorded worktree head, and perform a
final reconciliation. Release SHALL remove the claim only after every aligned
worktree satisfies the reusable snapshot requirement. Release SHALL NOT fetch
remotes, delete branches or commits, or remove ignored files.

#### Scenario: Align a Clean Changed Worktree

- **WHEN** a claimed worktree is clean but branch-attached or at a revision
  different from its origin repository's current `HEAD`
- **THEN** release checks out the origin repository `HEAD` in detached mode,
  records the aligned head, and releases the claim after final reconciliation

#### Scenario: Release a Reusable Workspace

- **WHEN** one release target resolves an active claim and every managed
  worktree passes preflight and final reconciliation
- **THEN** the selected active claim is removed atomically, a release operation
  and immutable release event are recorded, and the workspace can be acquired
  again

#### Scenario: Reject Dirty Release Before Alignment

- **WHEN** any managed worktree has staged, unstaged, or untracked changes
- **THEN** release retains the active claim and does not align any managed
  worktree

#### Scenario: Retain the Claim After Alignment Failure

- **WHEN** Git alignment, persistence, or final reconciliation fails
- **THEN** release records the failure, retains the active claim, and does not
  report a successful release

#### Scenario: Release from a Workspace Descendant

- **WHEN** `trees release` runs without a target from a directory below a
  managed workspace
- **THEN** release selects the nearest containing managed workspace and its
  active claim

#### Scenario: Release a Relative Workspace Directory

- **WHEN** `trees release <workspace-dir>` receives a relative directory
- **THEN** release resolves it against the process current directory and
  selects that exact managed workspace

#### Scenario: Reject Invalid Target Combinations

- **WHEN** release receives both a workspace directory and claim-identifier
  target
- **THEN** argument parsing fails before any workspace state changes

#### Scenario: Reject an Unknown Claim Identifier

- **WHEN** the selected claim identifier is absent
- **THEN** release fails without releasing another claim or changing Git state

#### Scenario: Reject an Unknown Workspace Target

- **WHEN** the selected workspace is unclaimed or the selected path does not
  identify a managed workspace
- **THEN** release fails without releasing another claim or changing Git state

#### Scenario: Exit on Concurrent Release

- **WHEN** release cannot immediately acquire operation admission for the
  selected workspace
- **THEN** release returns a busy failure without waiting, retrying, or changing
  Git or claim state

### Requirement: Reclaim Idle Automatic Workspaces

The CLI SHALL provide `trees gc --older-than <duration> [--dry-run] [--yes]
[--force]`. GC SHALL calculate a UTC cutoff from the current time minus the
supplied duration and consider all `automatic` workspaces. The idle timestamp
SHALL be the last successful release time, or `created_at` when the workspace
has never been released.
Each candidate SHALL use `canonical_path.parent()` for root-containment and
unexpected-content checks; the current configuration root SHALL not filter
reuse or GC candidates.
GC SHALL report the total automatic workspaces, the number currently not
claimed, the number currently claimed, the number older than the
cutoff, and the number selected for reclamation. A normal non-dry-run SHALL
request confirmation before mutation unless `--yes` or `--force` is supplied.
`--yes` SHALL skip only the confirmation and SHALL retain the normal safety
filter. A dry run SHALL perform read-only inspection and SHALL NOT modify Git,
SQLite, or the filesystem.

The `--force` policy SHALL be scoped to the current GC invocation and SHALL
not be persisted as candidate or workspace snapshot state.

Without `--force`, GC SHALL select only workspaces with no active operation,
no claim, `ready` health, clean reusable worktrees, and no unexpected root
content. `--force` SHALL imply `--yes` and permit cleanup of age-qualified
automatic workspaces with dirty, diverged, missing, prunable, or unexpected
content. `--force` SHALL still refuse manual workspaces, young workspaces,
active claims, active operations, and paths whose source repository identity
cannot be verified. Both modes SHALL refuse a worktree that is branch-attached
or not detached. Forced cleanup SHALL record that it was forced.

#### Scenario: Preview GC Candidates Safely

- **WHEN** a caller runs `trees gc --older-than 30d --dry-run`
- **THEN** the command reports automatic, unclaimed, claimed,
  age-qualified, selected, and skipped counts with workspace paths and
  reasons, without removing worktrees, directories, claims, rows, or events

#### Scenario: Confirm a Normal GC Run

- **WHEN** a caller runs a non-dry-run GC without `--yes` or `--force` and the
  scan finds reclaimable automatic workspaces
- **THEN** the command displays how many workspaces are currently unclaimed
  and how many will be reclaimed, and performs no mutation until the caller
  confirms

#### Scenario: Skip Confirmation Without Forcing Cleanup

- **WHEN** a caller runs `trees gc --older-than 30d --yes`
- **THEN** the command displays the same counts, performs no interactive
  prompt, and reclaims only the normal clean automatic candidates

#### Scenario: Require Authorization Without Confirmation

- **WHEN** a caller runs a non-dry-run GC without confirmation, `--yes`, or
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
  claim or operation, and final reconciliation confirms every worktree is
  clean, detached, present, non-prunable, identity-matched, and at its
  recorded revision
- **THEN** after confirmation GC removes each worktree without a force flag,
  removes the empty workspace directory, marks the workspace and
  repo-worktree snapshots as `reclaimed`, and records a successful GC
  operation and reclamation event

#### Scenario: Preserve an Unsafe GC Candidate

- **WHEN** a candidate is dirty, diverged, missing, claimed, or the
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

- **WHEN** a forced GC scan encounters a manual workspace, an active claim, an
  active operation, a young workspace, or a path whose source repository
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

Every successful acquire, release, rejected release, and GC attempt that
reaches a per-workspace operation SHALL be
represented by an operation and an immutable lifecycle event. A `--dry-run` GC
inspection and a read-only candidate skip SHALL NOT create operations or
events. Access and GC events SHALL use the stable workspace entity identity
and SHALL include management mode, claim identifiers, operation context,
timestamps, age cutoff, unclaimed/claimed counts, and relevant
reconciliation or failure details as canonical JSON. Forced GC events SHALL
include `forced: true`. Claim or filesystem snapshot changes SHALL be committed
atomically with their access or reclamation events. Terminal operation events
and current lease cleanup SHALL be committed atomically in a short SQLite
transaction after the final external-state check; physical GC removal SHALL be
completed before a workspace is marked `reclaimed`. The operation fact itself
SHALL remain append-only.

#### Scenario: Audit a Release Rejection

- **WHEN** release is rejected because a worktree is dirty or diverged
- **THEN** a terminal operation-failed event is appended. The workspace and
  worktree snapshot reflect the observed health, the active claim remains, and
  the event log contains the rejection reason and claim identifier

#### Scenario: Audit Repeated Reconciliation

- **WHEN** acquire or release observes no change from the stored reusable
  snapshot
- **THEN** no duplicate external-change event is appended, while the access
  operation still appends its own successful terminal event

#### Scenario: Preserve Reclamation History

- **WHEN** GC successfully removes an automatic workspace
- **THEN** the workspace and repo-worktree rows remain as `reclaimed`
  tombstones, prior lifecycle events remain readable, and a later acquire
  cannot reuse the reclaimed record
