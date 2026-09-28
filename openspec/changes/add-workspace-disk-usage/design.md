# Design

## Context

See `proposal.md` for motivation. `status::report::load` obtains the target and
inventory in a read-only SQLite transaction, closes the connection, then runs
live session and process observations. `WorkspaceStatus` is shared by target and
inventory JSON. The current status contract forbids workspace tree traversal,
and a removed target may have no directory.

## Goals / Non-Goals

**Goals:** Show disk allocation in both the selected workspace summary and the
workspace inventory table without changing persisted state, preserve useful
status output when filesystem observation fails, and keep the version-2
workspace object shape stable.

**Non-Goals:** Pool-wide aggregates and exclusive or reclaimable space. Also
outside scope are filesystem quotas, free-space reports, content hashing, Git
checks, cached sizes, and new status selectors.

## Decisions

### Measure Allocated Bytes Under the Target Root

On Linux and macOS, sum the allocated blocks reported by filesystem metadata
for the root and each reachable entry (`st_blocks * 512`). Include hidden
entries, worktree metadata, nested directories, and mount points. Count a
hard-linked inode once per observation using device and inode identity.
Count a symbolic link's own allocation without following its target. This
matches the user's disk-space question more closely than summing apparent file
lengths. It is not an estimate of space freed by deleting the workspace:
snapshots, clones, compression, and files linked outside the tree can share
storage. Two nested registered workspaces can produce overlapping totals.

Use iterative, directory-relative traversal with no-follow directory opens and
metadata reads. This keeps a concurrent symlink replacement from redirecting
the scan outside the selected root. Use checked arithmetic; overflow makes the
observation unavailable rather than wrapping or saturating the number. A
non-Unix build reports an unsupported-platform observation until an equivalent
allocated-block implementation exists.

### Keep Live Observation Separate from Persisted Status

Add a disk usage observer to the status report flow after the database
connection closes. In the workspace view, observe every displayed inventory
row, including removed rows with `--all`. In every view, also observe a
selected target, even if that target is excluded from the displayed inventory.
Deduplicate by workspace ID so a target appearing in the table is scanned
once and its observation is reused. Pool and repository views without a target
run no disk usage scans. Use a bounded number of concurrent scans so a large
inventory does not open an unbounded number of directories; preserve the
persisted inventory order in output.

Keep `WorkspaceStatus` and the existing `snapshot_at` semantics intact. Add
nullable top-level `target_disk_usage` and an ordered top-level
`workspace_disk_usage` array to the flattened version-2 report. Each array
entry carries a workspace ID and one observation; the array follows displayed
workspace inventory order and is empty in other views. The observer captures
its own `observed_at` at traversal start. Its work does not alter lifecycle
decisions or invoke external programs.

### Make Incomplete Observations Explicit

Return `complete` with exact observed allocated bytes when traversal finishes
without errors. Return `partial` with the sum of successfully inspected entries
when a descendant disappears or cannot be read. Return `unavailable` with a
null byte count when the root is missing, unreadable, or not a directory, the
platform cannot provide allocated blocks, or arithmetic overflows. Aggregate
stable issue codes (`root_missing`, `root_unreadable`, `entry_changed`,
`entry_unreadable`, `unsupported_platform`, `size_overflow`) with affected entry
counts where applicable. Do not print raw filesystem errors or paths in the
report. A partial sum is an observed subset, not a guaranteed lower bound of
any instantaneous filesystem state because files can change during the scan.

The observation does not change a successful status exit code. The target
remains selected even when its directory is missing. This is especially
important for removed workspace records.

### Render Compact Fields

Place `Disk usage` between `Repos` and `Processes` in the target summary, and
place `SIZE` between `REPOS` and `RECONCILED` in the workspace table. Show
bytes below 1024 as `N B`, otherwise use binary units with one decimal place,
such as `1.5 KiB` or `2.0 GiB`. The summary appends `(partial: REASONS)` for
partial scans or shows `unavailable (REASONS)` when no number exists. To keep
table rows compact, use `SIZE` values such as `1.5 KiB (partial)` or
`unavailable`; full issue codes remain in JSON and, for the target, the summary.
Map issue codes to fixed English reasons and keep existing field alignment and
terminal escaping. JSON carries exact `allocated_bytes`, `observed_at`,
`status`, and `issues` for each observation.

## Risks / Trade-Offs

- Directory scans are proportional to total entry count and can slow the
  workspace view when many rows are displayed. Deduplicate the selected target,
  bound scan concurrency, and measure representative large and many-workspace
  inventories during implementation.
- The tree can change while scanning, so even a complete observation is not an
  atomic filesystem snapshot. Document this and keep its timestamp separate
  from the SQLite snapshot.
- Filesystem allocation can differ from reclaimable space, especially with
  copy-on-write filesystems. Label the value as allocated disk usage and avoid
  using it for cleanup or capacity decisions.
- Strict JSON consumers may reject an added top-level key. Keep version 2 and
  all existing keys and object shapes unchanged, with regression coverage.

## Migration Plan

No database migration is required. Add the observer and report fields, then
human rendering and documentation. Rolling back removes the additive observation
fields, summary line, and table column without changing persisted data.
