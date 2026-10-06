## MODIFIED Requirements

### Requirement: Open a Managed Workspace Using an Identifier

The CLI SHALL provide `trees open <workspace-id> [--program=<PROGRAM>]`.
`workspace-id` SHALL be a valid workspace UUID v7. The command SHALL resolve
the persisted canonical workspace path using lifecycle storage. It SHALL NOT
create or migrate storage and SHALL close its database connection before
starting the program.

Without `--program`, open SHALL resolve a nonempty `$SHELL`. With
`--program=<PROGRAM>`, it SHALL execute that program directly without shell
parsing. A missing or empty default and an explicitly empty program SHALL fail
before process handoff. The selected program SHALL inherit the environment and
standard streams and use the canonical workspace path as its current directory.
Where supported, the program SHALL replace the Trees process.

#### Scenario: Open a Workspace in the Default Shell

- **WHEN** open receives a valid workspace ID and `$SHELL` identifies a program
- **THEN** that program starts with the persisted canonical workspace path as
  its current directory

#### Scenario: Open an Explicit Program

- **WHEN** open receives `--program=<PROGRAM>`
- **THEN** it executes that program directly with inherited process context

#### Scenario: Reject an Invalid Workspace Identifier

- **WHEN** open receives a malformed or non-v7 workspace ID
- **THEN** it fails before opening lifecycle storage or starting a program

Open SHALL also accept `--workspace-id`, `--workspace-dir`, or `--claim-id` as
alternatives to the positional ID. Exactly one explicit selector SHALL be
required. A path SHALL select an exact root and a claim SHALL select its
associated workspace. All existing ownership checks SHALL apply to every form.

#### Scenario: Require One Explicit Selector

- **WHEN** open receives no selector or multiple selectors
- **THEN** it fails before storage access or program launch

#### Scenario: Open by Path or Claim

- **WHEN** one explicit path or claim identifies an eligible workspace
- **THEN** open launches the selected program in that workspace

### Requirement: Preserve Workspace Ownership Boundaries

Open SHALL reject an unknown or removed workspace and a workspace with an
unexpired operation lease. Automatic and manual workspaces SHALL be openable
regardless of claim status. Open SHALL NOT create, replace, or release a claim.

When a retained lease has expired, open SHALL use the shared recovery entry
point with a policy that preserves worktrees. Only `acquire`, `claim`, `release`,
`gc`, and `remove` operations SHALL be eligible for this policy. Recovery SHALL
observe existing worktrees, record the interrupted operation as failed, and
release its lease without resuming the original command or changing branches,
files, or directory structure. Expired `create`, `add`, and unknown operation
kinds SHALL be rejected before taking over the lease, with a diagnostic naming
the operation and requiring recovery through its lifecycle command.

Initial selection and final admission checks SHALL use short consistent
transactions. Git observations and recovery SHALL run outside those
transactions. After recovery, open SHALL recheck the selected workspace ID for
removal and any retained operation lease before handoff. Workspaces without a
retained lease SHALL NOT trigger recovery, reconciliation, or lifecycle events.

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

#### Scenario: Recover an Interrupted Release Before Opening

- **WHEN** the workspace has an expired release operation lease
- **THEN** open records recovery and releases the lease before starting the program
- **AND** its claim, branch, staged and unstaged changes, untracked files, and
  ignored files remain intact

#### Scenario: Require Structural Recovery Before Opening

- **WHEN** the workspace has an expired create or add operation lease
- **THEN** open reports the operation ID and kind without recovering it or
  starting the program

#### Scenario: Reject a New Operation After Recovery

- **WHEN** another operation acquires a lease after recovery and before final admission
- **THEN** open rejects the new lease and does not start the program

#### Scenario: Do Not Launch After Failed Recovery

- **WHEN** recovery cannot complete its observations or persist its terminal event
- **THEN** open reports the recovery error and does not start the program
