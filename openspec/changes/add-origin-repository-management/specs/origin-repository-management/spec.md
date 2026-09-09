## Purpose

Manage automatically provisioned and manually registered source repositories
through existing workspace commands while preserving repository identity and
explicit ownership independently of directory placement.

## ADDED Requirements

### Requirement: Preserve Explicit Repository Management Modes

New origins registered from local paths SHALL be manual. Origins cloned by
Trees from remote inputs SHALL be automatic. Existing records SHALL migrate as
manual regardless of their location. Registration through an existing origin's
primary or linked path SHALL preserve its ID and mode. Neither directory
containment nor configuration changes SHALL convert management modes. Either
origin mode SHALL support either workspace mode.

#### Scenario: Register a Manual Repository Inside the Managed Root

- **WHEN** create receives a previously unknown local checkout inside `origins-dir`
- **THEN** its origin is manual and its files remain user-owned

#### Scenario: Resolve an Existing Automatic Origin by Path

- **WHEN** create receives a path to an automatic source or a linked worktree
- **THEN** it retains the existing ID, automatic mode, managed root, and URL metadata

### Requirement: Derive Names from Source Directories

Origins SHALL use primary source directory base names as their names without
stored aliases or global name uniqueness. Duplicate base names SHALL remain
valid origin records. No dedicated repo command group or alias mutation
interface SHALL be introduced.

#### Scenario: Keep Same-Named Repositories Distinct

- **WHEN** two origins have source paths ending in `api`
- **THEN** both retain their distinct origin IDs and paths without requiring a rename

### Requirement: Configure Automatic Origin Placement

`trees config set origins-dir <PATH>` SHALL persist `repository.origins_dir`,
with a default of `trees/origins` below the platform data base. Relative paths
SHALL resolve against the configuration file directory. Changing configuration
SHALL preserve unrelated settings and affect only future allocations. Each
automatic origin SHALL retain its allocation root. Manual registration SHALL
NOT create, move into, or claim ownership of the configured directory.

#### Scenario: Change the Root for New Clones

- **WHEN** the root changes and create receives a previously unseen URL
- **THEN** the clone is allocated under the new root and existing origins remain in place

### Requirement: Provision or Reuse an Automatic Origin by URL

A remote create input SHALL reuse a known automatic origin for the exact
provisioning URL after validating its stored identity, or clone a new source
under the configured root. URL lookup SHALL be independent of current root
configuration and SHALL NOT adopt manual repositories by their remote URL.
Missing or replaced known clones SHALL fail without rebinding their IDs.
Different URL spellings SHALL NOT be assumed equivalent. A new clone SHALL
use a contained exclusive target and SHALL be registered only after Git cloning
and primary HEAD validation succeed. Existing contents SHALL NOT be overwritten.

#### Scenario: Reuse a URL Across Workspace Creations

- **WHEN** create receives the same URL again, including after `origins-dir` changes
- **THEN** it uses the original valid automatic origin and the same repository-set pool identity

#### Scenario: Clone a New Source

- **WHEN** create receives an unknown reachable remote URL
- **THEN** Trees clones and registers an automatic origin before workspace creation
- **AND** a remote base name `api.git` produces a source directory named `api`

#### Scenario: Reject a Broken Retained Clone

- **WHEN** a known URL's source is missing or its common directory differs
- **THEN** create fails with the origin ID and path without replacing the retained identity

### Requirement: Recover Partial Provisioning Without Removing Published Origins

Trees SHALL persist clone intent before filesystem mutation, isolate targets,
and serialize provisioning for the same exact URL. A live concurrent reservation
SHALL produce an in-progress error without a duplicate clone. Failure cleanup
SHALL affect only provably owned partial files within the recorded root.
Interrupted provisioning SHALL be recoverable on subsequent create for that
URL. Uncertain ownership or cleanup failure SHALL retain an actionable operation
record. Published origins SHALL survive subsequent workspace or other input
failures and SHALL be reported for reuse. Partial origins SHALL NOT be available for selection.

#### Scenario: Fail During Clone

- **WHEN** Git fails or the new source has no usable primary HEAD
- **THEN** no registered origin is published and only owned partial clone files are cleaned

#### Scenario: Preserve a Committed Origin

- **WHEN** clone publication succeeded but a later step or acknowledgment fails
- **THEN** its source files and origin record remain available for retry

#### Scenario: Recover After Interruption

- **WHEN** create encounters an abandoned reservation for its URL
- **THEN** it acquires exclusive recovery ownership before cleaning proven partial files and retrying
- **AND** it never takes over a live operation or deletes unrelated paths

### Requirement: Remove Registration for an Origin Without Deleting Source Data

`trees remove <REPO_ID>` SHALL remove registration for either origin mode while preserving
source files, ownership metadata, origin ID, pool membership, worktrees, claims,
and history. Existing workspace references SHALL NOT block registration removal.
An already unregistered origin SHALL be a successful no-op. Subsequent create
by its valid source path or exact provisioning URL SHALL re-register the same
identity. Directory-name lookup SHALL exclude unregistered origins.

#### Scenario: Remove a Referenced Automatic Origin

- **WHEN** remove targets an automatic origin referenced by existing workspaces
- **THEN** it removes registration for metadata without deleting the clone or changing those workspaces
- **AND** existing release and workspace removal remain available under their usual guards

#### Scenario: Re-Register a Retained Origin

- **WHEN** create explicitly uses an unregistered origin's valid path or known URL
- **THEN** it reuses the same ID, mode, and ownership metadata
