# Design

## Context

See `proposal.md` for motivation. The current report scans every displayed
workspace after its read-only database transaction. Workspaces, repository
worktrees, and registered source repositories have distinct stored paths.
One repository worktree can occupy the workspace root. Each size is a separate
observation, and worktree sizes cannot be added to derive the workspace size.

## Goals / Non-Goals

**Goals:** Make status latency independent of directory entry counts, keep
measurement age visible, refresh all three entity types on demand and after
physical lifecycle changes, and preserve nonfatal scan failures.

**Non-Goals:** Detecting arbitrary edits made outside Trees, background
scheduling, quota accounting, or deriving reclaimable space from stored sizes.

## Decisions

### Store One Observation Column Per Entity

Add nullable `disk_usage_json` columns to `workspaces`, `repo_worktrees`, and
`origin_repositories`. A non-null value contains the measured observation:
`observed_at`, completeness, allocated bytes, and stable issues. Null means
never measured or explicitly invalidated. This keeps the current observation
model intact and avoids a set of nullable columns whose combinations can
contradict one another. Status decodes only the stored observations it needs
inside its existing read-only transaction. A malformed stored observation is a
storage error, not a reason to traverse the filesystem.

The human size cell shows `unknown` for null, the existing complete or partial
format for measured results, and `unavailable` for a failed refresh. JSON
keeps the observation shape for unknown values with `status: "unknown"`, null
time and bytes, and empty issues. The measurement timestamp remains distinct
from the status snapshot time. It is not a freshness guarantee: ordinary file
edits do not pass through Trees.

### Refresh Outside Database Transactions

An explicit `trees size refresh` command selects the current workspace by
default, `--workspace-id` selects another workspace, `--origin-id` selects a
source repository, and `--all` refreshes all registered paths. A workspace
refresh includes its root, its repository worktrees, and their referenced
origins. `--all` deduplicates shared origin repositories and scans each stored
entity once. It scans paths without holding a database transaction, then
writes observations in a short transaction only if each entity still has the
same stored path. A path changed during scanning is skipped rather than
receiving a result for the old path.

Successful create, add, and release operations refresh affected workspace and
repository observations after their physical work completes. Successful
removal invalidates the removed workspace and worktree observations. A cache
scan or write failure does not reverse a completed lifecycle operation; the
CLI reports a warning and leaves the previous observation or unknown state.
Claim and open do not modify workspace files and do not refresh sizes.

### Read Cached Values in Status

Remove directory scans from `status::report::load`. Load cached observations
for the selected target, displayed workspace rows, their worktrees, and source
repository rows in the same SQLite snapshot as other persisted fields. Keep
the existing top-level target and workspace usage fields, including exact
reuse when a target is also an inventory row. Add cached size to repository
worktree JSON and a `SIZE` column to the source repository human view.
Display individual repository worktree sizes in the selected workspace's repo
details, while retaining the compact workspace inventory row.

Do not silently refresh on a cache miss. A read-only status invocation must
remain independent of tree size, including `--json` and `--all`.

## Risks / Trade-Offs

- Cached values can be stale immediately after external edits. Always expose
  `observed_at`, document explicit refresh, and label unmeasured values
  `unknown`.
- Lifecycle refresh adds latency to write commands. Run it after physical work
  and outside long database transactions, and keep failures nonfatal.
- JSON stored in SQLite needs validation. Serialize from one typed model and
  reject malformed persisted values on read.
- Existing databases start with unknown sizes. The migration does not scan
  filesystem paths; users can refresh all values explicitly.

## Migration Plan

Apply the additive migration through writable database access. Existing
observations start null. Reverting this migration drops the new columns and restores live-scan
behavior only if the previous binary is also restored.
