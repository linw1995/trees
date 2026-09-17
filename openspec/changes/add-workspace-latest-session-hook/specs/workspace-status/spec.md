## MODIFIED Requirements

### Requirement: Render Pool and Workspace Human Views

The pool human view SHALL render `REPOS`, `CAPACITY`, and `UPDATED`. Capacity
SHALL use `<available>/<total>/<abnormal>`. On an interactive terminal, the
available, total, and abnormal numbers SHALL be green, blue, and red. With
non-terminal standard output or `NO_COLOR`, status SHALL emit the same value
without ANSI escapes. The order SHALL define the meaning independently from
color. `UPDATED` SHALL be the greatest `updated_at` among current capacity
slots.

The workspace human view SHALL render `STATUS`, `MODE`, `REPOS`, `RECONCILED`,
and `ID`, followed by `LATEST SESSION` when the session hook is enabled for a
nonempty inventory, as specified by workspace-session-hook. `STATUS` SHALL render persisted workspace health and append `🔒`
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
remain unchanged; the enabled session hook SHALL only append its column. With a summary, one blank line and the heading `Pools`,
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

### Requirement: Read Status Without Side Effects

Status SHALL capture one snapshot timestamp and load every persisted relationship and registered workspace boundary needed
by the selected view and target through relational joins in one read-only SQLite
transaction. It SHALL NOT expand all selected entity IDs into a single `IN`
expression. Status SHALL NOT open lifecycle storage for writing or append an
event. It SHALL NOT acquire or release a claim or start or recover an operation.
Trees itself SHALL NOT invoke Git or inspect workspace file contents or traverse workspace
directory trees. A user-configured session query hook SHALL be permitted after the
lifecycle connection closes, as specified by workspace-session-hook. The hook
is user-controlled code, not a sandboxed or guaranteed side-effect-free operation;
Trees SHALL NOT persist its results or use them for lifecycle decisions. It SHALL permit read-only `OS` process metadata access and `cwd`
path resolution solely for process attribution after the database transaction. Without an
explicit ID, it SHALL resolve the invocation directory canonically solely for
target selection. It SHALL NOT probe a stored target path or require it to exist as a condition of selecting or reporting the target.

Persisted unhealthy states, claims, leases, and removed rows in the explicit
workspace all-view or target summary SHALL be report data rather than command failures. Status
SHALL return nonzero only when arguments are invalid, an explicit ID is unknown, or it cannot
resolve the invocation directory, load persisted status, or serialize the report.
Session hook failures SHALL produce unavailable or partial observation data and
SHALL NOT change an otherwise successful exit status.
Process observation failures SHALL produce partial or unavailable report data
and SHALL NOT change an otherwise successful exit status. No process observer
SHALL run before successful persisted status loading or without a target.
Registered boundaries used for attribution SHALL share the persisted snapshot.

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

#### Scenario: Observe Sessions After Closing Storage

- **WHEN** an enabled workspace session hook is eligible to run
- **THEN** Trees closes its lifecycle database connection before starting the hook
- **AND** hook failure leaves persisted status and the successful exit status intact
