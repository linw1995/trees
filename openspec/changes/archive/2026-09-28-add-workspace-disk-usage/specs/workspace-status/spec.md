# Spec Delta

## MODIFIED Requirements

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

### Requirement: Read Status Without Side Effects

Status SHALL capture one snapshot timestamp and load every persisted relationship and registered workspace boundary needed
by the selected view and target through relational joins in one read-only SQLite
transaction. It SHALL NOT expand all selected entity IDs into a single `IN`
expression. Status SHALL NOT open lifecycle storage for writing or append an
event. It SHALL NOT acquire or release a claim or start or recover an operation.
Trees itself SHALL NOT invoke Git or inspect workspace file contents. It SHALL
traverse only the selected target's and displayed workspace rows' directory
trees for disk usage observation, after the lifecycle connection closes. A
user-configured session query hook
SHALL be permitted after the lifecycle connection closes, as specified by
workspace-session-hook. The hook is user-controlled code, not a sandboxed or
guaranteed side-effect-free operation; Trees SHALL NOT persist its results or
use them for lifecycle decisions. It SHALL permit read-only `OS` process metadata
access and `cwd` path resolution solely for process attribution after the database
transaction. Without an explicit ID, it SHALL resolve the invocation directory
canonically solely for target selection. Target selection and persisted reporting
SHALL NOT require the stored target path to exist; a missing target directory
SHALL instead make disk usage unavailable.

Persisted unhealthy states, claims, leases, and removed rows in the explicit
workspace all-view or target summary SHALL be report data rather than command failures. Status
SHALL return nonzero only when arguments are invalid, an explicit ID is unknown, or it cannot
resolve the invocation directory, load persisted status, or serialize the report.
Session hook failures SHALL produce unavailable or partial observation data and
SHALL NOT change an otherwise successful exit status.
Process observation failures SHALL produce partial or unavailable report data
and SHALL NOT change an otherwise successful exit status. No process observer
SHALL run before successful persisted status loading or without a target.
Disk usage observation failures SHALL produce partial or unavailable report data
and SHALL NOT change an otherwise successful exit status. No disk usage observer
SHALL run before successful persisted status loading or for a workspace absent
from both the target and the displayed workspace inventory.
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

#### Scenario: Observe Sessions After Closing Storage

- **WHEN** an enabled workspace session hook is eligible to run
- **THEN** Trees closes its lifecycle database connection before starting the hook
- **AND** hook failure leaves persisted status and the successful exit status intact

#### Scenario: Preserve Removed Target Details Without Its Directory

- **WHEN** an explicit selector resolves a removed workspace whose directory is absent
- **THEN** status retains its persisted target and inventory data and reports disk
  usage as unavailable without failing the command

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

## ADDED Requirements

### Requirement: Observe Workspace Disk Usage

Status SHALL attempt one live disk usage observation for every displayed
workspace inventory row in the workspaces view and for the selected target in
every view, after closing lifecycle storage and regardless of persisted state.
If the target also appears in the workspace inventory, status SHALL reuse its
observation and SHALL NOT traverse that directory twice. Pools and repos views
without a target SHALL run no disk usage observation. On Linux and macOS,
allocated bytes SHALL sum filesystem allocated blocks multiplied by 512.
The sum SHALL include the root and every reachable
entry beneath it, including hidden entries, directory metadata, worktree
metadata, nested registered workspaces, and entries on mounted filesystems.
Each hard-linked inode SHALL be counted only once per observation. Symbolic
links SHALL contribute their own allocation but their targets SHALL NOT be
traversed. Directory traversal SHALL NOT escape the selected tree through a
symbolic link, including when entries change during observation.

Each observation SHALL capture RFC 3339 `observed_at` at its start and report
`complete`, `partial`, or `unavailable`. Complete SHALL mean every reachable
entry was inspected without a detected error. Partial SHALL contain the sum of
successfully inspected entries when a descendant cannot be inspected.
Unavailable SHALL have no byte count when the root is missing, unreadable, or
not a directory, the platform is unsupported, or the sum overflows. An
observation SHALL NOT claim an atomic filesystem snapshot or imply reclaimable
space. Disk usage SHALL NOT affect workspace lifecycle decisions.

Issues SHALL use stable codes `root_missing`, `root_unreadable`,
`entry_changed`, `entry_unreadable`, `unsupported_platform`, and
`size_overflow`, sorted by code. Each issue SHALL carry a nullable
`affected_count`: descendant issues SHALL count affected entries; root,
platform, and overflow issues SHALL use null. A complete observation SHALL
have no issues; partial and unavailable observations SHALL have at least one.

#### Scenario: Count Allocated Space Without Following Links

- **WHEN** a target contains sparse files, hard links, hidden entries, and a
  symbolic link to a directory outside the target
- **THEN** status uses allocated blocks rather than apparent file lengths,
  counts each hard-linked inode once, includes hidden entries and the link itself,
  and does not traverse the link target

#### Scenario: Count Nested Physical Content

- **WHEN** a target contains a nested registered workspace or a mounted directory
- **THEN** the reachable physical entries beneath the target contribute to its
  observation without changing the global workspace inventory

#### Scenario: Keep a Partial Observation

- **WHEN** an entry disappears or a descendant directory cannot be read during
  traversal
- **THEN** status reports partial with the sum of successfully inspected entries
  and a corresponding issue, while keeping a successful exit status

#### Scenario: Keep a Missing Root Unavailable

- **WHEN** a selected target's stored root no longer exists
- **THEN** status reports unavailable with a null byte count and `root_missing`
  without discarding the persisted target

#### Scenario: Avoid Symlink Replacement Escape

- **WHEN** a directory entry is replaced by a symbolic link during observation
- **THEN** status does not traverse the replacement's target and reports an
  incomplete observation if it cannot inspect the original entry

#### Scenario: Scan Inventory Without a Target

- **WHEN** current directory selects no workspace and `--view workspaces` has
  displayed workspace rows
- **THEN** status observes each displayed workspace once for its `SIZE` column

#### Scenario: Reuse the Target Observation

- **WHEN** the selected target also appears in the workspace inventory
- **THEN** status traverses its directory once and reuses that observation in
  both the target summary and inventory row

#### Scenario: Skip Unrelated Inventories

- **WHEN** current directory selects no workspace in pools or repos view
- **THEN** status does not traverse any workspace directory

### Requirement: Render Workspace Disk Usage

The target summary SHALL render `Disk usage` as integer bytes with `B` below
1024 bytes and otherwise as a binary unit (`KiB`, `MiB`, `GiB`, `TiB`, or higher)
with one decimal place. A partial observation SHALL append
`(partial: REASONS)` to the successfully inspected amount. An unavailable
observation SHALL show `unavailable (REASONS)`. Reasons SHALL be deterministic
English descriptions of issue codes, deduplicated in code order and separated
by semicolons. The workspace table's `SIZE` cell SHALL use the same unit
format, append `(partial)` without reasons for a partial observation, and show
`unavailable` for an unavailable observation. Neither presentation SHALL emit
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

### Requirement: Serialize Workspace Disk Usage Independently

Every version-2 JSON status view SHALL include top-level `target_disk_usage`,
null exactly when `target_workspace` is null. It SHALL also include a top-level
`workspace_disk_usage` array. In the workspaces view, the array SHALL have one
entry per displayed workspace in the same order as `workspaces`; in pools and
repos views, it SHALL be empty. Each array entry SHALL contain `workspace_id`
and the observation fields `observed_at`, `status`, nullable integer
`allocated_bytes`, and `issues`. A non-null `target_disk_usage` SHALL contain
the same observation fields without `workspace_id`. Each issue SHALL contain
`code` and nullable integer `affected_count`.
Complete and partial observations SHALL have an integer byte count, including
zero; unavailable observations SHALL have null. The observation time SHALL be
independent of persisted `snapshot_at`. Existing workspace objects, inventory
arrays, target process and session observations, and schema version SHALL keep
their current shapes and meanings. If the target appears in the workspace
inventory, its two JSON observations SHALL have identical fields and values.

#### Scenario: Include Usage in Every View

- **WHEN** a target is selected with `--json` in pools, workspaces, or repos view
- **THEN** the report has one structured `target_disk_usage` observation and
  unchanged persisted target and inventory objects

#### Scenario: Include an Ordered Inventory Observation Array

- **WHEN** `--view workspaces --json` displays workspace rows
- **THEN** `workspace_disk_usage` has one matching workspace ID and observation
  per row in inventory order, including removed rows selected by `--all`

#### Scenario: Reuse an Inventory Target in JSON

- **WHEN** the selected target also appears in a workspace JSON inventory
- **THEN** its `target_disk_usage` has the same observation time, status, byte
  count, and issues as its `workspace_disk_usage` entry

#### Scenario: Emit Null Without a Target

- **WHEN** no target is selected with `--json`
- **THEN** `target_disk_usage` is null, while `workspace_disk_usage` still
  contains observations for any displayed workspace rows

#### Scenario: Keep Unrelated View Arrays Empty

- **WHEN** status uses pools or repos view with `--json`
- **THEN** `workspace_disk_usage` is empty regardless of target selection

#### Scenario: Serialize an Unavailable Result

- **WHEN** a selected target directory is missing with `--json`
- **THEN** `target_disk_usage.status` is `unavailable`, `allocated_bytes` is null,
  and `issues` includes `root_missing`
