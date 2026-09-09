## MODIFIED Requirements

### Requirement: Select Pool or Workspace Status

The CLI SHALL provide `trees status [--view pools|workspaces|repos] [--all] [--json]`. The view SHALL default to `pools`. The pool view SHALL contain only
automatic repository-set pools with at least one current non-reclaimed
workspace. The workspace view SHALL contain individual automatic and manual
workspaces, exclude reclaimed records by default, and include reclaimed
tombstones with `--all`. The repos view SHALL contain registered origins and include unregistered
origins with `--all`. The CLI SHALL reject `--all` for the pools view.

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
  explicit workspace or repos view
- **THEN** it fails before opening lifecycle storage

#### Scenario: Select Origin Repositories

- **WHEN** status is invoked with `--view` repos
- **THEN** it reports registered origins of both management modes
- **AND** `--all` additionally includes retained unregistered origins

## ADDED Requirements

### Requirement: Render Read-Only Repository Status

The repos view SHALL render REPO, MODE, STATUS, PATH, and ID in stored
source-path then ID order. REPO SHALL use the base name and expand to a shortest
unique path suffix for collisions. MODE SHALL use the existing automatic/manual
presentation. STATUS SHALL report registered or unregistered metadata rather
than live Git health. Existing escaping and NO_COLOR rules SHALL apply.
An empty view SHALL print `No repositories.` and succeed.

The JSON envelope SHALL contain `schema_version` 1, view repos, `snapshot_at`, and
a repos array. Each row SHALL contain `origin_repository_id`, `source_path`, label,
`repository_identity`, `management_mode`, registered, nullable `managed_root`, and
nullable `remote_url`. Existing pools and workspaces JSON SHALL remain unchanged.

Repos SHALL use the same read-only snapshot guarantees as other status views,
without Git probes, migration, or clone recovery. Missing storage SHALL yield
an empty view without creating state; an incompatible schema or unreadable
snapshot SHALL fail without partial JSON output.

#### Scenario: Display Same-Named Origins

- **WHEN** registered sources include /teams/one/`api` and /teams/two/`api`
- **THEN** their display labels are one/`api` and two/`api` and both full paths and IDs remain available

#### Scenario: Inspect Without Side Effects

- **WHEN** repos status encounters a missing source or abandoned clone operation
- **THEN** it reports stored registered-origin metadata without probing or recovering filesystem state

#### Scenario: Emit Repos JSON

- **WHEN** status receives `--view` repos `--json`
- **THEN** standard output contains one version-1 repos snapshot with mode and ownership fields
