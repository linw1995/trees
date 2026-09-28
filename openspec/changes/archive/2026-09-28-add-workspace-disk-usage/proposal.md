# Proposal

## Why

Status shows lifecycle and process state but not how much local disk space each
workspace occupies. Users currently need a separate filesystem command to
compare workspace sizes before cleanup.

## What Changes

- Add a live `Disk usage` line to the selected workspace summary in every
  `trees status` view.
- Add a `SIZE` column for every displayed row of `--view workspaces`, including
  removed rows shown with `--all`.
- Measure allocated space beneath each relevant workspace root and report
  complete, partial, or unavailable observations without failing status.
- Add top-level `target_disk_usage` and `workspace_disk_usage` observations to
  version-2 JSON, separate from persisted workspace and inventory objects.
- Reuse one observation when the selected target also appears in the workspace
  table; leave pool and repository inventories free of directory scans when no
  target is selected.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-status`: Permit disk usage scans for the selected target and
  displayed workspace rows, with human and JSON observations.

## Impact

The change affects status report orchestration, a new disk usage observer,
workspace inventory and target summary rendering, tests, and `docs/status.md`.
It needs no database migration or new command-line flag. The workspace view
adds filesystem metadata reads proportional to the entries beneath its displayed
workspaces.
