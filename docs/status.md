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
```

Status reads one consistent SQLite snapshot. It does not reconcile, recover an
expired operation, run Git, inspect workspace files, or assert that an
available workspace is currently reusable. Use
`trees gc --older-than 30d --dry-run` to check which workspaces currently qualify
for reclamation at a chosen age threshold.

## Pool Capacity

The default `pools` view reports one row per automatic repository set.
`CAPACITY` is `<available>/<total>/<abnormal>`:

| Value | Meaning | Terminal color |
| --- | --- | --- |
| Available | Persisted ready slots without a claim or current operation. | Green |
| Total | Current slots that have not been reclaimed. | Blue |
| Abnormal | Degraded or failed slots. | Red |

Pipelines and `NO_COLOR` receive plain values without ANSI escapes. Availability
is a scheduling hint; automatic allocation still reconciles a slot before use.
Repository labels start with source-path base names and expand conflicting
labels with parent components until unique within the pool.

## Workspace Health

Use `--view workspaces` for individual manual and automatic workspaces.
`STATUS` shows workspace health and appends `🔒` when an active claim exists;
absence of the lock means unclaimed. Automatic mode is shown as `🤖`, while
manual mode is shown as `👤`. Reclaimed workspace records are hidden by default;
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
version-1 envelope with `view: "repos"` and a `repos` array containing
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
while manual workspaces do not require one. Open rejects reclaimed workspaces
and retained operation leases, closes its read-only database connection before
handoff, and does not reconcile or mutate lifecycle state.

See [Workspace lifecycle](workspaces.md) for allocation and release, and
[Cleanup](cleanup.md) for reclamation checks and removal.
