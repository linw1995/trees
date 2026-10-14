# Workspace Open Delta

## MODIFIED Requirements

### Requirement: Recover Expired Operations Before Opening

Open SHALL recover expired `acquire`, `claim`, `release`, `gc`, `remove`, and `reconcile`
operations through shared lifecycle recovery without resuming the original
command or changing branches, files, directory structure, or claims. It SHALL
reject expired structural or unknown operations before lease takeover. Recovery
SHALL run outside SQLite transactions and record the interrupted operation as
failed before retiring its lease.

#### Scenario: Recover an Interrupted Release Before Opening

- **WHEN** the workspace has an expired release operation lease
- **THEN** open observes the worktrees, records recovery, and releases the lease
  before starting the program
- **AND** its claim, branch, staged and unstaged changes, untracked files, and
  ignored files remain intact

#### Scenario: Require Structural Recovery Before Opening

- **WHEN** the workspace has an expired create, add, or unknown operation lease
- **THEN** open reports the operation ID and kind and requires recovery through
  its lifecycle command without recovering it or starting the program

#### Scenario: Reject a New Operation After Recovery

- **WHEN** another operation acquires a lease after recovery and before final admission
- **THEN** open rejects the new lease and does not start the program

#### Scenario: Do Not Launch After Failed Recovery

- **WHEN** recovery cannot complete its observations or persist its terminal event
- **THEN** open reports the recovery error and does not start the program

#### Scenario: Recover an Interrupted Reconciliation Before Opening

- **WHEN** the workspace has an expired reconciliation operation lease
- **THEN** open observes the worktrees, records the interrupted check as failed, and releases its lease
- **AND** user files, branches, and claims remain intact
