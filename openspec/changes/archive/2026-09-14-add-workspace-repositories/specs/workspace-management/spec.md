## ADDED Requirements

### Requirement: Add Repositories to an Existing Workspace

The CLI SHALL accept `trees add [WORKSPACE_DIR] --repo <PATH|URL|NAME>... [--offline] [--json]` with
at least one repeatable `--repo` and the shared optional `--workspace-id WORKSPACE_ID`,
`--workspace-dir WORKSPACE_DIR`, and `--claim-id CLAIM_ID` selectors. All explicit selectors,
including the positional directory, SHALL be mutually exclusive. An explicit directory SHALL select
that exact managed workspace, resolving relative paths against the invocation directory. Without a
target, the command SHALL select the nearest managed workspace containing the current directory. A
workspace ID SHALL select its exact registered workspace. A claim ID SHALL select only its active
allocation. Explicit misses SHALL NOT fall back to current directory, and an ineligible nearest
containing workspace SHALL NOT cause fallback to an outer workspace. Unknown targets and invalid
argument combinations SHALL fail without workspace mutation.

#### Scenario: Add from a Repository Descendant

- **WHEN** `add` runs without a target from a managed repository's subdirectory
- **THEN** it adds the requested repositories to the nearest containing managed workspace

#### Scenario: Select an Explicit Target

- **WHEN** the `add` command receives an explicit workspace ID, workspace path, or active claim ID
- **THEN** it selects exactly that workspace or allocation, independent of the containing workspace

#### Scenario: Reject Invalid Arguments

- **WHEN** the `add` command receives multiple selectors, no repo input, an unknown target, or an inactive claim ID
- **THEN** it fails without modifying workspace membership or Git state

### Requirement: Resolve Repository Inputs Without Duplicates

The `add` command SHALL reuse the create command's local-path, URL, registered-name, source
provisioning, and upstream revision selection rules. Relative paths SHALL be resolved against the
original invocation directory. It SHALL deduplicate canonical Git common-directory identities and
treat valid existing associations as `already_present` without fetching, resetting, or recreating
them. Only new origins SHALL undergo revision selection; `--offline` SHALL skip fetching and forbid
unknown-URL cloning. Existing local paths SHALL retain precedence over names, and ambiguous names or
remote matches SHALL fail. Successfully published origins SHALL survive a later addition failure and
be reported for retry.

#### Scenario: Retry a Completed Addition

- **WHEN** every requested identity already has a valid worktree in the workspace
- **THEN** `add` succeeds with `already_present` results without fetching or changing those worktrees

#### Scenario: Add Mixed Inputs

- **WHEN** a request includes existing identities, repeated aliases, and new repositories
- **THEN** `add` produces one result per unique identity and creates only the new worktrees in detached mode at the create command's selected upstream revisions

#### Scenario: Add Offline

- **WHEN** the `add` command receives `--offline`
- **THEN** new worktrees use primary-worktree local HEAD without fetching, and an unknown URL fails without cloning

#### Scenario: Preserve Resolved Sources After Failure

- **WHEN** source provisioning succeeds but a later input or workspace step fails
- **THEN** published source repositories remain registered and diagnostics identify them while workspace compensation follows the `add` lifecycle contract

### Requirement: Preserve Existing Work During Addition

The `add` command SHALL preserve existing worktree identities, `HEAD` values, branches, index state,
tracked changes, untracked files, and ignored files. Dirty or branch-attached worktrees SHALL NOT
alone reject addition. Missing, prunable, identity-mismatched, failed, or unresolved partial
associations SHALL reject ordinary addition. New child paths SHALL use source directory base names;
collisions among existing and new names, occupied destinations, unsafe containment, and symlink
destinations SHALL fail before workspace mutation without overwrite or automatic renaming.

#### Scenario: Add While Existing Work Is Dirty

- **WHEN** existing worktrees have staged, unstaged, untracked, or ignored content or a user-selected branch or revision
- **THEN** a valid addition preserves that content and Git state and retains truthful observed workspace health

#### Scenario: Reject a Conflicting Destination

- **WHEN** a new repository name conflicts with another repository or an occupied target path
- **THEN** `add` fails before moving or creating workspace worktrees and leaves the conflicting content intact

#### Scenario: Reject a Broken Existing Association

- **WHEN** a requested existing association is missing or identity-mismatched
- **THEN** `add` fails rather than returning `already_present` or recreating it

### Requirement: Promote Root Worktrees When Expanding a Workspace

When a distinct repository is added to a workspace whose only worktree occupies its root, `add` SHALL
relocate that original worktree into the direct child named after its source repository and create
the new repositories as sibling children. Workspace ID, workspace path, original worktree ID, and
active claim SHALL remain unchanged. The `add` command SHALL preserve existing child paths in an
already expanded workspace. Unsafe or unsupported moves SHALL fail without forcing Git locks or
discarding content. The command SHALL report the original and new repository paths and SHALL NOT
promise to update paths for running tools or the parent shell.

#### Scenario: Expand a Single Repository Workspace

- **WHEN** workspace `W` is an `api` worktree and `add` successfully adds `web`
- **THEN** the original worktree becomes `W/api`, the new worktree is `W/web`, and the result reports the relocation while retaining workspace and original worktree identities

#### Scenario: Avoid Promotion for a Repeated Request

- **WHEN** a root-directory structure workspace receives only already-present repository identities
- **THEN** its root worktree path remains unchanged

#### Scenario: Preserve an Existing Multi-Repository Directory Structure

- **WHEN** `add` expands a workspace with existing child worktrees
- **THEN** only new child paths are created and existing worktree paths remain unchanged

### Requirement: Report Addition Results

Successful `add` SHALL report the operation ID, workspace ID and path, nullable active claim ID,
previous pool ID, and current nullable pool ID, per-repository `added` or `already_present` outcomes with
origin and worktree IDs, and paths, and any old/new path mappings. Default output SHALL be
line-oriented text. `--json` SHALL emit one JSON object with `schema_version` equal to 1 and fields
`operation_id`, `workspace_id`, `workspace_path`, `claim_id`, `previous_pool_id`, `pool_id`,
`repositories`, and `relocated`. Diagnostics SHALL use standard error. Success including a no-op
SHALL exit zero; failures SHALL exit nonzero without emitting a success object.

#### Scenario: Report a Pool and Path Change

- **WHEN** automatic single-repository expansion succeeds with `--json`
- **THEN** one JSON object identifies the retained workspace and claim, old/new pools, all requested repository outcomes, and the original worktree old/new path mapping
