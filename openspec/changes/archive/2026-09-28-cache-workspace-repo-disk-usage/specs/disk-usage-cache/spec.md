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

### Requirement: Measure Affected Entity Paths

When a successful lifecycle command requests a size update, Trees SHALL
measure the affected workspace root, its active repository worktrees, and
referenced origin repositories. Shared origins SHALL be measured once per
operation. Trees SHALL read entity IDs and paths, end the selection transaction,
measure without holding a database write transaction, and persist each result
only if that entity still has the same stored path. A changed path SHALL be
skipped. An unreadable or missing entity SHALL produce a partial or unavailable
observation without preventing other selected entities from being measured.
Trees SHALL NOT expose a separate size refresh command.

#### Scenario: Measure a Workspace and Its Repositories

- **WHEN** a lifecycle operation changes a workspace with two worktrees that
  reference one origin
- **THEN** Trees measures the workspace, both worktrees, and the shared origin
  once each

#### Scenario: Skip a Changed Path

- **WHEN** an entity path changes after selection but before persistence
- **THEN** Trees does not store the observation under the new path and records
  that entity as skipped

#### Scenario: Continue After a Missing Path

- **WHEN** one referenced source path is missing during measurement
- **THEN** Trees stores its unavailable observation and continues measuring
  the other selected paths

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
