# Inspect and Open Workspaces

[Back to Trees](../README.md#documentation)

Inspect the persisted workspace inventory without changing lifecycle or Git
state:

```sh
trees status
trees status --json
trees status --view workspaces
trees status --view workspaces --all
trees status --view repos
trees status --view repos --json
trees status WORKSPACE_ID
trees status WORKSPACE_ID --view repos --json
```

Status reads one consistent SQLite snapshot. It does not reconcile, recover an
expired operation, run Git, inspect workspace files, or assert that an
available workspace is currently reusable. Use
`trees gc --older-than 30d --dry-run` to check which workspaces currently qualify
for removal at a chosen age threshold.

## Target Workspace

Every view starts with a summary of the current workspace, including when run
from a nested repository directory. Status resolves the invocation directory
through symlinks and selects the nearest registered ancestor by path components.
An optional `WORKSPACE_ID` selects a different workspace regardless of the
invocation directory. Selection does not filter the global inventory.

For example, a claimed automatic workspace with no attached repositories renders
this summary before the pool inventory:

```text
Workspace (current directory)
  ID          01990000-0000-7000-8000-000000000001
  Path        /work/api
  Status      ready 🔒
  Mode        automatic 🤖
  Repos       0/0
  Reconciled  2026-09-11 14:32:05

Pools
No workspace pools.
```

An explicit ID changes the heading to `Workspace (selected by ID)`. Manual mode
uses `manual 👤`. A retained operation adds an `Operation` line between Mode
and Repos, for example `release / running (lease expired)`; active and
inconsistent leases are also identified. Status reports these operations without
recovering them. Claim information appears only as the lock on Status.

Summary reconciliation times use local `YYYY-MM-DD HH:MM:SS`, with the timezone
offset applicable at the recorded instant, or `never` when absent. If local
timezone lookup fails, the summary uses UTC without a suffix. Inventory tables
keep their existing compact UTC times. Health and repository readiness are
persisted observations, not live Git cleanliness checks.

Outside any registered workspace, status silently prints the existing inventory
without a summary, extra heading, or diagnostic. A removed workspace can still
be selected without `--all`, even if its directory no longer exists. The nearest
registered boundary also wins when removed. Unknown explicit IDs fail with no
standard output, including before a database exists. A target that belongs to
the workspace inventory appears in both the summary and its normal table row.

## JSON Snapshot

All three version-2 views add `target_workspace`: a complete workspace object
with the same fields as a workspace inventory entry, or `null` outside any
workspace. The existing `pools`, `workspaces`, or `repos` array remains global.
A removed target remains present even when excluded from the workspace array.

Target and inventory relationships share one read-only transaction and one
`snapshot_at`, including lease classification. JSON preserves original paths,
RFC 3339 timestamps, and structured claim and operation objects. Human headings,
escaping, colors, and emoji do not change the JSON contract. Prefer JSON for
scripts; terminal summaries add lines when a target is found.

## Pool Capacity

The default `pools` view reports one row per automatic repository set.
`CAPACITY` is `<available>/<total>/<abnormal>`:

| Value | Meaning | Terminal color |
| --- | --- | --- |
| Available | Persisted ready slots without a claim or current operation. | Green |
| Total | Current slots that have not been removed. | Blue |
| Abnormal | Degraded or failed slots. | Red |

Pipelines and `NO_COLOR` receive plain values without ANSI escapes. Availability
is a scheduling hint; automatic allocation still reconciles a slot before use.
Repository labels start with source-path base names and expand conflicting
labels with parent components until unique within the pool.

## Workspace Health

Use `--view workspaces` for individual manual and automatic workspaces.
`STATUS` shows workspace health and appends `🔒` when an active claim exists;
absence of the lock means unclaimed. Automatic mode is shown as `🤖`, while
manual mode is shown as `👤`. Removed workspace records are hidden by default;
`--all` includes them in this detail view. `--json` emits a versioned
snapshot for the selected view. Workspace JSON retains separate state and claim
fields plus complete current operation, path, and repo-worktree details.

Workspace `REPOS` uses `<ready>/<total>` followed by repository labels. Ready
is the user-facing name for repo worktrees stored in the `attached` state. On
interactive terminals, ready and total are green and blue; pipelines and
`NO_COLOR` receive the same uncolored value. Ready repository labels are green,
pending labels are yellow, and problem labels are red. Non-ready repositories
also retain explicit suffixes such as `(dirty)`, `(missing)`, `(mismatch)`, and
`(error)` in plain output. Removed labels are gray and use `(removed)`.
Path-derived labels escape control characters and table delimiters before color
is applied, preventing repository names from injecting terminal output.

## Source Repositories

Use `--view repos` for source repositories, with `REPO`, `PATH`, and `ID`
columns. Conflicting labels expand to unique path suffixes. JSON uses the
version-2 envelope with `view: "repos"` and a `repos` array containing
`origin_repository_id`, `source_path`, `repository_identity`, and `label`.
This view reads stored metadata without probing Git, migrating storage, or
recovering clone operations. A missing source remains visible. `--all` applies
only to the workspace view; repos has no hidden registration state.

## Open an Existing Workspace

```sh
trees open WORKSPACE_ID
trees open WORKSPACE_ID --program=codex
```

The human workspace view identifies each record by stable workspace ID rather
than path. `trees open` resolves that ID and starts `$SHELL` in the persisted
canonical workspace directory; `--program=<PROGRAM>` selects another executable
without shell parsing. Automatic workspaces must already have an active claim,
while manual workspaces do not require one. Open rejects removed workspaces
and retained operation leases, closes its read-only database connection before
handoff, and does not reconcile or mutate lifecycle state.

See [Workspace lifecycle](workspaces.md) for allocation and release, and
[Cleanup](cleanup.md) for cleanup and explicit removal.

Status JSON uses schema version 2: terminal workspace and repo-worktree states
are `removed`, and workspace removal timestamps use `removed_at`.
GC uses `safe_to_remove` and `removed` counters. Writable database access upgrades
older databases automatically; read-only commands report a required schema
upgrade until that has happened. Existing audit history remains unchanged.
