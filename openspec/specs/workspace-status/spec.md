# Workspace Status Specification

## Purpose

This capability defines read-only pool capacity and workspace detail snapshots,
plus live process observations for a selected workspace. It includes deterministic
human output and versioned JSON without lifecycle or Git side effects.

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

The workspace human view SHALL render `STATUS`, `MODE`, `REPOS`, `SIZE`,
`RECONCILED`, and `ID`, followed by `LATEST SESSION` when the session hook is
enabled for a nonempty inventory, as specified by workspace-session-hook.
`STATUS` SHALL render persisted workspace health and append `🔒` when an active
claim exists. An unclaimed workspace SHALL have no claim marker. It SHALL omit
current operation details. Workspace `MODE` SHALL render as `🤖` for automatic
and `👤` for manual. Workspace `REPOS` SHALL render attached repo-worktree count
over total count followed by shortest unique source-path labels. The attached
count SHALL be presented as ready. On an interactive terminal, ready and total
SHALL be green and blue. With non-terminal standard output or `NO_COLOR`, status
SHALL emit the same `<ready>/<total>` value without ANSI escapes. `SIZE` SHALL
render the row's disk usage observation as specified by Render Workspace Disk Usage.

Each repository label SHALL reflect its persisted state. Attached labels SHALL
be green without a suffix. Pending labels SHALL be yellow with `(pending)`.
Dirty, missing, diverged, and failed labels SHALL be red with `(dirty)`,
`(missing)`, `(mismatch)`, and `(error)` respectively. Removed labels SHALL
be gray with `(removed)`. Non-terminal output and `NO_COLOR` SHALL retain the
same suffixes without ANSI escapes.

Before rendering, repository labels SHALL escape control characters, ANSI
escape bytes, backslashes, commas, and parentheses originating from persisted
paths. Generated state suffixes and ANSI colors SHALL be added only after this
escaping. The workspace inventory table SHALL retain one physical line per
workspace. JSON SHALL retain original path values without human-display escaping.

In the inventory tables, missing times SHALL render as `never`. Relative to
`snapshot_at` in UTC, times SHALL render as `HH:MM` on the same date,
`MM-DD HH:MM` within the same year, and `YYYY-MM-DD HH:MM` otherwise. Column
alignment SHALL account for terminal display width and SHALL NOT depend on
color. An empty view SHALL print `No workspace pools.` or `No workspaces.` as
appropriate and succeed. Human spacing SHALL NOT be a machine-readable
contract.

A resolved target SHALL precede the inventory as specified by the target summary
requirement. Pool and repository inventory columns and compact UTC timestamps
SHALL remain unchanged; the workspace inventory SHALL add only `SIZE` before
`RECONCILED`, and the enabled session hook SHALL still append its column. With a
summary, one blank line and the heading `Pools`, `Workspaces`, or
`Repositories` SHALL separate it from the selected inventory. Without a target,
the selected inventory SHALL still have no added heading or blank line.

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

#### Scenario: Show Size for Every Workspace Row

- **WHEN** status uses `--view workspaces` with or without a selected target
- **THEN** every displayed row has a `SIZE` value from that workspace's disk
  usage observation, including removed rows displayed with `--all`

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
operation details, and ordered repo-worktree snapshots. Fatal diagnostics SHALL be
written only to standard error. Nonfatal process observation issues SHALL be
encoded in the report without additional standard output or standard error text.

Every view SHALL also contain `target_workspace`, either a complete workspace
object with the same fields as a workspace inventory entry or null when current directory
matches no workspace. Repo JSON SHALL retain `view = "repos"` and its `repos`
array with `origin_repository_id`, `source_path`, `repository_identity`, and
`label`. All persisted target and inventory data SHALL share the envelope's snapshot time.
Every view SHALL additionally contain nullable top-level `target_processes` as
specified by the process observation JSON requirement. Its `observed_at` SHALL
be independent of `snapshot_at`; it SHALL NOT change the workspace object shape.
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

Status SHALL capture one snapshot timestamp and load every persisted relationship and registered workspace boundary needed
by the selected view and target through relational joins in one read-only SQLite
transaction. It SHALL NOT expand all selected entity IDs into a single `IN`
expression. Status SHALL NOT open lifecycle storage for writing or append an
event. It SHALL NOT acquire or release a claim or start or recover an operation.
Trees itself SHALL NOT invoke Git, inspect workspace file contents, or traverse
workspace directory trees. It SHALL read cached disk usage from the same
persisted snapshot as workspace and repository metadata. A user-configured session query hook
SHALL be permitted after the lifecycle connection closes, as specified by
workspace-session-hook. The hook is user-controlled code, not a sandboxed or
guaranteed side-effect-free operation; Trees SHALL NOT persist its results or
use them for lifecycle decisions. It SHALL permit read-only `OS` process metadata
access and `cwd` path resolution solely for process attribution after the database
transaction. Without an explicit ID, it SHALL resolve the invocation directory
canonically solely for target selection. Target selection and persisted reporting
SHALL NOT require the stored target path to exist. A missing path SHALL NOT
trigger a size scan or change an already stored observation during status.

Persisted unhealthy states, claims, leases, and removed rows in the explicit
workspace all-view or target summary SHALL be report data rather than command failures. Status
SHALL return nonzero only when arguments are invalid, an explicit ID is unknown, or it cannot
resolve the invocation directory, load persisted status, or serialize the report.
Session hook failures SHALL produce unavailable or partial observation data and
SHALL NOT change an otherwise successful exit status.
Process observation failures SHALL produce partial or unavailable report data
and SHALL NOT change an otherwise successful exit status. No process observer
SHALL run before successful persisted status loading or without a target.
Unknown or failed cached disk usage SHALL be report data, not command failures.
Malformed stored cache data SHALL be a persisted status loading error rather
than a reason to scan the filesystem.
Registered boundaries used for process attribution SHALL share the persisted snapshot.

#### Scenario: Preserve State During Inspection

- **WHEN** a status view reports persisted lifecycle data without an external hook
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

#### Scenario: Observe Outside the Persisted Transaction

- **WHEN** status resolves a target and loads its persisted snapshot
- **THEN** it ends the read-only database transaction before process observation
- **AND** process observation does not alter claims, health, capacity, or lifecycle state

#### Scenario: Preserve Status When Observation Fails

- **WHEN** process enumeration fails after a valid persisted snapshot has loaded
- **THEN** status succeeds with an unavailable process observation and intact inventory

#### Scenario: Skip Unneeded Observation

- **WHEN** no target exists or argument, target, or database loading fails
- **THEN** status does not enumerate processes

#### Scenario: Skip Unneeded Disk Usage Observation

- **WHEN** no target or workspace inventory row needs disk usage, or argument,
  target, or database loading fails
- **THEN** status does not traverse workspace directories

- **AND** a successful status invocation also does not traverse workspace
  directories when size cells are present

#### Scenario: Observe Sessions After Closing Storage

- **WHEN** an enabled workspace session hook is eligible to run
- **THEN** Trees closes its lifecycle database connection before starting the hook
- **AND** hook failure leaves persisted status and the successful exit status intact

#### Scenario: Preserve Removed Target Details Without Its Directory

- **WHEN** an explicit selector resolves a removed workspace whose directory is absent
- **THEN** status retains its persisted target and inventory data and reports
  its cached disk usage or `unknown` without probing the missing directory

### Requirement: Render Existing Repository Metadata

The repos human view SHALL render `REPO`, `PATH`, `SIZE`, and `ID`, ordered by source
path then ID. Labels SHALL use shortest unique path suffixes and existing
terminal escaping. JSON SHALL retain the version-2 envelope with `view: repos`
and a `repos` array containing `origin_repository_id`, `source_path`,
`repository_identity`, `label`, and `disk_usage`. No mode,
registration state, root, or remote URL SHALL be included. The view SHALL read
the stored origin size observation and SHALL NOT invoke Git, run migrations,
or recover clone operations. `--all`
SHALL remain unsupported for repos. Missing storage SHALL yield an empty view.

#### Scenario: Require the Current Lifecycle Schema

- **WHEN** repos status reads a database with pending lifecycle migrations
- **THEN** it reports a required schema upgrade without changing that database

#### Scenario: Inspect a Missing Source

- **WHEN** a stored source path is missing
- **THEN** repos status still lists its stored identity, path, and cached size
  without probing Git or the missing directory

#### Scenario: Show Source Repository Size

- **WHEN** a source repository has a stored complete size observation
- **THEN** its repos human row shows that amount in `SIZE` and JSON includes
  the complete observation

### Requirement: Resolve the Target Workspace Independently of View

An explicit `WORKSPACE_ID` SHALL select that stored workspace regardless of current directory,
management mode, claim, operation, health, removed state, or path existence.
Without an explicit selector, status SHALL select the nearest stored workspace whose canonical
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

- **WHEN** no explicit selector is supplied and no stored workspace contains current directory
- **THEN** no summary, no summary placeholder, no extra blank line, and no
  diagnostic is emitted; only the existing selected inventory is rendered

#### Scenario: Inspect a Missing Removed Target Path

- **WHEN** the supplied ID identifies a removed workspace whose directory is absent
- **THEN** status reports its persisted removed state successfully

Status SHALL also accept `--workspace-id`, `--workspace-dir`, or `--claim-id`.
All named selectors and the positional ID SHALL be mutually exclusive. A
workspace directory SHALL select only an exact root; a claim SHALL select its
associated workspace. An unmatched explicit selector SHALL fail with no standard
output, including when storage is absent. Path and claim selection SHALL have
appropriate human summary headings without changing the JSON structure.

#### Scenario: Select an Explicit Path or Claim

- **WHEN** status receives one valid path or claim selector
- **THEN** it reports that workspace regardless of the selected inventory view
- **AND** inventory filtering remains unchanged

### Requirement: Render a Compact Target Workspace Summary

Human output SHALL begin with `Workspace (current directory)` for an inferred
target or `Workspace (selected by ID)` for an explicit target. The summary SHALL
list ID, Path, Status, Mode, optional Operation, Repos, Disk usage, Processes,
and Reconciled in order. Disk usage SHALL be present whenever a target is
selected and SHALL follow the target disk usage rendering requirement.
Processes SHALL follow the process observation rendering requirement, including
its indented table when confirmed matches exist.
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

#### Scenario: Place Disk Usage in Every Target Summary

- **WHEN** a target is selected in any status view
- **THEN** its summary contains Disk usage after Repos and before Processes

### Requirement: Attribute Current User Processes to the Target Workspace

Status SHALL perform one live process observation on Linux and macOS when
a target is selected, regardless of the view. It SHALL include only processes whose
effective user ID equals the invoking process's effective user ID and whose
current physical working directory belongs to the target. Ownership SHALL use
absolute path components and the nearest registered workspace boundary from the
persisted snapshot, including removed boundaries. It SHALL NOT use command-line
substring matches, launch history, or parentage to infer workspace ownership.

The current status process and its explicitly identified observation helpers
SHALL be excluded; other shells and processes SHALL remain eligible. Threads
SHALL NOT appear as separate processes. Matches SHALL be sorted by numeric `PID`
and include `PID`, nullable process name, and absolute `cwd`. A missing name SHALL
NOT discard an otherwise confirmed match. Unresolved or deleted `cwd` values
SHALL NOT be guessed into a workspace. Status SHALL NOT require the stored
target path to exist or skip observation merely because its record is removed.

#### Scenario: Include Workspace and Repository Processes

- **WHEN** eligible processes have `cwd` equal to the target root and nested repository directories
- **THEN** all appear in its count and `PID`-sorted process list, including shells

#### Scenario: Respect the Nearest Registered Boundary

- **WHEN** `/work/api/nested` is registered inside `/work/api`, including as removed
- **THEN** a process in `/work/api/nested/repo` belongs only to the nested workspace

#### Scenario: Reject Similar Prefixes

- **WHEN** the target is `/work/api` and a process has `cwd` `/work/api-extra`
- **THEN** the process is excluded from the target observation

#### Scenario: Resolve Physical Working Directories

- **WHEN** a process entered the target through a symlink
- **THEN** attribution uses the physical `cwd` rather than the symlink spelling

#### Scenario: Exclude Observer and Other Users

- **WHEN** the enumerated processes include this status command, its helper, and a known other-user process
- **THEN** none contributes to the target count or list
- **AND** an unreadable `cwd` on the known other-user process does not degrade completeness

#### Scenario: Do Not Follow Departed Descendants

- **WHEN** a child of a target process changes `cwd` outside the target
- **THEN** it is not attributed to the target based on its parent

#### Scenario: Observe a Removed Target

- **WHEN** an explicit ID selects a removed workspace with no existing directory
- **THEN** its persisted summary remains available and a normal observation is attempted
- **AND** unresolved deleted `cwd` values are not interpreted as confirmed matches

### Requirement: Report Process Observation Completeness

Each attempted observation SHALL capture an RFC 3339 `observed_at` at the start of
collection. It SHALL report complete, partial, or unavailable. Observation SHALL be
best effort under `OS` visibility and SHALL NOT claim an atomic process snapshot
or safe workspace removal or reuse. It SHALL NOT require elevated privileges.

Complete SHALL mean enumeration succeeded and all enumerated eligible live
candidates were classified. Unknown candidate user identity, unreadable eligible
`cwd`, or an unresolved identity race SHALL yield partial with confirmed matches.
Partial counts SHALL be lower bounds, including when zero matches are confirmed.
Known exited candidates SHALL be skipped without making the observation partial.
A known `PID` reuse SHALL NOT combine metadata from different process identities;
an ambiguous identity race SHALL be omitted and reported as partial.

Unavailable SHALL mean enumeration, invoking-user identification, or platform
support prevents observation. It SHALL have no numeric count or process entries.
Nonfatal issues SHALL be aggregated into stable codes with nullable affected
candidate counts. Codes SHALL be `user_unreadable`, `cwd_unreadable`, `process_raced`,
`enumeration_failed`, `current_user_unavailable`, or `unsupported_platform`. Global
issues SHALL use null affected counts. Complete SHALL have no issues; partial
and unavailable SHALL have at least one. Issue order SHALL be lexicographic by
code. Issues SHALL NOT expose other-user process identities or command arguments.

#### Scenario: Confirm an Empty Result

- **WHEN** enumeration completes and all eligible candidates are classified outside the target
- **THEN** the observation is complete with count zero and no processes or issues

#### Scenario: Preserve Confirmed Matches Under Limited Visibility

- **WHEN** two target processes are confirmed but an eligible candidate's `cwd` cannot be read
- **THEN** the observation is partial with count two and both confirmed processes
- **AND** it contains a `cwd_unreadable` issue counting the unreadable candidate

#### Scenario: Avoid a False Exact Zero

- **WHEN** no target matches are confirmed and an eligible candidate cannot be classified
- **THEN** the observation is partial with count zero, not complete

#### Scenario: Handle Exit During Collection

- **WHEN** a candidate is confirmed to have exited before its metadata is read
- **THEN** the candidate is skipped without failing status or adding a partial issue

#### Scenario: Handle Total Failure

- **WHEN** the `OS` process list cannot be obtained
- **THEN** the observation is unavailable with `enumeration_failed` and a null count
- **AND** the existing persisted status is still reported successfully

#### Scenario: Handle an Unsupported Platform

- **WHEN** a target is selected on a platform without process observation support
- **THEN** the observation is unavailable with `unsupported_platform` and status succeeds

### Requirement: Render Target Process Counts and Lists

The target summary SHALL render Processes between Repos and Reconciled. A
complete observation SHALL show its numeric count. A partial observation SHALL
show COUNT (partial: REASONS). An unavailable observation SHALL show unavailable
(REASONS). Reasons SHALL be deterministic English descriptions of issue codes,
deduplicated and separated by semicolons in code order. `cwd_unreadable` SHALL
render as some process working directories could not be read; `enumeration_failed`
SHALL render as process enumeration failed.

When confirmed matches exist, an indented `PID` / NAME / `CWD` table SHALL follow
immediately. It SHALL show all confirmed matches without truncation, sorted by
`PID`, with `cwd` relative to the target root and a dot for the root itself. A null
name SHALL render as unknown. Empty lists SHALL omit the table and its headers.
Process names, paths, and reasons SHALL escape terminal control characters;
alignment SHALL account for display width. The new table SHALL use plain text
without generated ANSI color, including under NO_COLOR and piped output.

#### Scenario: Render Three Processes

- **WHEN** `PID` 1201 runs `zsh` at the root, `PID` 1248 runs cargo in `api`, and `PID` 1302 runs node in web
- **THEN** Processes shows 3 followed by `PID` / NAME / `CWD` rows for 1201 / `zsh` / ., 1248 / cargo / `api`, and 1302 / node / web
- **AND** Reconciled follows the last process row

#### Scenario: Omit the Empty Process Table

- **WHEN** a complete observation has no matches
- **THEN** Processes shows 0 and Reconciled follows without process table headers

#### Scenario: Render Partial Observation

- **WHEN** two matches are confirmed and an eligible `cwd` cannot be read
- **THEN** Processes shows 2 (partial: some process working directories could not be read)
- **AND** both confirmed rows follow

#### Scenario: Render Unavailable Observation

- **WHEN** process enumeration fails
- **THEN** Processes shows unavailable (process enumeration failed) without a table

#### Scenario: Escape Untrusted Process Text

- **WHEN** a process name or `cwd` contains a newline, tab, or ANSI escape byte
- **THEN** that row remains one physical line without injecting terminal controls

### Requirement: Serialize Target Process Observations Independently

All version-2 JSON views SHALL add top-level target_processes. It SHALL be null
exactly when target_workspace is null. Otherwise, it SHALL contain observed_at,
status, count, processes, and issues. Existing workspace object fields, inventory
arrays, snapshot_at meaning, and schema_version SHALL remain unchanged.

For complete or partial, count SHALL equal processes length. For unavailable,
count SHALL be null and processes SHALL be empty. Each process SHALL contain
integer `pid`, nullable string name, and absolute string `cwd`. Each issue SHALL
contain code and nullable integer affected_count. Paths and names SHALL use
original text without human escaping or relative-path conversion; non-`UTF-8`
values SHALL use lossy Unicode conversion only for text representation.

#### Scenario: Emit an Independent Observation in Every View

- **WHEN** pools, workspaces, or repos JSON is requested with a target
- **THEN** the version-2 document includes target_processes with its own observed_at
- **AND** persisted target and inventory objects retain their existing contracts

#### Scenario: Emit Null Without a Target

- **WHEN** status JSON has no selected target
- **THEN** target_processes is null and no process enumeration occurs

#### Scenario: Serialize a Partial Result

- **WHEN** two processes match and one candidate `cwd` is unreadable
- **THEN** target_processes has status partial, count 2, two processes, and an issue with code `cwd_unreadable` and affected_count 1

#### Scenario: Serialize an Unavailable Result

- **WHEN** enumeration fails
- **THEN** target_processes has status unavailable, count null, processes [], and an `enumeration_failed` issue with affected_count null

### Requirement: Observe Workspace Disk Usage

Status SHALL report the latest stored disk usage observations for the selected
workspace, displayed workspace rows, their repository worktrees, and displayed
source repositories. It SHALL NOT measure filesystem usage or traverse a
registered path. A null cache SHALL render as `unknown`; complete, partial,
and unavailable cached observations SHALL retain their measured bytes, time,
and issues. A target that also appears in workspace inventory SHALL use the
same stored observation in both places. Cached sizes SHALL NOT determine
workspace lifecycle eligibility or imply current or reclaimable space.

#### Scenario: Count Allocated Space Without Following Links

- **WHEN** a lifecycle refresh measures sparse files, hard links, hidden entries,
  and a symbolic link outside a registered path
- **THEN** status later displays the stored allocated-block observation without
  running another filesystem scan

#### Scenario: Count Nested Physical Content

- **WHEN** a refreshed workspace contains a nested workspace or mounted directory
- **THEN** its stored observation includes the reachable physical entries and
  status reads that observation without modifying inventory membership

#### Scenario: Keep a Partial Observation

- **WHEN** a refresh stores a partial result after an unreadable descendant
- **THEN** status displays the stored amount and partial marker with its original
  observation time

#### Scenario: Keep a Missing Root Unavailable

- **WHEN** a refresh stores an unavailable result for a missing root
- **THEN** status displays that stored result without probing the root again

#### Scenario: Avoid Symlink Replacement Escape

- **WHEN** an entry changes into a symbolic link during refresh
- **THEN** the refresh does not traverse the link target and status reads its
  resulting cached observation

#### Scenario: Scan Inventory Without a Target

- **WHEN** `--view workspaces` displays rows without a selected target
- **THEN** status reads each row's stored size and performs no directory scan

#### Scenario: Reuse the Target Observation

- **WHEN** the selected target also appears in workspace inventory
- **THEN** its summary and row use the same stored size and observation time

#### Scenario: Skip Unrelated Inventories

- **WHEN** status uses pools or repos view without a target
- **THEN** it does not load unrelated workspace size observations or traverse
  workspace paths

### Requirement: Render Workspace Disk Usage

The target summary SHALL render `Disk usage` as integer bytes with `B` below
1024 bytes and otherwise as a binary unit (`KiB`, `MiB`, `GiB`, `TiB`, or higher)
with one decimal place. A partial observation SHALL append
`(partial: REASONS)` to the successfully inspected amount. An unavailable
observation SHALL show `unavailable (REASONS)`. Reasons SHALL be deterministic
English descriptions of issue codes, deduplicated in code order and separated
by semicolons. An unmeasured target SHALL show `unknown`. The workspace and
source repository tables' `SIZE` cells SHALL use the same unit
format, append `(partial)` without reasons for a partial observation, and show
`unavailable` for an unavailable observation or `unknown` when unmeasured.
Neither presentation SHALL emit
raw filesystem error text, raw paths, or generated ANSI color. Each inventory
workspace SHALL occupy one physical line.

#### Scenario: Render a Complete Binary Amount

- **WHEN** the observation contains 1536 allocated bytes and is complete
- **THEN** Disk usage renders as `1.5 KiB`

#### Scenario: Identify a Partial Amount

- **WHEN** a descendant is unreadable after some entries were inspected
- **THEN** Disk usage includes the observed amount and a partial reason

#### Scenario: Identify a Missing Root

- **WHEN** the target directory is missing
- **THEN** Disk usage renders as `unavailable (workspace directory missing)`

#### Scenario: Keep Workspace Rows Compact

- **WHEN** an inventory workspace has a partial 1536-byte observation or an
  unavailable observation
- **THEN** its `SIZE` cell shows `1.5 KiB (partial)` or `unavailable`,
  respectively, with issue details available in JSON

#### Scenario: Show an Unmeasured Size

- **WHEN** a workspace or source repository has no stored observation
- **THEN** its human size shows `unknown` and status does not measure it

### Requirement: Serialize Workspace Disk Usage Independently

Every version-2 JSON status view SHALL retain top-level `target_disk_usage`,
null exactly when `target_workspace` is null, and a `workspace_disk_usage`
array ordered with displayed workspace rows. Pools and repos views SHALL have
an empty workspace usage array. Each selected target and row SHALL include a
cached observation with nullable `observed_at`, `status`, nullable integer
`allocated_bytes`, and `issues`. A never-measured entity SHALL use status
`unknown`, null time and bytes, and empty issues. Measured complete, partial,
and unavailable observations SHALL preserve their original time, bytes, and
issues. Each array entry SHALL also include `workspace_id`.

Every repository worktree snapshot in a workspace JSON object SHALL include a
`disk_usage` observation with the same shape. Every source repository in the
repos JSON view SHALL also include `disk_usage`. Existing workspace and
repository identities, paths, lifecycle fields, and inventory order SHALL
remain unchanged. Process observations, session observations, and schema
version SHALL also remain unchanged. Cached
observation times SHALL remain independent of persisted `snapshot_at`.

#### Scenario: Include Usage in Every View

- **WHEN** a target is selected with `--json` in any view
- **THEN** `target_disk_usage` contains its stored observation or an unknown
  observation without scanning its path

#### Scenario: Include an Ordered Inventory Observation Array

- **WHEN** `--view workspaces --json` displays workspace rows
- **THEN** `workspace_disk_usage` has one matching ID and cached observation per
  row in inventory order, including removed rows selected by `--all`

#### Scenario: Reuse an Inventory Target in JSON

- **WHEN** the selected target also appears in workspace JSON inventory
- **THEN** its target and inventory usage have identical status, time, bytes,
  and issues

#### Scenario: Emit Null Disk Usage Without a Target

- **WHEN** no target is selected with `--json`
- **THEN** `target_disk_usage` is null while workspace rows retain their cached
  observations

#### Scenario: Keep Unrelated View Arrays Empty

- **WHEN** status uses pools or repos view with `--json`
- **THEN** `workspace_disk_usage` is empty regardless of target selection

#### Scenario: Serialize an Unavailable Disk Usage Result

- **WHEN** a refresh has stored an unavailable result for a missing target path
- **THEN** `target_disk_usage.status` is `unavailable`, bytes are null, and
  issues include `root_missing` with the saved observation time

#### Scenario: Serialize an Unknown Size

- **WHEN** a workspace, worktree, or source repository has never been measured
- **THEN** its JSON usage has status `unknown`, null observation time and bytes,
  and an empty issue list

### Requirement: Show Cached Repository Worktree Sizes

When a selected workspace has repository worktrees, its human summary SHALL
show an indented `REPO` and `SIZE` table immediately after the compact `Repos`
line. Each row SHALL use the existing escaped label and state suffix for that
worktree and render its cached size using the workspace size cell rules. An
unmeasured worktree SHALL show `unknown`. A workspace with no worktrees SHALL
omit the table. The workspace inventory table SHALL remain one physical line
per workspace.

#### Scenario: Show Different Worktree Sizes

- **WHEN** a selected workspace has two worktrees with different cached sizes
- **THEN** its repo detail table shows each label beside its own size while the
  workspace's `Disk usage` remains the separate workspace-root measurement

#### Scenario: Omit an Empty Worktree Table

- **WHEN** a selected workspace has no repository worktrees
- **THEN** its summary keeps the compact `Repos 0/0` line without a repo table
