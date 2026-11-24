# Show Container Workspace Content

## Why

A multi-repository workspace root is a container. Files or directories outside
the expected worktrees and managed `AGENTS.md` are not visible in status, so a
workspace can contain unexpected content while the summary looks clean.

## What Changes

- Observe workspace-root content for the selected container workspace.
- Add a `Content` line and a `WORKSPACE PATH` / `KIND` table.
- Exclude expected worktree paths and managed `AGENTS.md`.
- Add top-level `target_workspace_observation` to JSON reports.
- Keep the workspace inventory, repository worktree states, and labels on
  persisted state.
- Document read-only observation, entry limits, and failure handling.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-status`: selected-target container content observation, detail
  rendering, JSON addition, and the read-only side-effect constraint.

## Impact

Status target observation, detail rendering, JSON reports, documentation, and
tests change. No database migration or new dependency is needed.
