## MODIFIED Requirements

### Requirement: Select Pool or Workspace Status

The CLI SHALL provide `trees status [--view pools|workspaces|repos] [--all]
[--json]`. The view SHALL default to `pools`. The pool view SHALL contain only
automatic repository-set pools with at least one current non-removed
workspace. The workspace view SHALL contain individual automatic and manual
workspaces, exclude removed records by default, and include removed
tombstones with `--all`. The CLI SHALL reject `--all` unless the selected view
is `workspaces`.

#### Scenario: Default to Pool Allocation Status

- **WHEN** status is invoked without a view
- **THEN** it reports automatic repository-set pool allocation and capacity

#### Scenario: Select Workspace Details

- **WHEN** status is invoked with `--view workspaces`
- **THEN** it reports individual non-removed manual and automatic workspaces

#### Scenario: Include Removed Workspace Details

- **WHEN** status is invoked with `--view workspaces --all`
- **THEN** it additionally reports removed workspace tombstones

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
terminal escaping. JSON SHALL retain the version-2 envelope with `view: repos`
and a `repos` array containing `origin_repository_id`, `source_path`,
`repository_identity`, and `label`. No mode, registration state, root, or remote
URL SHALL be included. The view SHALL require no origin schema changes and
SHALL NOT invoke Git, run migrations, or recover clone operations. `--all`
SHALL remain unsupported for repos. Missing storage SHALL yield an empty view.

#### Scenario: Require the Current Lifecycle Schema

- **WHEN** repos status reads a database with pending lifecycle migrations
- **THEN** it reports a required schema upgrade without changing that database

#### Scenario: Inspect a Missing Source

- **WHEN** a stored source path is missing
- **THEN** repos status still lists its stored identity and path without probing Git
