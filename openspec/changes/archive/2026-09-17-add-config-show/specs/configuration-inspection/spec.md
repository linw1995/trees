## ADDED Requirements

### Requirement: Show Effective Configuration

The CLI SHALL provide `trees config show` without positional arguments and with an
optional `--json` flag. Without `--json`, successful inspection SHALL print
`workspaces_dir`, `origins_dir`, `latest_session_hook_program`,
and `latest_session_hook_timeout_ms`, in that
order, as newline-terminated
Bash variable assignments and exit successfully. Every value SHALL be single-quoted
with embedded single quotes escaped using shell-compatible quote concatenation.
The output SHALL contain no `export` prefix, headings, or progress messages.
Interpreting the output in Bash SHALL preserve displayed path values literally
without evaluating their contents as shell expansions or commands. Neither output
format SHALL include a `config_file` field. Directory values SHALL match existing configuration
resolution used by allocation commands, including defaults and relative paths.
Hook values SHALL use the same parsing, validation, and path resolution as status.
An absent hook SHALL produce empty strings for both hook variables. A
configured hook SHALL display its program and positive
millisecond timeout, including the default of 2000 when omitted.

#### Scenario: Configuration Has Not Been Created

- **WHEN** the user runs `trees config show` with no configuration file
- **THEN** the command prints both platform defaults
- **AND** the command succeeds without creating the configuration file

#### Scenario: One Setting Is Configured

- **GIVEN** a valid configuration containing only one supported directory setting
- **WHEN** the user runs `trees config show`
- **THEN** the command prints the resolved configured directory and the other setting's default

#### Scenario: Relative and Explicit Directories Are Configured

- **GIVEN** a valid configuration with both supported directory settings
- **WHEN** the user runs `trees config show`
- **THEN** relative values resolve against the configuration file's directory
- **AND** existing and nonexistent paths follow the same normalization behavior as existing getters

#### Scenario: Bash Output Preserves Special Path Characters

- **GIVEN** a resolved path contains spaces, single quotes, dollar signs, backticks, backslashes, newlines, or command-substitution text
- **WHEN** the user runs `trees config show` and reads its output as Bash assignments
- **THEN** the corresponding variable values equal the displayed effective paths
- **AND** path content does not trigger expansion or command execution
- **AND** embedded newlines remain part of the variable values

### Requirement: Show Effective Configuration as JSON

With `--json`, successful inspection SHALL print exactly one compact JSON
object followed by a newline. The object SHALL contain `workspaces_dir` and
`origins_dir` as required string fields and `latest_session_hook` as a required
nullable object. A configured hook SHALL contain `program` as a
string and `timeout_ms` as an integer, matching text output. An absent hook
SHALL be represented by `null`. Member order SHALL NOT be significant. Paths
SHALL use the existing lossy display conversion and standard JSON escaping.

#### Scenario: Structured Inspection with Defaults or Configured Values

- **WHEN** the user runs `trees config show --json` with a valid or absent configuration
- **THEN** standard output contains one JSON object with both string fields
- **AND** the values match the effective configuration shown by text output
- **AND** no text-format fields or progress messages accompany the object

#### Scenario: Paths Contain Special Characters

- **GIVEN** a resolved path contains quotes, backslashes, or control characters
- **WHEN** the user runs `trees config show --json`
- **THEN** standard output is valid JSON whose decoded strings preserve the displayed path values

### Requirement: Inspect Without Storage Mutation

`config show` inspection SHALL read the configuration table once and resolve all supported
settings from that table. It SHALL NOT create or modify configuration, workspace,
origin, or database storage. It SHALL NOT require a repository or registered
workspace. It SHALL NOT execute or locate the hook program. Unknown settings SHALL follow existing getter behavior.

#### Scenario: Inspection Before Initialization

- **GIVEN** no Trees storage exists and the current directory is outside a repository
- **WHEN** the user runs `trees config show`
- **THEN** inspection succeeds using defaults and leaves storage absent

#### Scenario: Existing Configuration Contains Unrelated Keys

- **GIVEN** a valid TOML file containing supported settings and unrelated keys
- **WHEN** the user runs `trees config show`
- **THEN** only the defined settings are printed
- **AND** configuration contents and existing storage remain unchanged

#### Scenario: Hook Inspection Does Not Execute the Provider

- **GIVEN** a configured hook with a relative program path and no explicit timeout
- **WHEN** the user runs `trees config show` in either output mode
- **THEN** inspection shows the resolved program path and default timeout
- **AND** the hook is not executed and a missing executable does not prevent inspection

### Requirement: Report Configuration Errors Without Partial Results

If reading, parsing, validating a supported setting, or resolving a path fails,
the command SHALL use the existing CLI error reporting behavior in both output modes and exit with a
failure status. Configuration failures SHALL leave standard output empty, including with `--json`; errors
SHALL remain standard error diagnostics rather than JSON error objects.

#### Scenario: Invalid Configuration

- **GIVEN** malformed TOML or a supported section or value with an invalid type
- **WHEN** the user runs `trees config show`
- **THEN** the command reports the configuration error on standard error and exits unsuccessfully
- **AND** standard output is empty and configuration is unchanged

#### Scenario: Configuration Cannot Be Read

- **GIVEN** reading the configuration lookup location fails with an error other than not found
- **WHEN** the user runs `trees config show`
- **THEN** the command reports the filesystem error and exits unsuccessfully without standard output

#### Scenario: JSON Inspection Encounters Invalid Configuration

- **GIVEN** malformed TOML or an invalid supported setting
- **WHEN** the user runs `trees config show --json`
- **THEN** the command reports the error on standard error and exits unsuccessfully
- **AND** standard output is empty, with no partial JSON object

### Requirement: Locate the Configuration File

The CLI SHALL provide `trees config path` without positional arguments or
command-specific flags. It SHALL print the configuration lookup path obtained
from existing platform and environment rules as plain text followed by a newline
and exit successfully. It SHALL NOT add labels, variable assignments, or shell
quoting. The command SHALL NOT read, parse, validate, canonicalize, check the
existence of, create, or modify the configuration file. It SHALL NOT initialize
storage or require a repository. A path-resolution failure SHALL produce an
existing CLI standard error diagnostic, empty standard output, and a failure exit status.

#### Scenario: Locate Configuration Before Initialization

- **GIVEN** no Trees storage exists and the current directory is outside a repository
- **WHEN** the user runs `trees config path`
- **THEN** standard output contains the platform configuration lookup path followed by a newline
- **AND** no file or storage directory is created

#### Scenario: Locate Invalid or Unreadable Configuration

- **GIVEN** the configuration lookup path resolves but the file is malformed or unreadable
- **WHEN** the user runs `trees config path`
- **THEN** the command succeeds and prints the lookup path without reading the file

#### Scenario: Path Includes Spaces

- **GIVEN** the platform configuration lookup path includes spaces
- **WHEN** the user runs `trees config path`
- **THEN** standard output preserves those spaces without adding quotes or escape characters

#### Scenario: Configuration Lookup Path Cannot Be Resolved

- **GIVEN** the environment lacks information required by the existing platform path resolver
- **WHEN** the user runs `trees config path`
- **THEN** the command exits unsuccessfully with a diagnostic on standard error and empty standard output
