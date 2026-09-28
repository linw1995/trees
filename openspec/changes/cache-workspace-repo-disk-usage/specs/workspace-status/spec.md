# Spec Delta

## MODIFIED Requirements

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

- **WHEN** an explicit refresh measures sparse files, hard links, hidden entries,
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

## ADDED Requirements

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
