# Proposal

## Why

Live disk usage scans make `trees status --view workspaces` slower as the number
and size of workspaces grow. Size measurements should be refreshed at known
write boundaries or on explicit request, while status remains a fast read.

## What Changes

- Persist the latest disk usage observation for workspaces, their repository
  worktrees, and registered source repositories.
- Read stored observations in status without traversing directory trees. Show
  `unknown` until a value is measured, and retain its measurement time.
- Add `SIZE` to the source repository view and expose repository worktree sizes
  in workspace detail output and JSON.
- Add an explicit size refresh command for one workspace, one source repository,
  or all registered entities. Refresh sizes after successful workspace lifecycle
  operations.
- Keep filesystem observation failures nonfatal and store their structured
  status instead of presenting stale measurements as current.

## Capabilities

### New Capabilities

- `disk-usage-cache`: Persist and refresh disk usage observations for registered
  workspace and repository paths.

### Modified Capabilities

- `workspace-status`: Read cached sizes and render workspace and repository
  usage without scanning during status.

## Impact

This change adds a migration for three lifecycle tables, a size refresh CLI
command, lifecycle refresh integration, status projection and rendering updates,
tests, and status documentation. Existing databases need a writable command to
apply the migration before read-only status can run.
