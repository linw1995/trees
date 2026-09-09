## MODIFIED Requirements

### Requirement: Select Pool or Workspace Status

The CLI SHALL provide `trees status [--view pools|workspaces|repos] [--all]
[--json]`. The view SHALL default to `pools`. The pool view SHALL contain only
automatic repository-set pools with at least one current non-reclaimed
workspace. The workspace view SHALL contain individual automatic and manual
workspaces, exclude reclaimed records by default, and include reclaimed
tombstones with `--all`. The CLI SHALL reject `--all` unless the selected view
is `workspaces`.

#### Scenario: Default to Pool Allocation Status

- **WHEN** status is invoked without a view
- **THEN** it reports automatic repository-set pool allocation and capacity

#### Scenario: Select Workspace Details

- **WHEN** status is invoked with `--view workspaces`
- **THEN** it reports individual non-reclaimed manual and automatic workspaces

#### Scenario: Include Reclaimed Workspace Details

- **WHEN** status is invoked with `--view workspaces --all`
- **THEN** it additionally reports reclaimed workspace tombstones

#### Scenario: Reject All for Pool Status

- **WHEN** status is invoked with `--view pools --all` or `--all` without an
  explicit workspace view
- **THEN** it fails before opening lifecycle storage

#### Scenario: Select Origin Repositories

- **WHEN** status uses `--view repos`
- **THEN** it lists all stored origins using their existing identity and path

## ADDED Requirements

### Requirement: Render Existing Repository Metadata

The repos human view SHALL render `REPO`, `PATH`, and `ID`, ordered by source
path then ID. Labels SHALL use shortest unique path suffixes and existing
terminal escaping. JSON SHALL retain the version-1 envelope with `view: repos`
and a `repos` array containing `origin_repository_id`, `source_path`,
`repository_identity`, and `label`. No mode, registration state, root, or remote
URL SHALL be included. The view SHALL require no origin schema changes and
SHALL NOT invoke Git, run migrations, or recover clone operations. `--all`
SHALL remain unsupported for repos. Missing storage SHALL yield an empty view.

#### Scenario: Inspect Existing Rows Without Migration

- **WHEN** repos status reads a database from before the clone-operation migration
- **THEN** it lists existing origins without changing that database

#### Scenario: Inspect a Missing Source

- **WHEN** a stored source path is missing
- **THEN** repos status still lists its stored identity and path without probing Git
