## ADDED Requirements

### Requirement: Open a Program in a Created Workspace

The CLI SHALL accept `--open[=<PROGRAM>]` for manual and automatic create.
When the option has no explicit program, Trees SHALL resolve the program from a
nonempty `$SHELL`. Trees SHALL reject a missing or empty default, an explicitly
empty program, or the combination of `--open` and `--json` before database,
Git, or filesystem mutation. After successful creation or allocation, Trees
SHALL start the selected program with the canonical workspace path as its
current directory and with inherited standard streams and environment. Where
process replacement is supported, the program SHALL replace the Trees process.
An automatic workspace SHALL retain its active claim after the opened program
exits.

#### Scenario: Open the Default Shell

- **WHEN** create receives `--open` and `$SHELL` identifies a program
- **THEN** Trees opens that program with the created or allocated workspace as
  its current directory

#### Scenario: Open an Explicit Program

- **WHEN** create receives `--open=<PROGRAM>`
- **THEN** Trees opens that executable directly without parsing its value as shell syntax

#### Scenario: Preserve the Parent Shell Directory

- **WHEN** the opened program exits and control returns to the shell that invoked Trees
- **THEN** that parent shell remains in its original directory

#### Scenario: Retain an Automatic Claim

- **WHEN** a program opened after automatic allocation exits
- **THEN** the allocated workspace remains claimed until an explicit release succeeds

#### Scenario: Reject an Unavailable Default Shell

- **WHEN** create receives `--open` while `$SHELL` is unset or empty
- **THEN** create fails before any workspace state changes

#### Scenario: Reject Conflicting Create Modes

- **WHEN** create receives both `--open` and `--json`
- **THEN** argument parsing fails before any workspace state changes
