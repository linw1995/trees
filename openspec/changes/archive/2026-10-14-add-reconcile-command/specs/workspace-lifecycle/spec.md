# Workspace Lifecycle Delta

## ADDED Requirements

### Requirement: Explicit Workspace Reconciliation

The CLI SHALL provide `trees reconcile [WORKSPACE_ID] [--json]` with mutually
exclusive `--workspace-id`, `--workspace-dir`, and `--claim-id` selectors. With
no selector, it SHALL check the nearest registered ancestor of the invocation
directory. It SHALL fail for missing targets and removed workspaces.

#### Scenario: Select a Workspace

- **WHEN** a caller provides one explicit ID, root directory, or active claim selector
- **THEN** reconciliation checks that workspace regardless of the invocation directory
- **AND** multiple selectors fail before storage access

### Requirement: Refresh Workspace Observations

Reconciliation SHALL refresh persisted workspace and worktree observations using
the existing Git and workspace-content checks. It SHALL preserve files, branches,
revisions, and claims. Repeating a check without new divergence SHALL NOT
duplicate external observation events. A completed degraded observation SHALL
retain successful command exit status.

#### Scenario: Observe an External Change

- **GIVEN** a ready workspace with an externally modified worktree
- **WHEN** the caller runs `trees reconcile` inside that workspace
- **THEN** its persisted worktree state becomes dirty and workspace state becomes degraded
- **AND** the result reports the workspace ID, path, state, and changed worktree count
- **AND** files and claims remain unchanged

#### Scenario: Restore Readiness

- **GIVEN** a workspace degraded by unexpected shared content
- **WHEN** the caller removes that content and runs reconciliation
- **THEN** otherwise reusable worktrees remain attached and the workspace becomes ready

### Requirement: Reconciliation Operation Admission

Explicit reconciliation SHALL acquire an exclusive operation lease after
validating its selected target again in a short write transaction. It SHALL reject
active operations without observing the workspace. Git observations SHALL run
outside long write transactions and renew the lease. The command SHALL record
terminal operation success or failure.

#### Scenario: Reject an Active Operation

- **WHEN** another lifecycle operation has an active workspace lease
- **THEN** reconciliation fails without observing the workspace or changing that operation

#### Scenario: Reject a Changed Target After Recovery

- **WHEN** the selected workspace changes between initial selection and final admission
- **THEN** reconciliation fails before starting a new check and asks the caller to retry

#### Scenario: Fail an Observation

- **WHEN** a workspace check fails but its terminal event can be persisted
- **THEN** the command records a failed operation, retires its lease, and returns the observation error

#### Scenario: Fail Finalization

- **WHEN** the observation and its failure finalization both fail
- **THEN** the command preserves the primary error chain and reports the finalization error
- **AND** its retained lease remains available for later recovery

#### Scenario: Fail After Successful Observation

- **WHEN** observations succeed but the terminal operation event cannot be persisted
- **THEN** reconciliation reports the persistence error and retains the operation lease for recovery

### Requirement: Recover Interrupted Reconciliation

Reconciliation SHALL use observation-safe recovery for expired `acquire`,
`claim`, `release`, `gc`, `remove`, and `reconcile` operations. Expired structural
or unknown operations SHALL require their lifecycle command before a check can
proceed.

#### Scenario: Interrupted Check

- **GIVEN** an expired reconciliation operation lease
- **WHEN** the caller invokes reconciliation or opens the workspace
- **THEN** observation-safe recovery records the expired operation as failed and releases its lease
- **AND** user files and claims are preserved
- **AND** the new command repeats admission before proceeding
