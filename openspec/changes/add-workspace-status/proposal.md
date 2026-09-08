## Why

Trees allocates automatic workspaces from repository-set pools, but users
cannot see total slots of each kind, claimed slots, or immediately available
slots in persisted state. Reading individual workspace rows does
not answer this capacity-planning question directly.

## What Changes

- Add a read-only `trees status [--view pools|workspaces] [--all] [--json]`
  command with a pool allocation default and an explicit workspace detail
  view.
- Report persisted available, total, and abnormal slot counts in one compact
  capacity value for each pool.
- Identify each pool by repository labels using the shortest unique source-path
  suffix within that repository set.
- Keep pool output compact with abbreviated UTC update times while retaining
  canonical paths and lifecycle details in the workspace view.
- Color available, total, and abnormal capacity counts green, blue, and red on
  interactive terminals while preserving plain output for pipelines.
- Emit a versioned JSON document matching the selected view.
- Keep status observational: it does not reconcile, recover, acquire, release,
  run Git, inspect the filesystem, or append lifecycle events.

## Capabilities

### New Capabilities

- `workspace-status`: Read and render consistent persisted pool allocation and
  workspace detail snapshots.

### Modified Capabilities

None.

## Impact

The CLI parser and dispatch, pool and lifecycle repository queries, status
projection and rendering, README usage, and integration tests change. No
persistence schema or Git mutation workflow changes.
