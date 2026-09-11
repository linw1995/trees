# Workspace Status Specification

## Purpose

This capability defines read-only pool capacity and workspace detail snapshots,
including deterministic human output and versioned JSON without lifecycle or
Git side effects.

## Requirements

### Requirement: Select Pool or Workspace Status

The CLI SHALL provide `trees status [WORKSPACE_ID] [--view pools|workspaces|repos]
[--all] [--json]`. The view SHALL default to `pools`. The pool view SHALL contain only
automatic repository-set pools with at least one current non-removed
workspace. The workspace view SHALL contain individual automatic and manual
workspaces, exclude removed records by default, and include removed
tombstones with `--all`. The CLI SHALL reject `--all` unless the selected view
is `workspaces`. The repos view SHALL continue to report the global source
repository inventory. Target selection SHALL NOT filter any global inventory.

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

#### Scenario: Select Origin Repositories

- **WHEN** status uses `--view repos`
- **THEN** it lists all stored origins using their existing identity and path

#### Scenario: Select a Target with Any View

- **WHEN** a valid workspace ID is supplied with any supported view
- **THEN** it selects the summary target while the view retains its global inventory

#### Scenario: Reject All for Repo Status

- **WHEN** status is invoked with `--view repos --all`
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
escaping. The workspace inventory table SHALL retain one physical line per workspace. JSON SHALL
retain original path values without human-display escaping.

In the inventory tables, missing times SHALL render as `never`. Relative to `snapshot_at` in UTC, times
SHALL render as `HH:MM` on the same date, `MM-DD HH:MM` within the same year,
and `YYYY-MM-DD HH:MM` otherwise. Column alignment SHALL account for terminal
display width and SHALL NOT depend on color. An empty view SHALL print
`No workspace pools.` or `No workspaces.` as appropriate and succeed. Human
spacing SHALL NOT be a machine-readable contract.

A resolved target SHALL precede the inventory as specified by the target summary
requirement. Existing inventory table columns and compact UTC timestamps SHALL
remain unchanged. With a summary, one blank line and the heading `Pools`,
`Workspaces`, or `Repositories` SHALL separate it from the selected inventory.
Without a target, inventory output SHALL remain unchanged with no added heading
or blank line.

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

#### Scenario: Keep the Target in the Workspace Inventory

- **WHEN** the target also belongs to the filtered workspace inventory
- **THEN** it appears both in the summary and in its normal inventory row

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

Every view SHALL also contain `target_workspace`, either a complete workspace
object with the same fields as a workspace inventory entry or null when current directory
matches no workspace. Repo JSON SHALL retain `view = "repos"` and its `repos`
array with `origin_repository_id`, `source_path`, `repository_identity`, and
`label`. All target and inventory data SHALL share the envelope's snapshot time.
Human-only formatting SHALL NOT change JSON path or timestamp values.

#### Scenario: Emit Pool JSON by Default

- **WHEN** status is invoked with `--json` and no explicit view
- **THEN** standard output is one version-2 pools document

#### Scenario: Emit Workspace Detail JSON

- **WHEN** status is invoked with `--view workspaces --json`
- **THEN** standard output is one version-2 workspaces document retaining paths,
  claims, current operations, and repo-worktree details

#### Scenario: Include a Target in Every JSON View

- **WHEN** status resolves a target with any view and `--json`
- **THEN** one version-2 document contains that complete target and the selected
  global inventory, with no human summary text

#### Scenario: Emit a Null Target Outside a Workspace

- **WHEN** no explicit ID is supplied and current directory belongs to no workspace
- **THEN** `target_workspace` is null and the selected JSON inventory is unchanged

#### Scenario: Include a Removed Target Independently of All

- **WHEN** an explicit ID selects a removed workspace without `--all`
- **THEN** `target_workspace.state` is `removed` and `removed_at` is retained
- **AND** the default workspace inventory still omits removed records

### Requirement: Read Status Without Side Effects

Status SHALL capture one snapshot timestamp and load every relationship needed
by the selected view and target through relational joins in one read-only SQLite
transaction. It SHALL NOT expand all selected entity IDs into a single `IN`
expression. Status SHALL NOT open lifecycle storage for writing or append an
event. It SHALL NOT acquire or release a claim or start or recover an operation.
It SHALL NOT invoke Git or inspect workspace filesystem contents. Without an
explicit ID, it SHALL resolve the invocation directory canonically solely for
target selection. It SHALL NOT probe a stored target path or require it to exist.

Persisted unhealthy states, claims, leases, and removed rows in the explicit
workspace all-view or target summary SHALL be report data rather than command failures. Status
SHALL return nonzero only when arguments are invalid, an explicit ID is unknown, or it cannot
resolve the invocation directory, load, or serialize a complete snapshot.

#### Scenario: Preserve State During Inspection

- **WHEN** either status view reports persisted lifecycle data
- **THEN** database contents, Git metadata, and workspace filesystem contents
  remain unchanged

#### Scenario: Report an Empty Installation

- **WHEN** status is invoked without an explicit ID before the lifecycle database has been created
- **THEN** the selected view succeeds with an empty result and does not create
  the database

#### Scenario: Reject a Corrupt Lifecycle Database

- **WHEN** the lifecycle database exists but cannot provide the selected view
- **THEN** status writes an error to standard error, returns nonzero, and emits
  no partial JSON document

#### Scenario: Keep Target and Inventory Consistent

- **WHEN** a concurrent writer changes a target claim while status is loading
- **THEN** target claim details and inventory allocation reflect the same
  database snapshot, never a combination of before and after states

#### Scenario: Reject an Explicit Target in an Empty Installation

- **WHEN** an ID is supplied before the lifecycle database has been created
- **THEN** status reports the unknown workspace on standard error, returns nonzero,
  emits no standard output, and does not create storage

### Requirement: Render Existing Repository Metadata

The repos human view SHALL render `REPO`, `PATH`, and `ID`, ordered by source
path then ID. Labels SHALL use shortest unique path suffixes and existing
terminal escaping. JSON SHALL retain the version-2 envelope with `view: repos`
and a `repos` array containing `origin_repository_id`, `source_path`,
`repository_identity`, and `label`. No mode, registration state, root, or remote
URL SHALL be included. The view SHALL require no origin schema changes and
SHALL NOT invoke Git, run migrations, or recover clone operations. `--all`
SHALL remain unsupported for repos. Missing storage SHALL yield an empty view.

#### Scenario: Require the Current Lifecycle Schema

- **WHEN** repos status reads a database with pending lifecycle migrations
- **THEN** it reports a required schema upgrade without changing that database

#### Scenario: Inspect a Missing Source

- **WHEN** a stored source path is missing
- **THEN** repos status still lists its stored identity and path without probing Git

### Requirement: Resolve the Target Workspace Independently of View

An explicit `WORKSPACE_ID` SHALL select that stored workspace regardless of current directory,
management mode, claim, operation, health, removed state, or path existence.
Without an ID, status SHALL select the nearest stored workspace whose canonical
path equals or contains canonical current directory by path components. Target lookup SHALL
include removed records and SHALL NOT depend on `--all`. An unknown explicit
ID SHALL produce a nonzero error on standard error and no standard output. An unmatched current directory
SHALL silently omit the human summary and leave the selected inventory intact.

#### Scenario: Resolve Nested Repository Directories

- **WHEN** status runs within a repository subdirectory inside a registered workspace
- **THEN** that workspace is selected for every view

#### Scenario: Explicit Identifier Selection

- **WHEN** current directory belongs to one workspace and a different valid ID is supplied
- **THEN** the supplied ID selects the target

#### Scenario: Resolve the Nearest Boundary

- **WHEN** multiple registered workspace paths contain canonical current directory
- **THEN** the closest ancestor is selected, including a removed boundary

#### Scenario: Avoid String Prefix Matches

- **WHEN** current directory is `/workspaces/api-extra` and only `/workspaces/api` is registered
- **THEN** it does not select that workspace

#### Scenario: Resolve a Symlinked Invocation Directory

- **WHEN** current directory is reached through a symlink into a registered workspace
- **THEN** canonical current directory resolves to that workspace

#### Scenario: Stay Silent Outside a Workspace

- **WHEN** no ID is supplied and no stored workspace contains current directory
- **THEN** no summary, no summary placeholder, no extra blank line, and no
  diagnostic is emitted; only the existing selected inventory is rendered

#### Scenario: Inspect a Missing Removed Target Path

- **WHEN** the supplied ID identifies a removed workspace whose directory is absent
- **THEN** status reports its persisted removed state successfully

### Requirement: Render a Compact Target Workspace Summary

Human output SHALL begin with `Workspace (current directory)` for an inferred
target or `Workspace (selected by ID)` for an explicit target. The summary SHALL
list ID, Path, Status, Mode, optional Operation, Repos, and Reconciled in order.
Status SHALL use persisted health with `🔒` appended only for an active claim.
There SHALL be no separate Claim row. Mode SHALL use `automatic 🤖` or
`manual 👤`. Repos SHALL reuse inventory readiness counts, labels, state suffixes,
and color rules. Removed health SHALL render as `removed`.

Operation SHALL be omitted when absent; otherwise it SHALL show operation kind,
state, and lease classification as `KIND / STATE (lease active|expired|inconsistent)`.
Reconciled SHALL use `never` or local `YYYY-MM-DD HH:MM:SS` without timezone text;
if platform local timezone information is unavailable, it SHALL use UTC without
a suffix. All persisted human text SHALL escape terminal control characters
before generated color is applied. `NO_COLOR` and non-terminal output SHALL
contain no generated ANSI escapes. Emoji SHALL follow their associated text.

#### Scenario: Render a Claimed Automatic Workspace

- **WHEN** the target is ready, automatic, and claimed
- **THEN** Status is `ready 🔒`, Mode is `automatic 🤖`, and no Claim row appears

#### Scenario: Render an Unclaimed Manual Workspace

- **WHEN** the target is manual without a claim or operation
- **THEN** Mode is `manual 👤`, Status has no lock, and Operation is absent

#### Scenario: Display a Retained Expired Operation

- **WHEN** a target retains a running release operation with an expired lease
- **THEN** Operation is `release / running (lease expired)` without triggering recovery

#### Scenario: Display Reconciliation in Local Time

- **WHEN** reconciliation is `2026-09-11T06:32:05Z` and the local offset for that
  instant is eight hours ahead of UTC
- **THEN** Reconciled is `2026-09-11 14:32:05` with no timezone suffix

#### Scenario: Display Missing Reconciliation History

- **WHEN** the target has no reconciliation timestamp
- **THEN** Reconciled is `never`

#### Scenario: Escape a Target Path

- **WHEN** a target path contains a newline or terminal escape byte
- **THEN** it is displayed as escaped text on one field line without injecting
  new terminal lines or control sequences
