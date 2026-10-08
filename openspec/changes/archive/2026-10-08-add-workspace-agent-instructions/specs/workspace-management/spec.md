## ADDED Requirements

### Requirement: Generate Shared Workspace Instructions

Trees SHALL create a root `AGENTS.md` for manual and automatic multi-repository
workspaces during creation, expansion, and automatic reuse when the file is
missing. Existing shared entries SHALL NOT be overwritten. A workspace whose
root is a single worktree SHALL retain its repository instructions unchanged.

#### Scenario: Create a Shared Workspace

- **WHEN** Trees creates a workspace containing multiple child worktrees
- **THEN** the shared root contains the generated `AGENTS.md` and repository instructions remain inside their worktrees

#### Scenario: Expand a Root Worktree

- **WHEN** adding a repository moves the original root worktree into a child directory
- **THEN** its instruction file moves with it and Trees generates separate shared instructions

#### Scenario: Restore Missing Instructions During Reuse

- **WHEN** automatic allocation reuses a safe multi-repository workspace without root instructions
- **THEN** it creates the generated file before granting the new claim

#### Scenario: Recover a Completed Addition

- **WHEN** an interrupted addition has completed its worktrees but lacks shared instructions
- **THEN** recovery creates the missing instruction file before publishing the layout

#### Scenario: Preserve Existing Instructions

- **WHEN** a shared `AGENTS.md` already exists during instruction preparation
- **THEN** Trees preserves it without overwriting its content or following a symlink

#### Scenario: Preserve a Single Worktree

- **WHEN** a workspace root is itself a single Git worktree
- **THEN** Trees does not inject or replace its repository instruction file

### Requirement: Explain Workspace Layout and Cleanliness

The generated instructions SHALL describe a monorepo-style workspace of
independent Git worktrees, repository-local Git commands and instructions,
named-branch preservation, external temporary artifacts, and untracked and
ignored file inspection. They SHALL keep this guidance concise and emphasize
preserving valuable work. Workspace release SHALL remain outside these agent
instructions.

#### Scenario: Prepare a Workspace for Recycling

- **WHEN** an agent reads the generated instructions
- **THEN** it can identify the independent repositories and the checks needed to leave the workspace clean for release and cleanup

## MODIFIED Requirements

### Requirement: Derive Multi-Repository Child Paths from Repository Names

For multiple repositories, Trees SHALL use each source repository's base name
as its direct child directory name. Duplicate names or the reserved name
`AGENTS.md` SHALL cause creation or addition to fail before mutating worktrees.
The instruction name reservation SHALL NOT apply to a single root worktree.

#### Scenario: Use Repository Names

- **WHEN** the user creates a workspace from repositories named `api` and `web`
- **THEN** the workspace contains direct child worktrees named `api` and `web`

#### Scenario: Reject a Name Collision

- **WHEN** two repository inputs resolve to the same base name
- **THEN** the create operation fails before creating the workspace worktrees

#### Scenario: Reject the Shared Instruction Name

- **WHEN** creation or addition would place a worktree at the shared root's `AGENTS.md` path
- **THEN** planning rejects the name and preserves the existing workspace layout

#### Scenario: Allow the Name for a Single Root Worktree

- **WHEN** the only selected source repository is named `AGENTS.md`
- **THEN** Trees can create its worktree directly at the workspace root
