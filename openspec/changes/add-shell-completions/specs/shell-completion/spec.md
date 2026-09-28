# Spec Delta

## Purpose

Provide Bash and `zsh` users with command-aware suggestions for Trees arguments, including persisted identifiers, while keeping completion independent of workspace operations.

## ADDED Requirements

### Requirement: Complete CLI Grammar

Trees SHALL provide Bash and `zsh` completion registrations that can be sourced. Once registered, completion SHALL suggest subcommands, valid options for the selected command, and finite values such as `status --view` and `config set` settings from the current CLI grammar. Path-valued workspace selectors SHALL retain filesystem path completion. Completion SHALL NOT execute the selected command.

#### Scenario: Complete Commands and Options

- **WHEN** a user requests completion after `trees` and a space, or after a partial option in a supported shell
- **THEN** suggestions reflect the subcommands and options accepted by the installed Trees binary

#### Scenario: Complete a Finite Value

- **WHEN** a user requests completion after `trees status --view` and a space
- **THEN** the available status view values are suggested

#### Scenario: Complete a Workspace Directory

- **WHEN** a user requests completion for a workspace directory argument
- **THEN** matching filesystem directories remain available as suggestions

### Requirement: Suggest Persisted Selectors by Command

Completion SHALL cover every current argument position that accepts a persisted Trees ID:

| Command | Argument positions | Candidate scope |
| --- | --- | --- |
| `add` | `--workspace-id`, `--claim-id` | Non-removed workspaces; active claims |
| `claim` | `--workspace-id` | Non-removed workspaces |
| `release` | `--workspace-id`, `--claim-id` | Non-removed workspaces; active claims |
| `status` | positional `WORKSPACE_ID`, `--workspace-id`, `--claim-id` | All workspaces including removed records; active claims |
| `open` | positional `ID`, `--workspace-id`, `--claim-id` | Non-removed workspaces and registered origins for positional `ID`; non-removed workspaces for `--workspace-id`; active claims |
| `remove` | positional `WORKSPACE_OR_REPO_ID` | Non-removed workspaces and registered origins |

Future CLI arguments that accept persisted Trees IDs SHALL receive completion as part of their introduction. A candidate SHALL be offered only when it matches the typed prefix. Suggestions SHALL be sorted and deduplicated. Completion SHALL NOT promise that a candidate passes the command's later eligibility checks.

#### Scenario: Complete Workspace Identifiers

- **WHEN** a user requests completion after `trees release --workspace-id` or `trees status` and a space
- **THEN** matching workspace IDs are suggested in the corresponding argument position

#### Scenario: Cover All Identifier Arguments

- **WHEN** a supported command accepts a positional or named persisted ID argument in the table above
- **THEN** completion offers candidates from that argument's stated scope
- **AND** a command option group shared by multiple commands does not cause one command's candidate policy to be applied to another

#### Scenario: Complete Positional Workspace and Origin Identifiers

- **WHEN** a user requests completion after `trees open` or `trees remove` and a space
- **THEN** matching non-removed workspace and registered origin repository IDs are suggested
- **AND** an ID present in both tables is not suggested as a positional candidate because the command would reject it as ambiguous

#### Scenario: Inspect a Removed Workspace

- **WHEN** a removed workspace ID matches a `trees status` positional or `--workspace-id` prefix
- **THEN** that ID is suggested regardless of the `--all` option

#### Scenario: Complete Claim Identifiers

- **WHEN** a user requests completion for a supported `--claim-id` argument
- **THEN** matching active claim IDs are suggested

### Requirement: Complete Repository Names and Paths

For `create --repo` and `add --repo`, completion SHALL suggest registered source directory base names that uniquely identify one origin and SHALL retain local path completion. It SHALL NOT offer an ambiguous registered name as an origin-name suggestion. Candidates containing control characters SHALL be omitted when the shell completion protocol cannot represent them safely. Completion SHALL leave URL entry available without fetching or looking up remotes.

#### Scenario: Complete a Registered Name

- **WHEN** one registered source has base name `api` and the user completes `--repo ap`
- **THEN** `api` is suggested

#### Scenario: Avoid an Ambiguous Name

- **WHEN** multiple registered sources have the same base name
- **THEN** that base name is not suggested as a registered-name candidate
- **AND** local path suggestions remain available

#### Scenario: Omit Unsafe Control Characters

- **WHEN** a source directory base name contains a control character used by the shell completion protocol
- **THEN** it is not suggested as a registered-name candidate

### Requirement: Keep Completion Read-Only and Quiet

Generating shell registrations SHALL require no lifecycle database. Dynamic candidates SHALL use read-only storage access and SHALL NOT create state directories, run migrations, invoke Git or session hooks, recover operations, or mutate workspaces, claims, origins, or files. Missing, unavailable, or incompatible storage SHALL yield no persisted candidates without completion diagnostics on standard output or standard error. Normal command diagnostics and behavior SHALL remain unchanged.

#### Scenario: Complete Without an Installation Database

- **WHEN** a user requests a dynamic candidate before the lifecycle database exists
- **THEN** completion returns no persisted candidate and creates no state file or directory

#### Scenario: Ignore a Storage Failure During Completion

- **WHEN** read-only storage cannot be opened or queried during completion
- **THEN** completion remains quiet and offers no persisted candidates
- **AND** an ordinary Trees invocation still reports its normal error

### Requirement: Distribute Shell Registration

Trees SHALL document Bash and `zsh` configuration commands that load completion from the installed binary. The Nix package SHALL install shell-discoverable Bash and `zsh` registrations. Release archives SHALL include Bash and `zsh` registrations that can be sourced. Packaged registrations SHALL obtain completion behavior from the current installed binary so that stale completion protocols are not retained across binary upgrades.

#### Scenario: Enable Completion from a Local Installation

- **WHEN** a user follows the documented Bash or `zsh` setup after installing Trees
- **THEN** completion is registered for `trees` in that shell session

#### Scenario: Install a Nix Package

- **WHEN** Trees is installed through Nix with standard Bash or `zsh` completion discovery enabled
- **THEN** the corresponding registration is available without copying a file from the source repository

#### Scenario: Extract a Release Archive

- **WHEN** a user extracts a release archive
- **THEN** it contains Bash and `zsh` registrations that can be sourced alongside the binary
