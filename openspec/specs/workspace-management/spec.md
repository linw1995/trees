# Workspace Management Specification

## Purpose

This capability defines how `trees` creates a workspace that physically associates one or more independent Git repositories through Git worktrees.

## Requirements

### Requirement: Create a Workspace with Direct Child Worktrees

The CLI SHALL provide `trees create <workspace-path> --repo <repository-path>...`. When given one repository, the command SHALL create its Git worktree directly at the workspace path. When given multiple repositories, the command SHALL create one distinct direct child directory below the workspace path for every repository input. Each worktree SHALL be physically associated with its source repository. The workspace SHALL NOT require an additional repository container directory.

#### Scenario: Create a Workspace from One Repository

- **WHEN** the user creates a workspace with one valid repository path
- **THEN** the workspace path itself is recognized by Git as a worktree of the source repository and no repository-name child directory is added

#### Scenario: Create a Workspace from Multiple Repositories

- **WHEN** the user creates a workspace with two valid repository paths
- **THEN** the workspace contains two direct child directories, and each child is recognized by Git as a worktree of the corresponding source repository

#### Scenario: Keep Source Repositories Outside the Workspace

- **WHEN** a workspace is created
- **THEN** the source repository working trees remain at their original paths, while the workspace layout provides the associated worktrees

### Requirement: Derive Multi-Repository Child Paths from Repository Names

For a workspace containing multiple repositories, the CLI SHALL use each source repository's base name as the corresponding direct child directory name. If two inputs resolve to the same base name, the create operation SHALL fail before creating any worktree rather than silently choosing a different layout.

#### Scenario: Use Repository Names

- **WHEN** the user creates a workspace from repositories named `api` and `web`
- **THEN** the workspace contains direct child worktrees named `api` and `web`

#### Scenario: Reject a Name Collision

- **WHEN** two repository inputs resolve to the same base name
- **THEN** the create operation fails before creating the workspace worktrees

### Requirement: Create Detached Worktrees at the Current Revision

The initial create operation SHALL create each worktree from the source repository's current `HEAD` in detached mode. Branch and ref selection SHALL remain outside the initial command contract.

#### Scenario: Create a Detached Worktree

- **WHEN** the user creates a workspace from a valid repository without branch or ref options
- **THEN** the created worktree points to the source repository's current `HEAD` and has no checked-out local branch

### Requirement: Reject Unsafe Workspace Targets

The CLI SHALL reject a workspace target that already exists or cannot be safely prepared, and SHALL NOT modify an existing target as part of the rejected operation.

#### Scenario: Existing Workspace Target

- **WHEN** the requested workspace path already exists
- **THEN** the create operation fails and the existing directory contents remain unchanged

#### Scenario: Invalid Repository Input

- **WHEN** any repository input is not a valid Git repository
- **THEN** the create operation fails without publishing a workspace containing only a subset of the requested repositories

### Requirement: Clean up Partial Repository Setup

If a repository worktree operation fails after earlier worktrees were created, the CLI SHALL remove the worktrees created by that operation and SHALL NOT leave a partially assembled workspace as a successful result. The failure and cleanup details SHALL be covered by the lifecycle event capability.

#### Scenario: Failure During Later Repository Setup

- **WHEN** a later repository worktree cannot be created after an earlier worktree succeeded
- **THEN** the earlier worktree is removed, the workspace is not reported as successfully created, and the failure is available to lifecycle tracking

### Requirement: Select Automatic Pool Allocation or Manual Provisioning

The CLI SHALL support two creation forms. Automatic creation SHALL be
`trees create --repo <repository-path>...` without a positional workspace path.
It SHALL resolve a repository-set pool backed by a UUID, using a non-unique
hash lookup index and the exact sorted origin repository ID set. Pool identity
SHALL be independent of the workspace root. It SHALL reuse a safe idle automatic
workspace referencing that pool when one exists, regardless of that slot's
persisted root. Otherwise, it SHALL generate a workspace path below the
currently configured Trees-managed workspace root and provision a new
automatic workspace. Manual creation SHALL be `trees create
<workspace-path> --repo <repository-path>...` and SHALL retain the existing
  repository-count-based worktree structure and detached initial worktree behavior. The
presence of a positional workspace path SHALL be the mode discriminator; no
additional `--mode` flag is required or accepted.

#### Scenario: Allocate an Existing Automatic Workspace

- **WHEN** the caller runs automatic create with repository directories whose
  canonical Git common-directory identities match an idle reusable pool slot
- **THEN** Trees acquires that existing workspace path with a new claim
  identifier without creating another workspace or Git worktree

#### Scenario: Provision a New Automatic Slot

- **WHEN** the caller runs automatic create and no reusable automatic workspace
  matches the exact repository set
- **THEN** Trees generates a path under its currently configured managed
  workspace root, creates detached worktrees using the repository-count-based layout, records the
  pool UUID, and returns the new workspace with an active claim

#### Scenario: Create a Manual Workspace

- **WHEN** the caller provides an explicit workspace path
- **THEN** the workspace is recorded as `manual`, its worktrees are created
  with the existing create semantics, and automatic pool allocation and GC do
  not select it

#### Scenario: Reject Conflicting Creation Forms

- **WHEN** the caller supplies a mode-selection flag or otherwise attempts to
  combine automatic repository-only arguments with a positional workspace
  path
- **THEN** argument validation fails before any database, Git, or filesystem
  mutation
