## MODIFIED Requirements

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
It SHALL NOT invoke Git or inspect workspace file contents or traverse workspace
directory trees. It SHALL permit read-only `OS` process metadata access and `cwd`
path resolution solely for process attribution after the database transaction. Without an
explicit ID, it SHALL resolve the invocation directory canonically solely for
target selection. It SHALL NOT probe a stored target path or require it to exist as a condition of selecting or reporting the target.

Persisted unhealthy states, claims, leases, and removed rows in the explicit
workspace all-view or target summary SHALL be report data rather than command failures. Status
SHALL return nonzero only when arguments are invalid, an explicit ID is unknown, or it cannot
resolve the invocation directory, load persisted status, or serialize the report.
Process observation failures SHALL produce partial or unavailable report data
and SHALL NOT change an otherwise successful exit status. No process observer
SHALL run before successful persisted status loading or without a target.
Registered boundaries used for attribution SHALL share the persisted snapshot.

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

### Requirement: Render a Compact Target Workspace Summary

Human output SHALL begin with `Workspace (current directory)` for an inferred
target or `Workspace (selected by ID)` for an explicit target. The summary SHALL
list ID, Path, Status, Mode, optional Operation, Repos, Processes, and Reconciled in order.
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

## ADDED Requirements

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
