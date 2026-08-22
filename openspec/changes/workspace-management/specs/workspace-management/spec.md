## Purpose

This capability defines how `trees` creates a workspace that physically associates multiple independent Git repositories through direct child worktrees.

## ADDED Requirements

### Requirement: Create a Workspace with Direct Child Worktrees

The CLI SHALL provide `trees create <workspace-path> --repo <repository-path>...`. For every repository input, the command SHALL create one distinct direct child directory below the workspace path. Each child directory SHALL be a Git worktree physically associated with its source repository. The workspace SHALL NOT require an additional repository container directory.

#### Scenario: Create a Workspace from Multiple Repositories

- **WHEN** the user creates a workspace with two valid repository paths
- **THEN** the workspace contains two direct child directories, and each child is recognized by Git as a worktree of the corresponding source repository

#### Scenario: Keep Source Repositories Outside the Workspace

- **WHEN** a workspace is created
- **THEN** the source repository working trees remain at their original paths, while the workspace children provide the associated worktrees

### Requirement: Derive Child Paths from Repository Names

The CLI SHALL use each source repository's base name as the corresponding direct child directory name. If two inputs resolve to the same base name, the create operation SHALL fail before creating any worktree rather than silently choosing a different layout.

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
- **THEN** the direct child worktree points to the source repository's current `HEAD` and has no checked-out local branch

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
