## MODIFIED Requirements

### Requirement: Resolve the Target Workspace Independently of View

An explicit `WORKSPACE_ID` SHALL select that stored workspace regardless of current directory,
management mode, claim, operation, health, removed state, or path existence.
Without an explicit selector, status SHALL select the nearest stored workspace whose canonical
path equals or contains canonical current directory by path components. Target lookup SHALL
include removed records and SHALL NOT depend on `--all`. An unknown explicit
ID SHALL produce a nonzero error on standard error and no standard output. An unmatched current directory
SHALL silently omit the human summary and leave the selected inventory intact.

#### Scenario: Resolve Nested Repository Directories

- **WHEN** status runs within a repository subdirectory inside a registered workspace
- **THEN** that workspace is selected for every view

#### Scenario: Explicit Identifier Selection

- **WHEN** current directory belongs to one workspace and a different valid ID is supplied
- **THEN** the supplied ID selects the target

#### Scenario: Resolve the Nearest Boundary

- **WHEN** multiple registered workspace paths contain canonical current directory
- **THEN** the closest ancestor is selected, including a removed boundary

#### Scenario: Avoid String Prefix Matches

- **WHEN** current directory is `/workspaces/api-extra` and only `/workspaces/api` is registered
- **THEN** it does not select that workspace

#### Scenario: Resolve a Symlinked Invocation Directory

- **WHEN** current directory is reached through a symlink into a registered workspace
- **THEN** canonical current directory resolves to that workspace

#### Scenario: Stay Silent Outside a Workspace

- **WHEN** no ID is supplied and no stored workspace contains current directory
- **THEN** no summary, no summary placeholder, no extra blank line, and no
  diagnostic is emitted; only the existing selected inventory is rendered

#### Scenario: Inspect a Missing Removed Target Path

- **WHEN** the supplied ID identifies a removed workspace whose directory is absent
- **THEN** status reports its persisted removed state successfully

Status SHALL also accept `--workspace-id`, `--workspace-dir`, or `--claim-id`.
All named selectors and the positional ID SHALL be mutually exclusive. A
workspace directory SHALL select only an exact root; a claim SHALL select its
associated workspace. An unmatched explicit selector SHALL fail with no standard
output, including when storage is absent. Path and claim selection SHALL have
appropriate human summary headings without changing the JSON structure.

#### Scenario: Select an Explicit Path or Claim

- **WHEN** status receives one valid path or claim selector
- **THEN** it reports that workspace regardless of the selected inventory view
- **AND** inventory filtering remains unchanged
