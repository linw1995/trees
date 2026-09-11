## Context

Automatic workspaces are slots in a UUID-backed pool defined by an exact set
of origin repositories. The primary status question is pool capacity: total
slots for each repository set, claimed slots, and slots available in the
current database snapshot. Individual workspace snapshots remain useful for
diagnosis but should be an explicit detail view rather than the default.

## Goals / Non-Goals

**Goals:**

- Make the default view one row per automatic repository-set pool.
- Report available, total, and abnormal current slot counts.
- Retain an explicit per-workspace detail view.
- Keep human output compact and provide versioned JSON for both views.
- Read one consistent SQLite snapshot without changing persisted or external
  state.

**Non-Goals:**

- Treat manual workspaces as allocatable pool capacity.
- Count removed tombstones as current capacity.
- Reconcile persisted state with Git or the filesystem.
- Assert live reusability without access-boundary checks.
- Show lifecycle history or completed operations.

## Decisions

### Select an Explicit View

The interface is `trees status [--view pools|workspaces] [--all] [--json]`.
`--view` defaults to `pools`. The pool view reports automatic allocation and
capacity. The workspace view retains individual manual and automatic records.
`--all` is valid only with `--view workspaces` and includes removed workspace
tombstones there.

The default is pool-oriented because automatic create allocates by exact
repository set rather than by a caller-selected workspace path. Making the
workspace view explicit prevents physical slot details from obscuring the
capacity question.

### Define Pool Capacity Counts

Each pool uses current non-removed automatic workspaces as its capacity.
`available` counts slots whose persisted workspace state is `ready`, with no
active claim and no retained operation lease. `abnormal` counts slots whose
persisted state is `degraded` or `failed`. A `creating` slot is transient rather
than abnormal. Claimed and operation-owned slots remain part of total capacity
but do not require a separate human count.

Availability is a persisted scheduling hint, not a live reusability promise.
Automatic allocation still performs reconciliation before returning a slot.

### Identify Pools by Repository Set

The pool view loads repositories from the explicit pool-to-origin relations.
Repositories within a pool are ordered by canonical source path. Each display
label starts with the source-path base name. Conflicting labels expand by one
parent component at a time until they are unique within the repository set.

Pools use lexicographical order based on their canonical source-path list, with
pool ID as a deterministic tiebreaker. Pools without current
non-removed automatic slots are omitted.

### Render Compact Pool and Detailed Workspace Tables

The human pool view contains these columns:

- `REPOS`: comma-separated shortest unique repository labels.
- `CAPACITY`: persisted available, total, and abnormal slots as
  `<available>/<total>/<abnormal>`.
- `UPDATED`: the latest workspace `updated_at` in the pool, rendered compactly.

On an interactive terminal, the three capacity numbers are green, blue, and
red respectively. When standard output is not a terminal or `NO_COLOR` is set,
the same ordered value is emitted without ANSI escapes. The order defines the
meaning independently from color.

The human workspace view contains these columns:

- `STATUS`: persisted workspace health followed by `🔒` when claimed.
- `MODE`: `🤖` for automatic or `👤` for manual.
- `REPOS`: attached repo worktrees over total repo-worktree count followed by
  shortest unique repository labels.
- `RECONCILED`: compact persisted reconciliation time or `never`.
- `ID`: stable workspace UUID.

Compact timestamps use UTC relative to `snapshot_at`: `HH:MM` on the same
date, `MM-DD HH:MM` within the same year, and `YYYY-MM-DD HH:MM` otherwise.
The renderer accounts for terminal display width when aligning emoji. Table
spacing is not a machine-readable compatibility contract.

For workspace `REPOS`, the attached count is presented as the ready count. On
an interactive terminal, ready and total are green and blue. Non-terminal
output and `NO_COLOR` use the same `<ready>/<total>` value without ANSI escapes.

Each repository label uses its persisted repo-worktree state. Ready labels are
green without a suffix. Pending labels are yellow with `(pending)`. Dirty,
missing, diverged, and failed labels are red with `(dirty)`, `(missing)`,
`(mismatch)`, and `(error)`. Removed labels are gray with `(removed)`.
Non-terminal output retains these suffixes without color, so color is never the
only indication of a problem.

`STATUS` renders the state alone for an unclaimed workspace and appends `🔒`
for a claimed workspace, such as `ready 🔒`. Absence of the lock means no active
claim. The JSON workspace projection remains normalized with separate state and
claim fields.

### Emit View-Specific Versioned JSON

`--json` emits exactly one JSON document. Both envelopes contain
`schema_version`, `view`, and `snapshot_at`.

The pool envelope contains `pools`. Each pool object contains `pool_id`, an
ordered `repositories` array, `available`, `capacity`, `abnormal`, and nullable
`updated_at`. Each repository contains `origin_repository_id`, `source_path`,
and its computed `label`.

The workspace envelope contains `workspaces` and retains the complete existing
workspace, claim, current operation, and repo-worktree projection. Canonical
paths and operation details therefore remain available without widening the
default human view.

### Open a Workspace Using a Stable Identifier

`trees open <workspace-id> [--program=<PROGRAM>]` resolves one persisted
workspace by UUID. Without `--program`, it selects a nonempty `$SHELL`; an
explicit empty program is invalid. Program execution reuses the create-open
handoff: inherited environment and standard streams, canonical workspace path
as the current directory, and process replacement where supported.

The lookup uses a read-only connection. Removed workspaces and workspaces
with a retained operation lease are rejected. An automatic workspace must
already have an active claim, so open cannot bypass pool allocation. Manual
workspaces do not require a claim. The database connection is dropped before
process handoff, and open does not reconcile or mutate lifecycle state.

### Read Without Side Effects

Each selected view captures one `snapshot_at` and loads all required pool,
workspace, claim, lease, event, relation, and origin rows with relational joins
inside one read-only SQLite transaction. Queries must not expand a global list
of identifiers into one `IN` expression. Status does not reconcile, recover,
acquire, release, run Git, inspect the filesystem, or append lifecycle events.

Human repository labels escape control characters, separators used by the
renderer, and ANSI escape bytes before status suffixes and generated colors are
added. This preserves one physical line per record and prevents path text from
injecting terminal control sequences. JSON retains the original path strings.

## Risks / Trade-Offs

- [Persisted availability can be stale after external Git changes] → Keep the
  access-boundary reconciliation requirement and document the metric as a
  persisted scheduling hint.
- [Pool aggregation can hide an unhealthy individual slot] → Preserve the
  explicit workspace detail view.
- [Two JSON views require consumers to branch] → Include an explicit `view`
  discriminator and keep one schema version while the command is unreleased.
- [Repository labels can collide] → Expand only conflicting labels to their
  shortest unique source-path suffix.

## Migration Plan

No data migration is required. The command has not been released, so its JSON
version 1 contract can adopt the view discriminator without a compatibility
transition. Rolling back removes the query and rendering code without changing
lifecycle records.

## Open Questions

None.
