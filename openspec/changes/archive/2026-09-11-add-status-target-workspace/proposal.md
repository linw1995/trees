## Why

Status currently shows a global inventory without identifying the workspace
being used. Every view should also show the current or explicitly selected
workspace so users can inspect its persisted state without finding it in a list.

## What Changes

- Add an optional `WORKSPACE_ID` positional argument to `trees status`.
- Resolve the nearest containing workspace from the current directory when no
  ID is supplied, including execution from nested repository directories.
- Prepend a compact workspace summary to every human view, with identity, path,
  health with a claim marker, mode, repository readiness, and reconciliation time.
- Keep claim information on the status line only; place mode emoji after text
  and display local summary timestamps without a timezone suffix.
- Omit the entire summary silently when the current directory has no workspace.
- Allow explicit inspection of removed workspaces independently of `--all`;
  reject an unknown explicit ID without printing a partial result.
- Add nullable `target_workspace` to each existing version-2 JSON view, sharing
  one database snapshot and timestamp with the global inventory.
- Preserve global view selection, ordering, filtering, and read-only behavior.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-status`: Select a target independently of the inventory view,
  render its summary, and include it in a consistent JSON snapshot.

## Impact

Affects `src/cli.rs`, status orchestration in `src/main.rs`, snapshot loading and
rendering in `src/status.rs` and `src/status/repos.rs`, and storage queries in
`src/storage/repository.rs`. Requires CLI, rendering, snapshot consistency, and
read-only regression coverage plus updates to `docs/status.md`. No lifecycle
schema migration is needed. Local time conversion may require enabling an
existing time-library feature or using a small platform time helper.

## Excluded Work

Live Git status, recovery, allocation, workspace mutation, additional views,
and implementation of this proposal during the planning step.
