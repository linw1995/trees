## Why

Source repository management should reuse the existing origin identity and Git
metadata. Local paths and remote URLs are two creation inputs, not persistent
repository policies. Additional mode, registration, root, and URL columns
would duplicate information or introduce an unnecessary lifecycle.

## What Changes

- Accept `create --repo <PATH|URL|NAME>` without a separate repo command group.
- Clone unknown URLs below configurable `repository.origins_dir`; reuse a unique
  existing source whose Git `remote.origin.url` matches the input exactly.
- Keep origin records limited to ID, canonical Git identity, and source path.
- Add `status --view repos --json` without mode, registration, root, or URL fields.
- Let `remove <ID>` delete an origin record without references while preserving source
  files, and reject removal when pool or worktree references remain.
- Keep interrupted-clone recovery in a transient operation table only.

## Capabilities

### New Capabilities

- `origin-repository-management`: Local registration, cloning, Git-based URL
  lookup, configurable placement, and reference-checked record removal.

### Modified Capabilities

- `workspace-management`: Resolve paths, URLs, and directory names before the
  existing workspace creation workflow.
- `workspace-status`: Add a read-only repos view using existing origin fields.
- `workspace-removal`: Dispatch IDs to workspace removal or origin record removal.

## Impact

The change affects CLI inputs, source preparation, Git inspection, status,
removal queries, documentation, and tests. The origin table remains unchanged.
The additional migration creates only the pending-clone operation table.
No source-file deletion, aliases, persistent repo modes, or soft deletion are
introduced. Existing workspace management modes remain unchanged.
