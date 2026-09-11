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

### Requirement: Create Detached Worktrees at the Upstream Revision

The initial create operation SHALL resolve every repository argument to its
upstream primary worktree and fetch before selecting the target revision. When
the current branch of the primary worktree has a tracking upstream, create
SHALL use the fetched tracking revision. Otherwise, it SHALL use the current
local `HEAD` of the primary worktree. It SHALL create the new worktree at that
revision in detached mode and SHALL NOT check out or reset the input repository. This rule
SHALL apply whether the argument names the upstream repository itself or one of
its linked workspace worktrees. Explicit branch and ref selection SHALL remain
outside the initial command contract. When `--offline` is present, create SHALL
skip fetching and SHALL select the current local `HEAD` of the primary worktree.

#### Scenario: Create a Detached Worktree

- **WHEN** the user creates a workspace from an upstream repository or one of its linked workspace worktrees without branch or ref options
- **THEN** create fetches using the configured fetch remote of the primary worktree
- **AND** the created worktree points to the fetched tracking revision when the current primary branch has a tracking upstream
- **AND** the created worktree otherwise points to the current local `HEAD` of the primary worktree
- **AND** the created worktree has no checked-out local branch

#### Scenario: Create Offline

- **WHEN** the user creates a workspace with `--offline`
- **THEN** create does not fetch any repository
- **AND** every created worktree points to the current local `HEAD` of its
  primary worktree
- **AND** manual and automatic create use the same offline revision selection

#### Scenario: Create from a Divergent Workspace Repo

- **WHEN** a linked workspace repo input has a different `HEAD` from its
  upstream primary worktree
- **THEN** create leaves the input repo unchanged and creates the target at the
  selected upstream revision

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

### Requirement: Open a Program in a Created Workspace

The CLI SHALL accept `--open[=<PROGRAM>]` for manual and automatic create.
When the option has no explicit program, Trees SHALL resolve the program from a
nonempty `$SHELL`. Trees SHALL reject a missing or empty default, an explicitly
empty program, or the combination of `--open` and `--json` before database,
Git, or filesystem mutation. After successful creation or allocation, Trees
SHALL start the selected program with the canonical workspace path as its
current directory and with inherited standard streams and environment. Where
process replacement is supported, the program SHALL replace the Trees process.
An automatic workspace SHALL retain its active claim after the opened program
exits.

#### Scenario: Open the Default Shell

- **WHEN** create receives `--open` and `$SHELL` identifies a program
- **THEN** Trees opens that program with the created or allocated workspace as
  its current directory

#### Scenario: Open an Explicit Program

- **WHEN** create receives `--open=<PROGRAM>`
- **THEN** Trees opens that executable directly without parsing its value as shell syntax

#### Scenario: Preserve the Parent Shell Directory

- **WHEN** the opened program exits and control returns to the shell that invoked Trees
- **THEN** that parent shell remains in its original directory

#### Scenario: Retain an Automatic Claim

- **WHEN** a program opened after automatic allocation exits
- **THEN** the allocated workspace remains claimed until an explicit release succeeds

#### Scenario: Reject an Unavailable Default Shell

- **WHEN** create receives `--open` while `$SHELL` is unset or empty
- **THEN** create fails before any workspace state changes

#### Scenario: Reject Conflicting Create Modes

- **WHEN** create receives both `--open` and `--json`
- **THEN** argument parsing fails before any workspace state changes

### Requirement: Resolve Paths URLs and Directory Names in Repo Inputs

`trees create [WORKSPACE_PATH] --repo <PATH|URL|NAME>...` SHALL accept local paths,
supported remote URLs, and unambiguous registered directory base names. Explicit
absolute, dot-relative, and Windows drive paths SHALL be paths. Supported URI
schemes and `SCP`-style host paths SHALL be URLs. Other inputs SHALL be paths;
a nonexistent single-component relative path SHALL resolve by registered
primary directory base name only when exactly one origin matches. Existing local
directories SHALL take precedence. NAME SHALL match the exact stored primary
directory base name across existing origins and reuse
that origin without cloning a new source. Unknown or ambiguous names SHALL fail.
Create SHALL retain at least one required repo input and SHALL NOT introduce
an origin selector option or separate repository command group.

Local inputs SHALL register or reuse origins; URLs SHALL provision or reuse
origins through Git remote configuration. Both workspace modes SHALL use existing layout, pool,
revision, source-preservation, open, and JSON rules after input resolution.
All inputs SHALL be parsed and predictable local errors rejected before cloning.
Duplicate identities SHALL fail before workspace creation. Successfully
published origins SHALL survive later failure, while partial workspace setup
SHALL follow existing rollback behavior.

#### Scenario: Create Directly from a Remote URL

- **WHEN** either create mode receives an unknown remote URL through `--repo`
- **THEN** Trees clones a source and creates or allocates the workspace from it

#### Scenario: Mix a Local Checkout and a Remote

- **WHEN** create receives a local checkout and a distinct remote URL
- **THEN** both sources participate in one workspace and use the same origin record model

#### Scenario: Resolve a Directory Name

- **WHEN** `api` is not an existing relative path and exactly one registered source has base name `api`
- **THEN** `--repo` `api` selects that source identity

#### Scenario: Reject an Ambiguous Directory Name

- **WHEN** a bare name matches multiple registered sources and no local path takes precedence
- **THEN** create fails and identifies paths the caller can use explicitly

#### Scenario: Preserve Explicit Path Interpretation

- **WHEN** the caller prefixes a colon-bearing local path with `./`
- **THEN** create treats it as a path and never invokes remote provisioning for it

#### Scenario: Reject Duplicate Resolved Origins

- **WHEN** path, URL, or directory-name inputs resolve to the same Git common directory
- **THEN** create fails before workspace mutation

#### Scenario: Respect Offline Mode

- **WHEN** `--offline` receives an unknown URL requiring a clone
- **THEN** create fails before any cloning or workspace mutation
- **AND** a known valid origin remains usable offline at its primary local HEAD

#### Scenario: Preserve Existing Path-Based Behavior

- **WHEN** create uses only local paths
- **THEN** existing manual and automatic workspace selection, default fetch, offline, layout, and opening behavior remain unchanged

#### Scenario: Reject an Unknown Name

- **WHEN** a bare NAME is neither an existing local path nor a registered source base name
- **THEN** create reports no matching repository without attempting a clone

#### Scenario: Look up an Existing Source by Name

- **WHEN** NAME uniquely matches an existing source
- **THEN** create reuses its stored identity and does not add repository mode metadata
