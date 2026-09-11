# Workspace Status Specification

## Purpose

This capability defines read-only pool capacity and workspace detail snapshots,
including deterministic human output and versioned JSON without lifecycle or
Git side effects.

## Requirements

### Requirement: Select Pool or Workspace Status

The CLI SHALL provide `trees status [--view pools|workspaces] [--all]
[--json]`. The view SHALL default to `pools`. The pool view SHALL contain only
automatic repository-set pools with at least one current non-removed
workspace. The workspace view SHALL contain individual automatic and manual
workspaces, exclude removed records by default, and include removed
tombstones with `--all`. The CLI SHALL reject `--all` unless the selected view
is `workspaces`.

#### Scenario: Default to Pool Allocation Status

- **WHEN** status is invoked without a view
- **THEN** it reports automatic repository-set pool allocation and capacity

#### Scenario: Select Workspace Details

- **WHEN** status is invoked with `--view workspaces`
- **THEN** it reports individual non-removed manual and automatic workspaces

#### Scenario: Include Removed Workspace Details

- **WHEN** status is invoked with `--view workspaces --all`
- **THEN** it additionally reports removed workspace tombstones

#### Scenario: Reject All for Pool Status

- **WHEN** status is invoked with `--view pools --all` or `--all` without an
  explicit workspace view
- **THEN** it fails before opening lifecycle storage

### Requirement: Aggregate Automatic Pool Allocation

Each pool row SHALL represent one exact persisted repository-set pool.
Capacity SHALL count its automatic workspaces whose state is not `removed`.
Available SHALL count capacity slots whose persisted state is `ready` and which
have neither an active workspace claim nor a retained operation lease.
Abnormal SHALL count capacity slots whose persisted state is `degraded` or
`failed`; `creating` SHALL be treated as transient rather than abnormal. Status
SHALL NOT describe persisted availability as live reusability.

#### Scenario: Count Available, Total, and Abnormal Slots

- **WHEN** a pool contains two claimed slots, three ready slots without claims
  or operation leases, and one unclaimed degraded slot
- **THEN** status reports available `3`, capacity `6`, and abnormal `1`

#### Scenario: Exclude an Active Unclaimed Operation

- **WHEN** a ready unclaimed slot has a retained operation lease
- **THEN** it contributes to capacity but not available or abnormal

#### Scenario: Exclude Removed and Manual Workspaces

- **WHEN** lifecycle storage contains removed automatic workspaces and manual
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

The pool human view SHALL render `REPOS`, `CAPACITY`, and `UPDATED`. Capacity
SHALL use `<available>/<total>/<abnormal>`. On an interactive terminal, the
available, total, and abnormal numbers SHALL be green, blue, and red. With
non-terminal standard output or `NO_COLOR`, status SHALL emit the same value
without ANSI escapes. The order SHALL define the meaning independently from
color. `UPDATED` SHALL be the greatest `updated_at` among current capacity
slots.

The workspace human view SHALL render `STATUS`, `MODE`, `REPOS`, `RECONCILED`,
and `ID`. `STATUS` SHALL render persisted workspace health and append `🔒`
when an active claim exists. An unclaimed workspace SHALL have no claim marker.
It SHALL omit current operation details.
Workspace `MODE` SHALL render as `🤖` for automatic and `👤` for manual.
Workspace `REPOS` SHALL render attached repo-worktree count over total count
followed by shortest unique source-path labels. The attached count SHALL be
presented as ready. On an interactive terminal, ready and total SHALL be green
and blue. With non-terminal standard output or `NO_COLOR`, status SHALL emit
the same `<ready>/<total>` value without ANSI escapes.

Each repository label SHALL reflect its persisted state. Attached labels SHALL
be green without a suffix. Pending labels SHALL be yellow with `(pending)`.
Dirty, missing, diverged, and failed labels SHALL be red with `(dirty)`,
`(missing)`, `(mismatch)`, and `(error)` respectively. Removed labels SHALL
be gray with `(removed)`. Non-terminal output and `NO_COLOR` SHALL retain the
same suffixes without ANSI escapes.

Before rendering, repository labels SHALL escape control characters, ANSI
escape bytes, backslashes, commas, and parentheses originating from persisted
paths. Generated state suffixes and ANSI colors SHALL be added only after this
escaping. Human output SHALL retain one physical line per workspace. JSON SHALL
retain original path values without human-display escaping.

Missing times SHALL render as `never`. Relative to `snapshot_at` in UTC, times
SHALL render as `HH:MM` on the same date, `MM-DD HH:MM` within the same year,
and `YYYY-MM-DD HH:MM` otherwise. Column alignment SHALL account for terminal
display width and SHALL NOT depend on color. An empty view SHALL print
`No workspace pools.` or `No workspaces.` as appropriate and succeed. Human
spacing SHALL NOT be a machine-readable contract.

#### Scenario: Render Compact Pool Capacity

- **WHEN** a pool for `api,web` has three available, six total, and one abnormal
  slot
- **THEN** its human row contains `api,web` and `3/6/1`

#### Scenario: Identify Workspace Details Using an Identifier

- **WHEN** status uses the workspace view
- **THEN** each human row contains the stable workspace ID and omits its path

#### Scenario: Color Workspace Repo Readiness

- **WHEN** workspace status writes to an interactive terminal and two of three
  repo worktrees are attached
- **THEN** `REPOS` renders `2/3` with `2` green and `3` blue

#### Scenario: Identify a Problem Repository

- **WHEN** one workspace repository is dirty
- **THEN** its label is red on an interactive terminal and carries a `(dirty)`
  suffix in colored and plain output

#### Scenario: Merge Workspace State and Usage

- **WHEN** a ready workspace has an active claim
- **THEN** its human `STATUS` value is `ready 🔒`

#### Scenario: Omit an Unclaimed Marker

- **WHEN** a degraded workspace has no active claim
- **THEN** its human `STATUS` value is `degraded` without an additional marker

### Requirement: Provide View-Specific Versioned JSON

With `--json`, status SHALL write exactly one JSON document to standard output.
Every document SHALL contain integer `schema_version` equal to `2`, a `view`
string, and one `snapshot_at` timestamp. Pool JSON SHALL use `view = "pools"`
and contain a `pools` array. Each pool object SHALL contain `pool_id`, ordered
`repositories`, `available`, `capacity`, `abnormal`, and nullable `updated_at`.
Each repository SHALL contain `origin_repository_id`, `source_path`, and
`label`.

Workspace JSON SHALL use `view = "workspaces"` and contain a `workspaces`
array. Each workspace SHALL contain `workspace_id`, `path`, `management_mode`,
`state`, lifecycle timestamps, nullable `pool_id`, nullable claim and current
operation details, and ordered repo-worktree snapshots. Diagnostics SHALL be
written only to standard error.

#### Scenario: Emit Pool JSON by Default

- **WHEN** status is invoked with `--json` and no explicit view
- **THEN** standard output is one version-2 pools document

#### Scenario: Emit Workspace Detail JSON

- **WHEN** status is invoked with `--view workspaces --json`
- **THEN** standard output is one version-2 workspaces document retaining paths,
  claims, current operations, and repo-worktree details

### Requirement: Read Status Without Side Effects

Status SHALL capture one snapshot timestamp and load every relationship needed
by the selected view through relational joins in one read-only SQLite
transaction. It SHALL NOT expand all selected entity IDs into a single `IN`
expression. Status SHALL NOT open lifecycle storage for writing or append an
event. It SHALL NOT acquire or release a claim or start or recover an operation.
It SHALL NOT invoke Git or inspect workspace filesystem contents.

Persisted unhealthy states, claims, leases, and removed rows in the explicit
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
