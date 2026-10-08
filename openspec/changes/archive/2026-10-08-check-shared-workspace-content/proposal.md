# Proposal

## Why

Multi-repository workspaces can contain local content outside their Git
worktrees, including a shared `public/` directory. Git cleanliness checks miss
that content and can incorrectly release or reuse the workspace.

## What Changes

- Include unexpected shared root entries in workspace health observations.
- Reject release before alignment while preserving content and the active claim.
- Skip affected slots during automatic allocation.
- Reuse the existing root content validator and retain focused regression tests.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-reuse`: Require shared root content to be clean before release
  or automatic reuse.

## Impact

Workspace reconciliation, alignment preflight, shared validation, tests, and
workspace documentation change. No schema migration or CLI option is needed.
