# Workspace Status Delta

## ADDED Requirements

### Requirement: Observe Container Workspace Content

Status SHALL observe workspace-root content for the selected target when its
root is a container. Observation SHALL run after the read-only snapshot
transaction closes. It SHALL collect entries that are neither expected
repository worktree paths nor managed `AGENTS.md` instructions. It SHALL sort
relative paths, retain the full count, and limit the reported list. It SHALL NOT
change persisted state.

#### Scenario: Report Unexpected Entries

- **WHEN** a container workspace root contains an unexpected file and directory
- **THEN** the workspace content state is `dirty`
- **AND** the entries are ordered relative paths with their kinds

#### Scenario: Exclude Managed Instructions

- **WHEN** the workspace root contains the managed `AGENTS.md` instructions
- **THEN** it is not reported as an unexpected entry

#### Scenario: Preserve State During Content Observation

- **WHEN** status reads the selected workspace root
- **THEN** database contents, workspace files, and repository worktrees remain
  unchanged

### Requirement: Classify Workspace Content

A workspace root that does not exist SHALL be `missing`. A removed workspace
SHALL be `removed`. A root that cannot be read SHALL be `unknown` with an issue.
An empty unexpected-entry set SHALL be `clean`; a nonempty set SHALL be `dirty`.

#### Scenario: Report a Missing Root

- **WHEN** the selected workspace path does not exist
- **THEN** its workspace content state is `missing`

#### Scenario: Report an Unreadable Root

- **WHEN** the workspace root cannot be read
- **THEN** its workspace content state is `unknown`
- **AND** the report contains a `root_unreadable` issue

#### Scenario: Report a Clean Root

- **WHEN** the workspace root contains only expected worktrees and managed
  instructions
- **THEN** its workspace content state is `clean`
- **AND** its changed entry count is zero

### Requirement: Skip Single Worktree Roots

A single-repository workspace whose root is the worktree SHALL NOT receive a
workspace content observation or `Content` line.

#### Scenario: Skip a Single Worktree Root

- **WHEN** the selected workspace has one repository worktree at the workspace
  root
- **THEN** status does not read workspace content
- **AND** the summary omits `Content`

### Requirement: Bound Workspace Content Entries

The reported entry list SHALL be ordered and limited to 20. `changed_entries`
SHALL retain the full count. `truncated` SHALL be true when entries are omitted.

#### Scenario: Bound the Entry List

- **WHEN** the workspace root contains more than 20 unexpected entries
- **THEN** `entries` contains 20 entries
- **AND** `changed_entries` retains the full count
- **AND** `truncated` is true

### Requirement: Show Workspace Content Entries

When the target is a container workspace with unexpected entries, its human
summary SHALL show a `WORKSPACE PATH` and `KIND` table immediately after the
`Content` line. Paths SHALL be relative to the workspace root and escaped.
Entries SHALL be ordered and limited to 20. A truncated list SHALL end with a
`…` row. Expected worktree paths and managed `AGENTS.md` SHALL NOT appear.

#### Scenario: Show Unexpected Entries

- **WHEN** the workspace root contains an unexpected file and directory
- **THEN** the table lists both paths and kinds

#### Scenario: Mark a Truncated List

- **WHEN** more than 20 unexpected entries exist
- **THEN** the table ends with a `…` row

### Requirement: Serialize Target Workspace Observation

Every version-2 report SHALL contain nullable top-level
`target_workspace_observation`. It SHALL be null without a target or for a
single-worktree root. Otherwise, it SHALL contain `observed_at`, `state`,
`changed_entries`, `entries`, `truncated`, and `issues`.

#### Scenario: Serialize a Dirty Container

- **WHEN** the target is a container root with unexpected entries
- **THEN** `state` is `dirty` and `entries` lists them

#### Scenario: Serialize a Null Observation

- **WHEN** no target is selected or the root is a single worktree
- **THEN** `target_workspace_observation` is null

### Requirement: Serialize Workspace Content Entries

Each entry SHALL contain a relative `path` and `kind` (`file`, `directory`,
`symlink`, or `other`). Entries SHALL be ordered and limited to 20.

#### Scenario: Serialize Entry Kinds

- **WHEN** the workspace root has file and directory entries
- **THEN** their JSON kinds are `file` and `directory`

### Requirement: Serialize Workspace Content Issues

Each issue SHALL contain `code` and nullable bounded `message`. Codes SHALL be
`root_unreadable` and `instructions_unreadable`. The observation time SHALL be
independent of `snapshot_at`. Failure SHALL NOT change the exit status.

#### Scenario: Serialize an Unreadable Root

- **WHEN** the workspace root cannot be read
- **THEN** `issues` contains a `root_unreadable` object with a bounded message

## MODIFIED Requirements

### Requirement: Render a Compact Target Workspace Summary

Human output SHALL begin with `Workspace (current directory)` for an inferred
target or `Workspace (selected by ID)` for an explicit target. The summary SHALL
list ID, Path, Status, Mode, optional Operation, optional Content, Repos, Disk
usage, Processes, and Reconciled in order. Disk usage SHALL be present whenever
a target is selected and SHALL follow the target disk usage rendering
requirement. Processes SHALL follow the process observation rendering
requirement, including its indented table when confirmed matches exist. Status
SHALL use persisted health with `🔒` appended only for an active claim. There
SHALL be no separate Claim row. Mode SHALL use `automatic 🤖` or `manual 👤`.
Repos SHALL reuse inventory readiness counts, labels, state suffixes, and color
rules. Removed health SHALL render as `removed`.

Content SHALL be present for a container workspace and omitted for a
single-repository workspace whose root is the worktree. It SHALL summarize the
workspace root as `clean`, `dirty` with the unexpected entry count, `missing`,
`removed`, or `unknown`. It SHALL be green for clean, red for dirty or missing,
and gray for removed or unknown.

Operation SHALL be omitted when absent; otherwise it SHALL show operation kind,
state, and lease classification as
`KIND / STATE (lease active|expired|inconsistent)`. Reconciled SHALL use `never`
or local `YYYY-MM-DD HH:MM:SS` without timezone text; if platform local timezone
information is unavailable, it SHALL use UTC without a suffix. All persisted
human text SHALL escape terminal control characters before generated color is
applied. `NO_COLOR` and non-terminal output SHALL contain no generated ANSI
escapes. Emoji SHALL follow their associated text.

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

#### Scenario: Show Container Workspace Content

- **WHEN** a container workspace root has two unexpected entries
- **THEN** its summary contains `Content` with `dirty (2 entries)`

#### Scenario: Omit Content for a Single Worktree Root

- **WHEN** the target root is a single repository worktree
- **THEN** its summary omits `Content`

### Requirement: Read Status Without Side Effects

Status SHALL capture one snapshot timestamp and load every persisted
relationship and registered workspace boundary needed by the selected view and
target through relational joins in one read-only SQLite transaction. It SHALL
NOT expand all selected entity IDs into a single `IN` expression. Status SHALL
NOT open lifecycle storage for writing or append an event. It SHALL NOT acquire
or release a claim or start or recover an operation. Trees itself SHALL NOT
invoke Git, inspect workspace file contents, or traverse workspace directory
trees for size, except that it MAY read the selected container workspace's
direct directory entries as specified by Observe Container Workspace Content.
It SHALL read cached disk usage from the same persisted snapshot as workspace
and repository metadata. A user-configured session query hook SHALL be permitted
after the lifecycle connection closes, as specified by workspace-session-hook.
The hook is user-controlled code, not a sandboxed or guaranteed side-effect-free
operation; Trees SHALL NOT persist its results or use them for lifecycle
decisions. It SHALL permit read-only `OS` process metadata access and `cwd` path
resolution solely for process attribution after the database transaction.
Without an explicit ID, it SHALL resolve the invocation directory canonically
solely for target selection. Target selection and persisted reporting SHALL NOT
require the stored target path to exist. A missing path SHALL NOT trigger a size
scan or change an already stored observation during status.

Persisted unhealthy states, claims, leases, and removed rows in the explicit
workspace all-view or target summary SHALL be report data rather than command
failures. Status SHALL return nonzero only when arguments are invalid, an
explicit ID is unknown, or it cannot resolve the invocation directory, load
persisted status, or serialize the report. Session hook failures SHALL produce
unavailable or partial observation data and SHALL NOT change an otherwise
successful exit status. Workspace content observation failures SHALL produce
unknown observation data and SHALL NOT change an otherwise successful exit
status. Process observation failures SHALL produce partial or unavailable report
data and SHALL NOT change an otherwise successful exit status. No workspace
content or process observer SHALL run before successful persisted status loading.
No workspace content or process observer SHALL run without a target. Unknown or
failed cached disk usage SHALL be report data, not command failures. Malformed
stored cache data SHALL be a persisted status loading error rather than a reason
to scan the filesystem. Registered boundaries used for process attribution SHALL
share the persisted snapshot.

#### Scenario: Preserve State During Inspection

- **WHEN** a status view reports persisted lifecycle data without an external hook
- **THEN** database contents, Git metadata, and workspace filesystem contents
  remain unchanged

#### Scenario: Observe Outside the Persisted Transaction

- **WHEN** status resolves a target and loads its persisted snapshot
- **THEN** it ends the read-only database transaction before content and process
  observation
- **AND** observation does not alter claims, health, capacity, or lifecycle state

#### Scenario: Preserve Status When Observation Fails

- **WHEN** workspace content or process observation fails after a valid
  persisted snapshot has loaded
- **THEN** status succeeds with an unavailable or partial observation and intact
  inventory

#### Scenario: Skip Unneeded Observation

- **WHEN** no target exists or argument, target, or database loading fails
- **THEN** status does not read workspace content or enumerate processes

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

#### Scenario: Skip Unneeded Disk Usage Observation

- **WHEN** no target or workspace inventory row needs disk usage, or argument,
  target, or database loading fails
- **THEN** status does not traverse workspace directories for disk usage

- **AND** a successful status invocation also does not traverse workspace
  directories for disk usage when size cells are present

#### Scenario: Observe Sessions After Closing Storage

- **WHEN** an enabled workspace session hook is eligible to run
- **THEN** Trees closes its lifecycle database connection before starting the hook
- **AND** hook failure leaves persisted status and the successful exit status intact

#### Scenario: Preserve Removed Target Details Without Its Directory

- **WHEN** an explicit selector resolves a removed workspace whose directory is absent
- **THEN** status retains its persisted target and inventory data and reports
  its cached disk usage or `unknown` without probing the missing directory
