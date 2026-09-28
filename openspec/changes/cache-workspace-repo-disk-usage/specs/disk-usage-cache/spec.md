# Disk Usage Cache Specification

## Purpose

This capability stores and refreshes measured disk usage for registered
workspace roots, repository worktrees, and source repositories, allowing
read-only status to report sizes without traversing directory trees.

## ADDED Requirements

### Requirement: Store Entity Disk Usage Observations

The lifecycle database SHALL store a nullable disk usage observation column
for each workspace, repository worktree, and origin repository. A null value
SHALL mean that no valid measurement is available. A non-null value SHALL
contain the allocated byte count or its unavailable state, RFC 3339 observation
time, completeness, and structured issue codes. A migration SHALL add the
columns without scanning paths or manufacturing measurements for existing
records. Cached values SHALL be treated as historical measurements, not current
filesystem facts.

#### Scenario: Upgrade an Existing Database

- **WHEN** a populated lifecycle database receives the disk usage migration
- **THEN** all existing workspaces, worktrees, and origins have unknown sizes
- **AND** no registered path is scanned during migration

#### Scenario: Preserve a Failed Measurement

- **WHEN** a refresh cannot inspect a registered path
- **THEN** its stored observation records the failed measurement and time
  without replacing it with a fabricated zero

### Requirement: Refresh Registered Disk Usage Explicitly

The CLI SHALL provide `trees size refresh` for the workspace containing the
current directory, `--workspace-id WORKSPACE_ID` for a selected workspace,
`--origin-id ORIGIN_ID` for a source repository, and `--all` for all registered
entities. These selectors SHALL be mutually exclusive. A workspace refresh
SHALL include its root, its repository worktrees, and their referenced origin
repositories. An all refresh SHALL include every current workspace, worktree,
and origin, deduplicating shared entities. A missing explicit target SHALL
fail before scanning. Each entity SHALL be scanned at most once per request.

The command SHALL read entity IDs and paths, close the read transaction,
measure without holding a database write transaction, and persist results in
short writes only if the stored entity path still matches the measured path.
The command SHALL report complete, partial, unavailable, and skipped counts.
An individual unreadable or missing path SHALL not prevent other entities from
being refreshed. A database write failure SHALL return a nonzero exit status
without claiming that all results were saved.

#### Scenario: Refresh the Current Workspace

- **WHEN** refresh runs inside a registered workspace without a selector
- **THEN** it measures that workspace, its worktrees, and their origins

#### Scenario: Refresh an Origin

- **WHEN** refresh receives `--origin-id` for a stored source repository
- **THEN** it measures only that origin's current source path

#### Scenario: Refresh All Entities Once

- **WHEN** multiple workspaces reference the same origin and `--all` is used
- **THEN** each workspace and worktree is measured once and the shared origin
  is measured once

#### Scenario: Skip a Changed Path

- **WHEN** an entity's path changes after selection but before persistence
- **THEN** refresh does not store the observation under its new path and counts
  that entity as skipped

### Requirement: Refresh After Physical Lifecycle Changes

Successful Trees CLI workspace creation and repository addition SHALL refresh
the affected workspace, its worktrees, and their origins after physical work
completes. Successful CLI workspace release SHALL refresh the affected workspace
and worktrees. Successful workspace removal SHALL invalidate the removed
workspace and worktree observations. Claiming or opening a workspace SHALL NOT
refresh sizes. Refresh failure after a completed lifecycle operation SHALL
leave that operation successful and emit a warning; it SHALL NOT roll back the
operation or expose the previous measurement as newly measured.

#### Scenario: Refresh After Adding a Repository

- **WHEN** a repository is added to a workspace successfully
- **THEN** subsequent status reads updated cached sizes for that workspace,
  its worktrees, and the referenced origin

#### Scenario: Keep a Completed Operation on Refresh Failure

- **WHEN** a workspace creation succeeds but its size refresh fails
- **THEN** creation remains successful and the CLI reports the cache failure

#### Scenario: Avoid Stale Removed Sizes

- **WHEN** workspace removal succeeds
- **THEN** its stored workspace and worktree sizes become unknown or a newly
  measured unavailable result rather than retaining pre-removal values
