# Spec Delta

## ADDED Requirements

### Requirement: Claim a Selected Automatic Workspace

The CLI SHALL provide `trees claim` for an explicitly selected existing automatic workspace. It
SHALL create a fresh persistent claim only when the target is unclaimed, has no retained operation
lease or unresolved mutation state, and passes structural validation. The root and every managed
worktree SHALL be present; source and worktree identities and canonical paths SHALL match their
persisted associations; worktrees SHALL be registered and non-prunable; the complete nonempty
repository membership SHALL match the existing pool. Manual, removed, creating, failed, incomplete,
or structurally damaged workspaces SHALL be rejected.

Staged, unstaged, untracked, and ignored content, an attached branch, or a changed HEAD SHALL NOT
alone prevent explicit claiming. A degraded snapshot caused only by these observations SHALL NOT
alone prevent claiming. Claim SHALL preserve files, index contents, branches, `HEAD` values,
worktree associations, and repository membership. It SHALL NOT fetch, checkout, reset, clean,
allocate another workspace, or convert management mode. Observed health SHALL remain independent of
claim status.

#### Scenario: Reserve One Selected Workspace

- **WHEN** the selected automatic workspace is structurally valid and unclaimed
- **THEN** claim creates a fresh active claim for that exact workspace
- **AND** ordinary status shows it as claimed, automatic allocation skips it, and cleanup observes existing active-claim protections

#### Scenario: Preserve Existing Work

- **WHEN** the selected workspace has dirty files, an attached branch, or a changed detached revision but its structure is valid
- **THEN** explicit claim succeeds without changing those files, index contents, branches, or revisions
- **AND** it does not force the recorded health to ready

#### Scenario: Reject Structural Divergence

- **WHEN** any managed worktree is missing, prunable, unregistered, associated with a different repository, or inconsistent with the pool membership
- **THEN** claim fails without granting a claim, repairing files, or choosing a different workspace

#### Scenario: Reject an Ineligible Workspace

- **WHEN** the selected workspace is manual, removed, creating, failed, or structurally incomplete
- **THEN** claim rejects it without changing its management mode or creating a replacement

#### Scenario: Reject an Existing Claim

- **WHEN** a workspace already has an active claim, including one created by a previous invocation of claim
- **THEN** claim reports already claimed without returning success, replacing the identifier, or refreshing the timestamp

#### Scenario: Release Through the Existing Workflow

- **WHEN** the user releases a workspace acquired by explicit claim
- **THEN** existing release admission, safety checks, and alignment behavior apply
- **AND** dirty work retains the claim on rejection while a successful release returns the workspace to its pool

### Requirement: Report Explicit Claim Identity

A successful explicit claim SHALL report `workspace_id`, `workspace_path`, `pool_id`, and `claim_id`. Default output SHALL use Bash-safe assignments named `WORKSPACE_ID`, `WORKSPACE_PATH`, `POOL_ID`, and `CLAIM_ID`. With `--json`, standard output SHALL contain one JSON object with the corresponding lowercase field names. Diagnostics SHALL go to standard error. Success output SHALL occur only after durable publication. Existing create output SHALL remain unchanged.

#### Scenario: Consume Shell Output

- **WHEN** a successful claim returns a path containing spaces or shell special characters
- **THEN** each emitted assignment preserves its exact literal value under Bash evaluation

#### Scenario: Consume JSON Output

- **WHEN** `trees claim --json` succeeds
- **THEN** standard output contains one valid JSON object with all four identity fields and no shell assignments

## MODIFIED Requirements

### Requirement: Require a Reusable Worktree Snapshot

A managed repo worktree SHALL satisfy all these conditions to be reusable. Its
source repository identity and canonical path SHALL match the persisted
association. The worktree SHALL be present, not prunable, and detached. Its
`HEAD` SHALL match `last_head`. The command `git status
--porcelain=v1 --untracked-files=all` SHALL report no staged, unstaged, or
untracked changes. Ignored files SHALL NOT make a worktree dirty. Automatic pool acquisition SHALL
reconcile this predicate against Git's authoritative metadata before returning
success. Release SHALL establish this predicate through the release alignment
requirement before making the workspace available. Explicit in-place claim SHALL use the structural eligibility requirement below instead of requiring a clean, detached reusable snapshot.

#### Scenario: Reject a Dirty Worktree

- **WHEN** automatic pool acquisition observes a managed worktree with staged, unstaged, or untracked changes
- **THEN** reconciliation records the worktree as `dirty`, marks the workspace
  as `degraded`, and automatic pool acquisition does not issue a claim

#### Scenario: Reject a Changed Detached Revision

- **WHEN** automatic pool acquisition observes a detached managed worktree whose `HEAD`
  differs from `last_head`
- **THEN** reconciliation records the worktree as `diverged`, marks the
  workspace as `degraded`, and no new workspace claim is issued

#### Scenario: Leave External Changes for the Current Claim Holder

- **WHEN** release finds a dirty, missing, prunable, identity-mismatched, or
  failed worktree
- **THEN** release records a rejection, retains the current workspace claim,
  and does not align any managed worktree
