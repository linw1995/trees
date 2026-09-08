## ADDED Requirements

### Requirement: Select Pool or Workspace Status

The CLI SHALL provide `trees status [--view pools|workspaces] [--all]
[--json]`. The view SHALL default to `pools`. The pool view SHALL contain only
automatic repository-set pools with at least one current non-reclaimed
workspace. The workspace view SHALL contain individual automatic and manual
workspaces, exclude reclaimed records by default, and include reclaimed
tombstones with `--all`. The CLI SHALL reject `--all` unless the selected view
is `workspaces`.

#### Scenario: Default to Pool Allocation Status

- **WHEN** status is invoked without a view
- **THEN** it reports automatic repository-set pool allocation and capacity

#### Scenario: Select Workspace Details

- **WHEN** status is invoked with `--view workspaces`
- **THEN** it reports individual non-reclaimed manual and automatic workspaces

#### Scenario: Include Reclaimed Workspace Details

- **WHEN** status is invoked with `--view workspaces --all`
- **THEN** it additionally reports reclaimed workspace tombstones

#### Scenario: Reject All for Pool Status

- **WHEN** status is invoked with `--view pools --all` or `--all` without an
  explicit workspace view
- **THEN** it fails before opening lifecycle storage

### Requirement: Aggregate Automatic Pool Allocation

Each pool row SHALL represent one exact persisted repository-set pool.
Capacity SHALL count its automatic workspaces whose state is not `reclaimed`.
Allocated SHALL count capacity slots with an active workspace claim. Available
SHALL count capacity slots whose persisted state is `ready` and which have
neither an active workspace claim nor a retained operation lease. Allocated and
available SHALL be disjoint. Status SHALL NOT describe persisted availability
as live reusability.

#### Scenario: Count Allocated and Available Slots

- **WHEN** a pool contains two claimed slots, three ready slots without claims
  or operation leases, and one unclaimed degraded slot
- **THEN** status reports allocated `2`, available `3`, and capacity `6`

#### Scenario: Exclude an Active Unclaimed Operation

- **WHEN** a ready unclaimed slot has a retained operation lease
- **THEN** it contributes to capacity but not allocated or available

#### Scenario: Exclude Reclaimed and Manual Workspaces

- **WHEN** lifecycle storage contains reclaimed automatic workspaces and manual
  workspaces with the same repositories as a pool
- **THEN** neither contributes to that pool's current capacity

### Requirement: Display Repository Sets Compactly

The pool view SHALL load repositories through persisted pool-to-origin
relations and order them by canonical source path. Each repository label SHALL
use the shortest source-path suffix unique within that pool. Labels SHALL begin
as source-path base names. Conflicting labels SHALL expand by one parent
component at a time until unique. Pools SHALL use lexicographical order based
on their canonical source-path lists and then pool ID.

#### Scenario: Display Unique Repository Base Names

- **WHEN** one pool contains source repositories `/origins/api` and
  `/origins/web`
- **THEN** its repository labels are `api,web`

#### Scenario: Expand Conflicting Repository Labels

- **WHEN** one pool contains `/teams/one/api` and `/teams/two/api`
- **THEN** its repository labels are `one/api,two/api`

### Requirement: Render Pool and Workspace Human Views

The pool human view SHALL render `REPOS`, `ALLOCATED`,
`AVAILABLE/CAPACITY`, and `UPDATED`. `UPDATED` SHALL be the greatest
`updated_at` among current capacity slots. The workspace human view SHALL
render `STATE`, `USAGE`, `MODE`, `REPOS`, `RECONCILED`, and `PATH`. It SHALL
omit current operation details. Workspace `MODE` SHALL render as `🤖` for
automatic and `👤` for manual. Workspace `REPOS` SHALL render attached
repo-worktree count over total count followed by shortest unique source-path
labels.

Missing times SHALL render as `never`. Relative to `snapshot_at` in UTC, times
SHALL render as `HH:MM` on the same date, `MM-DD HH:MM` within the same year,
and `YYYY-MM-DD HH:MM` otherwise. Column alignment SHALL account for terminal
display width and SHALL NOT depend on color. An empty view SHALL print
`No workspace pools.` or `No workspaces.` as appropriate and succeed. Human
spacing SHALL NOT be a machine-readable contract.

#### Scenario: Render Compact Pool Capacity

- **WHEN** a pool for `api,web` has two allocated, three available, and six
  capacity slots
- **THEN** its human row contains `api,web`, `2`, and `3/6`

#### Scenario: Retain Paths in the Workspace View

- **WHEN** status uses the workspace view
- **THEN** each human row contains the canonical path identifying that slot

### Requirement: Provide View-Specific Versioned JSON

With `--json`, status SHALL write exactly one JSON document to standard output.
Every document SHALL contain integer `schema_version` equal to `1`, a `view`
string, and one `snapshot_at` timestamp. Pool JSON SHALL use `view = "pools"`
and contain a `pools` array. Each pool object SHALL contain `pool_id`, ordered
`repositories`, `allocated`, `available`, `capacity`, and nullable `updated_at`.
Each repository SHALL contain `origin_repository_id`, `source_path`, and
`label`.

Workspace JSON SHALL use `view = "workspaces"` and contain a `workspaces`
array. Each workspace SHALL contain `workspace_id`, `path`, `management_mode`,
`state`, lifecycle timestamps, nullable `pool_id`, nullable claim and current
operation details, and ordered repo-worktree snapshots. Diagnostics SHALL be
written only to standard error.

#### Scenario: Emit Pool JSON by Default

- **WHEN** status is invoked with `--json` and no explicit view
- **THEN** standard output is one version-1 pools document

#### Scenario: Emit Workspace Detail JSON

- **WHEN** status is invoked with `--view workspaces --json`
- **THEN** standard output is one version-1 workspaces document retaining paths,
  claims, current operations, and repo-worktree details

### Requirement: Read Status Without Side Effects

Status SHALL capture one snapshot timestamp and load every relationship needed
by the selected view through batched queries in one read-only SQLite
transaction. Status SHALL NOT open lifecycle storage for writing or append an
event. It SHALL NOT acquire or release a claim or start or recover an
operation. It SHALL NOT invoke Git or inspect workspace filesystem contents.

Persisted unhealthy states, claims, leases, and reclaimed rows in the explicit
workspace all-view SHALL be report data rather than command failures. Status
SHALL return nonzero only when arguments are invalid or it cannot load or
serialize a complete snapshot.

#### Scenario: Preserve State During Inspection

- **WHEN** either status view reports persisted lifecycle data
- **THEN** database contents, Git metadata, and workspace filesystem contents
  remain unchanged

#### Scenario: Report an Empty Installation

- **WHEN** status is invoked before the lifecycle database has been created
- **THEN** the selected view succeeds with an empty result and does not create
  the database

#### Scenario: Reject a Corrupt Lifecycle Database

- **WHEN** the lifecycle database exists but cannot provide the selected view
- **THEN** status writes an error to standard error, returns nonzero, and emits
  no partial JSON document
