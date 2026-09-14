## Why

A workspace's repository set is currently fixed at creation, so expanding an ongoing task requires
creating another workspace and relocating work. Adding repositories in place must preserve existing
work and remain auditable and recoverable across Git, filesystem, and database failures.

## What Changes

- Add `trees add [WORKSPACE_DIR] --repo PATH|URL|NAME... [--offline] [--json]`, with the shared `--workspace-id`, `--workspace-dir`, and `--claim-id` selectors. All explicit selectors, including the positional directory, are mutually exclusive; omission selects the containing workspace.
- Support manual workspaces and actively claimed automatic workspaces, deduplicate repository identities, and preserve existing Git content and revisions.
- Promote a root-level single worktree to a named child when adding a second repository. Keep workspace identity and path stable, and report the changed repository path.
- Move automatic workspaces to the exact expanded repository-set pool while preserving the active claim.
- Persist an immutable `operations` row with `kind = add`, step intents, lifecycle events, lease ownership, compensation outcomes, and dedicated interrupted-addition recovery.
- Document structured results, path changes, retry behavior, and retained source clones.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-locator`: Include `add` in shared CLI selection without moving eligibility or mutation into the locator.
- `workspace-management`: Add repository inputs, target selection, idempotency, directory structure promotion, and result reporting.
- `workspace-reuse`: Define claim admission and repository-set pool migration for additions.
- `workspace-lifecycle`: Record and recover additions, directory structure moves, batch compensation, and atomic membership publication.

## Impact

Based on `origin/main` at `5ecd658` (shared workspace locator, #27). Reuses `WorkspaceLocatorArgs`,
`WorkspaceSelector`, and `locate` rather than extracting release-specific lookup. Touches CLI
dispatch, workspace orchestration, origin resolution, naming and Git helpers, Diesel storage,
reconciliation, and lifecycle integration tests. Existing status and workspace consumers must read
the resulting worktree paths and pool membership correctly. No new external dependency or database
table is planned; operations remain append-only. Existing create behavior is unchanged. Running
shells and tools cannot be transparently updated after single-worktree directory structure promotion.
