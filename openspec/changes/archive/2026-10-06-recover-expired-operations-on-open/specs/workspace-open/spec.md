## MODIFIED Requirements

### Requirement: Open a Managed Workspace Using an Identifier

The CLI SHALL provide `trees open <workspace-id> [--program=<PROGRAM>]` and
require exactly one explicit selector. Positional workspace IDs SHALL be UUID v7.
Open SHALL resolve the canonical path using existing lifecycle storage without
creating or migrating storage, then close its database connection before
starting the selected program in that directory.

#### Scenario: Open a Workspace in the Default Shell

- **WHEN** open receives a valid workspace ID and a nonempty `$SHELL`
- **THEN** it starts that shell with inherited environment and standard streams
- **AND** a missing or empty `$SHELL` fails before storage access

#### Scenario: Open an Explicit Program

- **WHEN** open receives a nonempty `--program=<PROGRAM>`
- **THEN** it executes that program directly without shell parsing, with
  inherited environment and standard streams
- **AND** an explicitly empty program fails before storage access
- **AND** the program replaces the Trees process where supported

#### Scenario: Reject an Invalid Workspace Identifier

- **WHEN** open receives a malformed or non-v7 workspace ID
- **THEN** it fails before opening lifecycle storage or starting a program

#### Scenario: Require One Explicit Selector

- **WHEN** open receives no selector or multiple selectors
- **THEN** it fails before storage access or program launch
- **AND** `--workspace-id`, `--workspace-dir`, and `--claim-id` are alternatives
  to the positional ID

#### Scenario: Open by Path or Claim

- **WHEN** one explicit path selects an exact registered root or a claim
  selects its associated workspace
- **THEN** open resolves that workspace and applies every eligibility check
- **AND** an explicit missing target fails without falling back to the current directory

### Requirement: Preserve Workspace Ownership Boundaries

Open SHALL reject unknown or removed workspaces and unexpired operation leases.
Manual and automatic workspaces SHALL be openable regardless of claim status.
Open SHALL NOT create, replace, or release a claim. Selection and final admission
SHALL use short consistent transactions. After recovery, open SHALL recheck the
selected workspace ID for removal and any retained lease. Without a retained
lease, open SHALL NOT trigger recovery, reconciliation, or lifecycle events.

#### Scenario: Open a Claimed Automatic Workspace

- **WHEN** a non-removed automatic workspace has an active claim and no operation lease
- **THEN** open starts the selected program without changing its claim

#### Scenario: Open an Unclaimed Automatic Workspace

- **WHEN** a non-removed automatic workspace has no claim and no operation lease
- **THEN** open starts the selected program without creating a claim

#### Scenario: Reject an Unclaimed Automatic Workspace

- **WHEN** an unclaimed automatic workspace has an unexpired operation lease
- **THEN** open fails because of that lease without creating a claim

#### Scenario: Open a Manual Workspace

- **WHEN** a non-removed manual workspace has no operation lease
- **THEN** open starts the selected program without requiring a claim

#### Scenario: Reject an Active or Interrupted Mutation

- **WHEN** the workspace has an unexpired lease or an expired structural operation
- **THEN** open fails without recovering or taking over the operation

## ADDED Requirements

### Requirement: Recover Expired Operations Before Opening

Open SHALL recover expired `acquire`, `claim`, `release`, `gc`, and `remove`
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
