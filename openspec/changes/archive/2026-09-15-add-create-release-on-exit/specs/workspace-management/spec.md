## MODIFIED Requirements

### Requirement: Open a Program in a Created Workspace

The CLI SHALL accept `--open[=<PROGRAM>]` for manual and automatic create.
When the option has no explicit program, Trees SHALL resolve the program from a
nonempty `$SHELL`. Trees SHALL reject a missing or empty default, an explicitly
empty program, or the combination of `--open` and `--json` before database,
Git, or filesystem mutation. After successful creation or allocation, Trees
SHALL start the selected program with the canonical workspace path as its
current directory and with inherited standard streams and environment. Unless
`--release-on-exit` is present, the program SHALL replace the Trees
process where process replacement is supported, and an automatic workspace
SHALL retain its active claim after the opened program exits. With
`--release-on-exit`, Trees SHALL supervise the program and follow the automatic
release and recovery requirements in workspace-reuse.

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

- **WHEN** a program opened after automatic allocation exits without `--release-on-exit`
- **THEN** the allocated workspace remains claimed until an explicit release succeeds

#### Scenario: Reject an Unavailable Default Shell

- **WHEN** create receives `--open` while `$SHELL` is unset or empty
- **THEN** create fails before any workspace state changes

#### Scenario: Reject Conflicting Create Modes

- **WHEN** create receives both `--open` and `--json`
- **THEN** argument parsing fails before any workspace state changes

## ADDED Requirements

### Requirement: Opt into Release After Program Exit

The CLI SHALL accept `trees create --repo <repository>... --open[=<PROGRAM>]
--release-on-exit`. The flag SHALL require `--open`, SHALL conflict with a
positional workspace path and `--json`, and SHALL reject invalid combinations
before database, Git, or filesystem mutation. Both explicit programs and the
default shell SHALL support this mode. Trees SHALL execute the program directly
without shell parsing, wait for its termination, and retain no database
connection or transaction while waiting for a child process.

#### Scenario: Supervise an Explicit Program

- **WHEN** automatic create receives `--open=program --release-on-exit`
- **THEN** it waits for that program in the allocated workspace before attempting to release

#### Scenario: Supervise the Default Shell

- **WHEN** automatic create receives `--open --release-on-exit` with a valid `$SHELL`
- **THEN** it waits for that shell to terminate before attempting to release

#### Scenario: Reject Invalid Session Arguments

- **WHEN** `--release-on-exit` is used without `--open`, with a positional workspace path, or with `--json`
- **THEN** argument validation fails before mutation

#### Scenario: Preserve Existing Open Behavior

- **WHEN** create omits `--release-on-exit` or the caller uses standalone `trees open`
- **THEN** existing program launch and claim retention behavior is unchanged
