## MODIFIED Requirements

### Requirement: Require a Reusable Worktree Snapshot

A managed repo worktree SHALL satisfy all these conditions to be reusable. Its
source repository identity and canonical path SHALL match the persisted
association. The worktree SHALL be present, not prunable, and detached. Its
`HEAD` SHALL match `last_head`. The command `git status
--porcelain=v1 --untracked-files=all` SHALL report no staged, unstaged, or
untracked changes. Ignored files SHALL NOT make a worktree dirty. Acquire SHALL
reconcile this predicate against Git's authoritative metadata before returning
success. Release SHALL establish this predicate through the release alignment
requirement before making the workspace available.

#### Scenario: Reject a Dirty Worktree

- **WHEN** a managed worktree contains staged, unstaged, or untracked changes
- **THEN** reconciliation records the worktree as `dirty`, marks the workspace
  as `degraded`, and acquire does not issue a claim

#### Scenario: Reject a Changed Detached Revision

- **WHEN** acquisition observes a detached managed worktree whose `HEAD`
  differs from `last_head`
- **THEN** reconciliation records the worktree as `diverged`, marks the
  workspace as `degraded`, and no new workspace claim is issued

#### Scenario: Leave External Changes for the Current Claim Holder

- **WHEN** release finds a dirty, missing, prunable, identity-mismatched, or
  failed worktree
- **THEN** release records a rejection, retains the current workspace claim,
  and does not align any managed worktree

### Requirement: Release Without Destroying Git State

The CLI SHALL accept `trees release [<workspace-dir>]` or `trees release
--claim-id <claim-id>`. The positional workspace directory and claim identifier
SHALL be mutually exclusive. An explicit workspace directory MAY be absolute
or relative; release SHALL resolve a relative directory against the process
current directory and select that exact managed workspace. When neither input
is supplied, release SHALL select the nearest managed workspace containing the
canonical current directory. A claim identifier SHALL select its active claim
and associated workspace.

Release SHALL snapshot the active claim and reconcile the workspace while
retaining that claim. Before changing any worktree, release SHALL verify that
every managed worktree is present, non-prunable, identity-matched, and reports
no staged, unstaged, or untracked changes. If every worktree passes preflight,
release SHALL align each worktree to its persisted origin repository's current
local `HEAD` in detached mode, update the recorded worktree head, and perform a
final reconciliation. Release SHALL remove the claim only after every aligned
worktree satisfies the reusable snapshot requirement. Release SHALL NOT fetch
remotes, delete branches or commits, or remove ignored files.

#### Scenario: Align a Clean Changed Worktree

- **WHEN** a claimed worktree is clean but branch-attached or at a revision
  different from its origin repository's current `HEAD`
- **THEN** release checks out the origin repository `HEAD` in detached mode,
  records the aligned head, and releases the claim after final reconciliation

#### Scenario: Release a Reusable Workspace

- **WHEN** one release target resolves an active claim and every managed
  worktree passes preflight and final reconciliation
- **THEN** the selected active claim is removed atomically, a release operation
  and immutable release event are recorded, and the workspace can be acquired
  again

#### Scenario: Reject Dirty Release Before Alignment

- **WHEN** any managed worktree has staged, unstaged, or untracked changes
- **THEN** release retains the active claim and does not align any managed
  worktree

#### Scenario: Retain the Claim After Alignment Failure

- **WHEN** Git alignment, persistence, or final reconciliation fails
- **THEN** release records the failure, retains the active claim, and does not
  report a successful release

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
