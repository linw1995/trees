## Purpose

Supply current coding-agent session metadata through a bounded, user-configured query hook without storing agent-specific information in workspace lifecycle records.

## ADDED Requirements

### Requirement: Configure a User-Level Session Hook

Trees SHALL read an optional `[status.latest_session_hook]` table from its existing user configuration. The table SHALL require a nonempty string `program` identifying one executable and accept a positive integer `timeout_ms` defaulting to 2000. An `args` setting SHALL be rejected as invalid configuration. Absence of the table SHALL disable the hook. Trees SHALL NOT discover hook configuration inside a workspace or repository.

A bare program name SHALL resolve through inherited PATH. A relative program path containing a path
separator SHALL resolve against the configuration directory. Trees SHALL execute the program
directly with no additional command-line arguments, shell parsing, tilde expansion, or environment
substitution. The child SHALL inherit the invoking environment and use the configuration directory
as its working directory. The executable MAY be a compiled binary or a script with an appropriate
shebang and executable permission. Trees SHALL NOT select or require a language or interpreter;
request data SHALL be supplied through standard input.

#### Scenario: Execute a Language-Independent Provider

- **WHEN** the table specifies `program = "/absolute/path/to/trees-sessions"`
- **THEN** Trees executes that file directly without additional command-line arguments
- **AND** the same protocol applies to compiled binaries and executable scripts

#### Scenario: Reject Argument Configuration

- **WHEN** the hook table includes `args`
- **THEN** Trees reports `configuration_failed` without starting a process

#### Scenario: Resolve a Relative Executable

- **WHEN** program is `hooks/latest-session` and status runs from another directory
- **THEN** the executable resolves relative to the configuration file and not the invocation directory

### Requirement: Invoke One Batch for the Workspace Inventory

Trees SHALL invoke the enabled hook exactly once per `status --view workspaces` call with a nonempty inventory, for both human and JSON output, after successful persisted loading and closing the lifecycle database connection. The request SHALL contain only the inventory entries selected by `--all`, in their existing order, each once. A target excluded from that inventory SHALL NOT be added solely for its summary. Trees SHALL leave the target summary unchanged.

`status --no-hooks` SHALL bypass hook configuration loading and execution and SHALL be accepted with every view. Pools, repos, empty inventories, and unsuccessful persisted loading SHALL NOT load hook configuration or execute the hook. Missing configuration SHALL preserve existing human output. Hook execution SHALL finish before process observation so the hook and its children are not intentionally included as observation helpers.

#### Scenario: Batch Without a Target

- **WHEN** workspace view contains three entries and no target exists
- **THEN** one invocation receives those three entries, including for JSON output

#### Scenario: Skip Hooks

- **WHEN** `--no-hooks` is supplied, another view is selected, or the inventory is empty
- **THEN** no hook configuration is read and no hook process starts

#### Scenario: Preserve Removed Filtering

- **WHEN** a removed workspace is selected as target without `--all`
- **THEN** it remains in the summary but is not added to the hook request

### Requirement: Exchange Versioned Session Metadata

Trees SHALL send one `UTF-8` JSON document with integer `version: 1` and `workspaces`, an array of objects containing string `id` and absolute string `path`, then close standard input. A successful child SHALL exit zero and write exactly one JSON document with `version: 1` and a `workspaces` object keyed by requested workspace ID.

Each returned value SHALL contain a required ordered `sessions` array. Each session SHALL be an
object with nonempty strings `agent`, `id`, and `title`, and an RFC 3339 string `updated_at`.
An empty array SHALL mean a successful lookup with no sessions. A null array or null element
SHALL be invalid. An omitted requested ID SHALL mean unavailable information, not
no session. IDs not requested, duplicate keys, unsupported versions, missing required fields, or
invalid values SHALL invalidate the entire response. Unknown non-key fields SHALL be ignored for
forward compatibility. Standard output SHALL contain no log text.

The provider SHALL handle session discovery, workspace attribution, title fallback, session filtering, and list ordering. Provider documentation SHALL describe its policy. Trees SHALL display supplied metadata without inspecting agent storage or inferring session activity from claims. Sessions SHALL be workspace history rather than claim-scoped records; Trees SHALL NOT discard metadata on release or reuse. Trees SHALL NOT require workspace paths to exist before requesting metadata.

#### Scenario: Distinguish Empty and Missing

- **WHEN** one requested ID returns `sessions: []` and another is omitted
- **THEN** the first is a complete lookup with no session and the second is unavailable

#### Scenario: Reject Incorrect Association

- **WHEN** a response contains a workspace ID that was not requested or duplicate workspace keys
- **THEN** the batch is unavailable with an invalid-response issue

#### Scenario: Preserve Provider Selection

- **WHEN** a provider returns a title for a released workspace
- **THEN** Trees displays that title without substituting claim timestamps or clearing it

#### Scenario: Validate Every Session

- **WHEN** a workspace returns a valid first session followed by an invalid session, or returns `sessions: null`
- **THEN** the entire response is invalid even though only the first session would be displayed

### Requirement: Bound Execution and Degrade Without Failing Status

The configured timeout SHALL cover request delivery, response collection, and child completion.
Trees SHALL concurrently service standard input, standard output, and standard error to avoid pipe deadlock. Standard output
SHALL be limited to 1 `MiB` and retained standard error to 16 `KiB`. Excess standard error SHALL be drained and
discarded; excess standard output SHALL terminate the hook as unavailable. On timeout or output overflow
Trees SHALL terminate and reap the child and, on `Unix`, its dedicated process group. Inherited pipe
handles SHALL NOT cause an unbounded wait after the deadline.

Configuration errors, start failures, input/output failures, timeout, excessive output, nonzero exit, and invalid responses SHALL preserve the persisted report and successful status exit code. Stable issue codes SHALL be `configuration_failed`, `spawn_failed`, `io_failed`, `timed_out`, `output_limit_exceeded`, `exit_failed`, `invalid_response`, and `missing_result`. Nonzero exits SHALL discard any output metadata. Failed batches SHALL have no accepted session results.

Hook standard output and standard error SHALL NOT be forwarded to Trees standard output or standard error. Failed execution diagnostics SHALL be represented in observation issues with nullable exit code and bounded captured standard error; configuration diagnostics SHALL NOT include the complete configuration or inherited environment. Successful hook standard error SHALL be discarded. Trees SHALL make no claim that arbitrary hook code is sandboxed or side-effect-free.

#### Scenario: Kill a Stalled Hook

- **WHEN** a hook stops reading standard input or a descendant holds an output pipe open beyond the timeout
- **THEN** collection terminates within bounded cleanup time and status reports `timed_out`

#### Scenario: Preserve JSON on Failure

- **WHEN** a hook prints logs to standard output and exits nonzero
- **THEN** status still emits one valid report document, records `exit_failed`, and accepts no sessions

#### Scenario: Bound Output Memory

- **WHEN** a hook emits more than 1 `MiB` to standard output
- **THEN** Trees terminates collection and reports `output_limit_exceeded`

### Requirement: Present Session Observations Independently

Version-2 status JSON SHALL add nullable top-level `workspace_sessions`. It SHALL be null when no hook is configured or observation is skipped. Otherwise, it SHALL contain `observed_at`, `status`, `workspaces`, and `issues`. Observation time SHALL be independent of `snapshot_at`. Existing workspace objects, target summary, process observations, inventory ordering, and lifecycle timestamps SHALL remain unchanged.

`workspaces` SHALL map every requested ID to `{status, sessions}`. Entry status SHALL be
`complete` for a supplied valid result, including an empty array, and `unavailable` otherwise.
Unavailable entries SHALL have an empty `sessions` array. Complete entries SHALL retain every
returned session in provider order, without sorting, deduplication, or truncating the list. Batch status
SHALL be `complete` when all entries are complete, `partial` when some are complete, and
`unavailable` when none are complete or the batch fails. Each issue SHALL contain `code`, nullable
`workspace_id`, nullable `exit_code`, and nullable `stderr`. Missing entries SHALL have
`missing_result` issues. Issues SHALL be ordered by code then workspace ID. Complete observations
SHALL have no issues.

For a non-null observation, the human workspace table SHALL append `LATEST SESSION`. Complete
nonempty session lists SHALL render only the first session as `AGENT: TITLE`, complete empty lists
SHALL render `—`, and unavailable
entries SHALL render `unavailable`. A single deterministic diagnostic line after the table SHALL
list issue codes when issues exist, without printing raw standard error. Hook-provided text SHALL escape
terminal controls and backslashes before truncation to 60 display columns including an ellipsis;
truncation SHALL preserve Unicode character boundaries and complete escape tokens. Every inventory
row SHALL remain one physical line. JSON SHALL retain original full strings and complete session
lists in provider order. Trees SHALL NOT reorder sessions by `updated_at` or agent.

#### Scenario: Display and Serialize a Title

- **WHEN** a valid session title contains a newline, terminal escape byte, and wide Unicode characters
- **THEN** the human cell is escaped, bounded, and aligned without injecting terminal controls
- **AND** JSON preserves the full original title

#### Scenario: Display the First Session and Preserve the List

- **WHEN** a provider returns sessions `A` and `B` in that order, and `B` has a later `updated_at`
- **THEN** the human workspace row displays only `A` without reordering or combining titles
- **AND** JSON retains both `A` and `B` in their original order

#### Scenario: Report Partial Coverage

- **WHEN** one workspace has a nonempty session list, one has an empty list, and one is omitted
- **THEN** the batch is partial and the respective cells show the title, `—`, and `unavailable`
- **AND** the omitted ID has a `missing_result` issue

#### Scenario: Preserve Unconfigured Output

- **WHEN** no hook is configured or `--no-hooks` is supplied
- **THEN** the human table retains its existing columns and JSON has `workspace_sessions: null`
