## ADDED Requirements

### Requirement: Check Shared Workspace Content Before Reuse

A shared workspace root SHALL contain only persisted managed worktree paths
before release or automatic reuse. Extra files, directories, or symlinks and
unreadable roots SHALL prevent reuse. Reconciliation SHALL mark an otherwise
attached workspace `degraded` when this check fails and restore `ready` after
content removal. A root that is itself a managed worktree SHALL continue to use
the existing Git cleanliness predicate.

#### Scenario: Observe Unexpected Shared Content

- **WHEN** otherwise attached worktrees share a root containing extra files, directories, or symlinks, including `public/` or an empty directory
- **THEN** reconciliation marks the workspace degraded while keeping its clean worktrees attached

#### Scenario: Restore Eligibility After Reconciliation

- **WHEN** shared content is removed and reconciliation observes otherwise reusable managed worktrees
- **THEN** the workspace becomes ready and can be acquired again

#### Scenario: Allow a Managed Worktree Named Public

- **WHEN** the workspace root contains only persisted managed worktree paths, including a worktree named `public`
- **THEN** its name does not prevent release or automatic reuse

#### Scenario: Reject a Dangling Shared Symlink

- **WHEN** an unexpected root entry is a symlink whose target does not exist
- **THEN** root content validation rejects that entry without following its target

#### Scenario: Reject an Unreadable Root

- **WHEN** root content validation cannot read the shared workspace directory
- **THEN** the workspace does not satisfy the reusable snapshot predicate

### Requirement: Preserve Shared Content During Workspace Access

Automatic allocation SHALL skip slots with unexpected shared root content and
preserve their contents. Release SHALL reject such content before fetching or
aligning worktrees, record the rejection, and retain the active claim.

#### Scenario: Reject Release with Public Content

- **WHEN** a claimed multi-repository workspace contains shared `public/` content outside the managed worktrees
- **THEN** release fails, preserves that content and the active claim, and leaves every worktree HEAD unchanged

#### Scenario: Skip a Slot with Shared Content

- **WHEN** automatic allocation observes unexpected root content in an idle ready slot
- **THEN** it does not claim the affected slot and provisions another slot if none is reusable
