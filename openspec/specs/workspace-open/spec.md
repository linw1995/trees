# Workspace Open Specification

## Purpose

This capability defines how callers safely launch a program in an existing
managed workspace selected by stable ID without bypassing workspace ownership
or operation boundaries.

## Requirements

### Requirement: Open a Managed Workspace Using an Identifier

The CLI SHALL provide `trees open <workspace-id> [--program=<PROGRAM>]`.
`workspace-id` SHALL be a valid workspace UUID v7. The command SHALL resolve
the persisted canonical workspace path through a read-only lifecycle database
connection and SHALL close that connection before starting the program.

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

Open SHALL reject an unknown or removed workspace. It SHALL reject a
workspace with a retained operation lease. An automatic workspace SHALL require
an active claim; a manual workspace SHALL not require one. These checks SHALL
come from one consistent read-only database transaction. Open SHALL NOT create
or release a claim. It SHALL NOT start or recover an operation, reconcile Git
state, inspect repo-worktree contents, or append a lifecycle event.

#### Scenario: Open a Claimed Automatic Workspace

- **WHEN** an automatic workspace has an active claim and no operation lease
- **THEN** open resolves its canonical path and starts the selected program

#### Scenario: Reject an Unclaimed Automatic Workspace

- **WHEN** an automatic workspace has no active claim
- **THEN** open fails without claiming or modifying that workspace

#### Scenario: Open a Manual Workspace

- **WHEN** a non-removed manual workspace has no operation lease
- **THEN** open starts the selected program without requiring a claim

#### Scenario: Reject an Active or Interrupted Mutation

- **WHEN** the workspace has any retained operation lease
- **THEN** open fails without recovering or taking over the operation
