## MODIFIED Requirements

### Requirement: Persist One Active Workspace Claim

The system SHALL persist at most one active workspace claim for each
workspace. The claim SHALL contain a UUID v7 claim identifier, the workspace
ID, and a claim timestamp. The workspace ID SHALL be unique
in the active claim table. A workspace with no active claim is unclaimed; a
workspace with an active claim is unavailable for another acquisition. The
claim records persistent usage state for the workspace. It is not a database
transaction or a database lock and SHALL remain until the caller releases it.
Release SHALL identify the active claim from an explicit workspace-directory
or claim-identifier target, or from the current directory when neither target
is supplied. The claim identifier SHALL be treated as a local coordination
token rather than a security credential. This capability SHALL NOT infer an
abandoned claim from process liveness or replace it automatically.

Access-boundary reconciliation SHALL require the current operation lease. The
capability SHALL not expose a lease-free access-boundary entry point; general
lease-free reconciliation remains available only for non-access observation
contexts.

#### Scenario: Serialize Concurrent Acquisitions

- **WHEN** two processes attempt automatic create for the same reusable pool
  slot
- **THEN** at most one process receives a successful claim and the other
  receives a busy or transaction-conflict error without a second claim row

#### Scenario: Preserve Workspace Identity Across Reuse

- **WHEN** a workspace is released and later acquired again
- **THEN** its workspace ID, repo-worktree IDs, canonical paths, and Git
  worktree associations remain unchanged while a new claim identifier may be
  issued

### Requirement: Release Without Destroying Git State

The CLI SHALL accept `trees release [<workspace-dir>]` or `trees release
--claim-id <claim-id>`. The positional workspace directory and claim identifier
SHALL be mutually exclusive. An explicit workspace directory MAY be absolute
or relative; release SHALL resolve a relative directory against the process
current directory and select that exact managed workspace. When neither input
is supplied, release SHALL select the nearest managed workspace containing the
canonical current directory. A claim identifier SHALL select its active claim
and associated workspace. Release SHALL snapshot the active claim selected by
a workspace directory or current directory, reconcile the workspace while
retaining that claim, and release it only when all managed worktrees satisfy
the reusable snapshot requirement. A successful release SHALL leave the
workspace directory, worktree files, source repositories, and worktree
associations unchanged.

#### Scenario: Release a Reusable Workspace

- **WHEN** one release target resolves an active claim and all managed
  worktrees pass the final reconciliation
- **THEN** the selected active claim is removed atomically, a release operation
  and immutable release event are recorded, and the workspace can be acquired
  again

#### Scenario: Release from a Workspace Descendant

- **WHEN** `trees release` runs without a target from a directory below a
  managed workspace
- **THEN** release selects the nearest containing managed workspace and its
  active claim

#### Scenario: Release a Relative Workspace Directory

- **WHEN** `trees release <workspace-dir>` receives a relative directory
- **THEN** release resolves it against the process current directory and
  selects that exact managed workspace

#### Scenario: Reject Invalid Target Combinations

- **WHEN** release receives both a workspace directory and claim-identifier
  target
- **THEN** argument parsing fails before any workspace state changes

#### Scenario: Reject an Unknown Claim Identifier

- **WHEN** the selected claim identifier is absent
- **THEN** release fails without releasing another claim or changing Git state

#### Scenario: Reject an Unknown Workspace Target

- **WHEN** the selected workspace is unclaimed or the selected path does not
  identify a managed workspace
- **THEN** release fails without releasing another claim or changing Git state

#### Scenario: Exit on Concurrent Release

- **WHEN** release cannot immediately acquire operation admission for the
  selected workspace
- **THEN** release returns a busy failure without waiting, retrying, or changing
  Git or claim state
