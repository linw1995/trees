# Proposal

## Why

Agents entering a multi-repository workspace need to understand its independent
Git worktrees and leave it clean enough for Trees to release, reuse, or remove.
A generated root file must also satisfy the existing shared-content guards.

## What Changes

- Generate root `AGENTS.md` instructions during creation, expansion, and reuse.
- Preserve repository instructions and existing shared instruction files.
- Recognize unchanged generated files during root validation and cleanup.
- Reserve the instruction filename in shared worktree layouts.
- Preserve existing containers when their creation fails.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-management`: Generate workspace instructions and reject filename collisions.
- `workspace-reuse`: Recognize generated instructions without weakening content preservation.

## Impact

Workspace creation, additions, reuse, validation, cleanup, Nix source packaging,
and documentation change. No CLI option or schema migration is needed.
