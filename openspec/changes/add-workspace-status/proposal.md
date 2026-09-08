## Why

Trees persists workspace health, claims, operation leases, and repo-worktree
observations, but users cannot inspect that state without reading SQLite or
running a command with mutation semantics. `gc --dry-run` answers a narrower
reclamation question and does not represent manual, claimed, unhealthy, or
recent workspaces as a general inventory.

## What Changes

- Add a read-only `trees status [--all] [--json]` command that lists the
  current persisted state of managed workspaces.
- Show workspace health separately from usage claims and current operation
  leases so that `ready`, `claimed`, and `busy` are not collapsed into one
  ambiguous status.
- Summarize repo-worktree states in human output and expose complete structured
  workspace, claim, operation, and repo-worktree data in versioned JSON.
- Exclude reclaimed workspace tombstones by default and include them with
  `--all`.
- Keep status observational: it does not reconcile, recover, acquire, release,
  run Git, inspect the filesystem, or append lifecycle events.

## Capabilities

### New Capabilities

- `workspace-status`: Read and render a consistent persisted workspace status
  snapshot.

### Modified Capabilities

None.

## Impact

The CLI parser and dispatch, lifecycle repository queries, status projection
and rendering, README usage, and integration tests change. No persistence
schema or Git mutation workflow changes.
