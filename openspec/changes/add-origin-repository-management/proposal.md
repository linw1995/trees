## Why

Trees already records source repositories while creating workspaces. Origin
management should extend that workflow: accept remote URLs when a checkout is
needed, expose repositories in status, and accept their IDs in remove without
introducing a separate command group or alias registry.

## What Changes

- Extend `create --repo <PATH|URL|NAME>` to accept local paths, remote URLs, and unambiguous
  registered directory names. Local paths register manual origins; remote URLs
  provision or reuse automatic origins before workspace creation.
- Add `config set origins-dir <PATH>` for future automatic clones. Persist
  ownership explicitly; manual repositories can reside inside that directory.
- Use source directory names and stable IDs, with no aliases or rename command.
- Add `status --view repos [--all] [--json]` using existing snapshot conventions.
- Extend `remove <ID>` to dispatch to workspace removal or repo registration removal.
- Preserve local Git identity and existing workspace allocation, revision,
  lifecycle, and pool semantics across both repo management modes.

## Capabilities

### New Capabilities

- `origin-repository-management`: Automatic provisioning, manual registration,
  directory configuration, ownership, and safe repository registration removal.

### Modified Capabilities

- `workspace-management`: Resolve inputs by path, URL, and directory-name repo inputs
  before the existing manual or automatic workspace creation workflow.
- `workspace-status`: Add a read-only repos view with versioned JSON.
- `workspace-removal`: Dispatch an explicit stable ID to a workspace or origin.

## Impact

Changes affect CLI parsing and dispatch, origin input resolution, configuration,
Git clone execution and recovery, schema migrations and typed storage, status,
remove, documentation, and integration tests. No `repo` command, alias table,
`--name`, `--remote-url`, or `--origin` option is introduced.

Physical deletion of successfully registered origins, mode conversion,
relocation, remote editing, and automatic repository GC remain outside scope.
