## ADDED Requirements

### Requirement: List Persisted Workspace Status

The CLI SHALL provide `trees status [--all] [--json]`. By default, the command
SHALL list every managed workspace whose persisted state is not `reclaimed`,
including both `automatic` and `manual` management modes. With `--all`, it
SHALL also list reclaimed workspace tombstones. Workspaces SHALL be ordered by
canonical workspace path, and repo worktrees within a workspace SHALL be
ordered by canonical worktree path.

#### Scenario: List Current Managed Workspaces

- **WHEN** status is invoked without options and lifecycle storage contains
  automatic, manual, and reclaimed workspaces
- **THEN** it reports the automatic and manual workspaces in canonical-path
  order and omits the reclaimed workspaces

#### Scenario: Include Reclaimed Tombstones

- **WHEN** status is invoked with `--all`
- **THEN** it additionally reports workspaces whose persisted state is
  `reclaimed`

#### Scenario: Report an Empty Installation

- **WHEN** status is invoked before the lifecycle database has been created or
  no workspaces match
- **THEN** it succeeds with an empty result and does not create the database

### Requirement: Separate Health, Usage, and Operation Activity

Each reported workspace SHALL retain its persisted health state and management
mode. Status SHALL independently report usage as `claimed` when an active
workspace claim exists and `unclaimed` otherwise. When an operation lease
exists, status SHALL report the operation identity, kind, latest state, lease
identity, lease expiry, and a lease status derived against one snapshot time.
It SHALL classify a running operation lease as `active` when its expiry is
later than the snapshot time and `expired` otherwise. It SHALL classify a lease
retained for a terminal operation as `inconsistent`. It SHALL NOT report an
unclaimed workspace as available or reusable without access-boundary
reconciliation.

#### Scenario: Report a Claimed Healthy Workspace

- **WHEN** a ready workspace has an active claim and no operation lease
- **THEN** status reports health `ready`, usage `claimed`, the claim details,
  and no current operation

#### Scenario: Report an Expired Running Operation

- **WHEN** a workspace has a running operation whose lease expiry is at or
  before the status snapshot time
- **THEN** status reports the operation lease as `expired` without recovering
  or taking over the operation

#### Scenario: Avoid an Unsupported Reusability Claim

- **WHEN** an automatic workspace is ready, unclaimed, and has no current
  operation in SQLite
- **THEN** status reports those facts without asserting that current Git and
  filesystem state make the workspace reusable

### Requirement: Read Status Without Side Effects

Status SHALL capture one snapshot timestamp and load workspace, claim,
operation lease and fact, latest operation state, and repo-worktree records in
one read-only SQLite transaction. All lease-expiry classifications SHALL use
that timestamp. Status SHALL NOT open lifecycle storage for writing, append an
event, acquire or release a claim, start or recover an operation, invoke Git,
or inspect workspace filesystem contents.

#### Scenario: Preserve State During Inspection

- **WHEN** status reports degraded workspaces or expired operation leases
- **THEN** database contents, Git metadata, and workspace filesystem contents
  remain unchanged

#### Scenario: Observe One Database Snapshot

- **WHEN** another process commits a claim or operation change while status is
  loading its report
- **THEN** every relationship in the returned report comes from one consistent
  SQLite snapshot rather than a mixture of states before and after that commit

### Requirement: Render a Human Workspace Summary

Without `--json`, status SHALL render one row per workspace containing the
persisted workspace state, claim usage, current operation summary, management
mode, repo-worktree state summary, last reconciliation time, and canonical
workspace path. Missing reconciliation time SHALL be rendered as `never`.
Output meaning SHALL NOT depend on terminal color. Empty results SHALL print
`No workspaces.` and succeed. Human table spacing SHALL NOT be a
machine-readable compatibility contract.

#### Scenario: Summarize Repository States

- **WHEN** a workspace has attached, dirty, and missing repo worktrees
- **THEN** its human row reports the total and each nonzero repo-worktree state
  count without hiding the workspace health state

#### Scenario: Report Unhealthy State Successfully

- **WHEN** the report contains a degraded workspace, dirty worktree, or expired
  lease
- **THEN** the command renders that state and exits successfully

### Requirement: Provide Versioned JSON Status

With `--json`, status SHALL write exactly one JSON document to stdout. The
document SHALL contain integer `schema_version` equal to `1`, one
`snapshot_at` timestamp, and a `workspaces` array. Each workspace object SHALL
contain `workspace_id`, `path`, `management_mode`, `state`, `created_at`,
`updated_at`, `last_reconciled_at`, `last_released_at`, `reclaimed_at`, nullable
`pool_id`, nullable `claim`, nullable `current_operation`, and
`repo_worktrees`. A claim SHALL contain `claim_id` and `claimed_at`. A current
operation SHALL contain `operation_id`, `kind`, `state`, `lease_id`,
`lease_expires_at`, and `lease_status`. Each repo worktree SHALL contain
`repo_worktree_id`, `origin_repository_id`, `source_path`, `worktree_path`,
`state`, `last_head`, and `last_observed_at`. Identifiers and timestamps SHALL
be strings, absent optional values SHALL be JSON `null`, arrays SHALL preserve
the required ordering, and diagnostics SHALL be written only to stderr.

#### Scenario: Emit Structured Workspace Details

- **WHEN** status is invoked with `--json` for a claimed workspace with a
  current operation and multiple repo worktrees
- **THEN** stdout is one schema-versioned JSON document containing the claim,
  operation lease classification, and every ordered repo-worktree snapshot

#### Scenario: Emit an Empty JSON Result

- **WHEN** status is invoked with `--json` and no workspaces match
- **THEN** stdout contains a valid version-1 document with an empty
  `workspaces` array

### Requirement: Fail Only When Status Cannot Be Produced

Status SHALL return a nonzero exit only when it cannot open or read an existing
lifecycle database, construct a consistent projection, or serialize the
selected output. Persisted unhealthy states, claims, active or expired
operations, and reclaimed rows included by `--all` SHALL be report data rather
than command failures.

#### Scenario: Reject a Corrupt Lifecycle Database

- **WHEN** the lifecycle database exists but cannot provide a valid status
  snapshot
- **THEN** status writes an error to stderr, returns nonzero, and emits no
  partial JSON document
