# Proposal

## Why

Users cannot reserve a specific existing automatic workspace without going through pool allocation. A standalone claim command lets them resume work in that workspace while preserving its current Git state and preventing another allocation or ordinary cleanup from taking it.

## What Changes

- Add `trees claim [WORKSPACE_DIR]`, `trees claim --workspace-id WORKSPACE_ID`, and `--json`; default to the nearest workspace containing the current directory.
- Intentionally omit `--workspace-dir` and `--claim-id`; positional paths and workspace IDs are mutually exclusive.
- Claim a structurally valid automatic workspace without fetching, checking out revisions, or changing files. Permit dirty files, attached branches, and changed revisions.
- Reject active claims, manual workspaces, structural damage, and unfinished operations without selecting another workspace.
- Publish the new claim and successful lifecycle outcome atomically using existing claims and operation leases.
- Preserve existing create, release, status, GC, and explicit removal behavior.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-reuse`: Add explicit in-place claim semantics and distinguish them from the reusable snapshot required by automatic allocation.
- `workspace-locator`: Define the smaller claim CLI selector surface while reusing shared lookup semantics.
- `workspace-lifecycle`: Define atomic claim publication and conservative handling of interrupted operations.

## Impact

Changes affect CLI arguments and dispatch, workspace orchestration, lease-owned storage transactions, focused integration tests, and workspace documentation. Existing typed errors and module boundaries remain in use. No new dependency or database migration is expected. This change contains planning artifacts only until implementation begins.
