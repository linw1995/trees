## Why

`trees gc` only selects idle automatic workspaces by age. Users currently have
no explicit way to remove one known workspace, including a manually managed
workspace, while preserving lifecycle history and the existing removal safety
checks.

## What Changes

- Add `trees remove <workspace-id> [--dry-run] [--yes] [--force]`.
- Allow explicit removal of either automatic or manual workspaces.
- Reuse the GC physical validation and removal behavior without applying an age
  threshold or automatic-management filter.
- Reject claimed, actively operated, unknown, or already reclaimed workspaces.
- Keep `--force` bounded by claim, operation, path-containment, source identity,
  and branch-attachment guards.
- Preserve reclaimed workspace and repo-worktree tombstones and lifecycle
  events after physical removal.

## Capabilities

### New Capabilities

- `workspace-removal`: Explicitly remove one managed workspace by stable ID.

### Modified Capabilities

- `workspace-lifecycle`: Record explicit removal through the existing
  tombstone and operation model.

## Impact

The CLI parser and dispatch, removal workflow, lifecycle event metadata,
README usage, and integration tests change. No persistence schema changes.
