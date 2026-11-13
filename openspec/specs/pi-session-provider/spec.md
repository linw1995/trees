# Pi Session Provider Specification

## Purpose

Provide real Pi conversation metadata through the existing executable workspace session hook without changing agent or workspace storage.

## Requirements

### Requirement: Discover Pi Storage

The provider SHALL read the Pi agent directory from `PI_CODING_AGENT_DIR`,
defaulting to `~/.pi/agent`. It SHALL resolve the session root from
`PI_CODING_AGENT_SESSION_DIR`, then a top-level `sessionDir` in the agent
directory's `settings.json`, then the `sessions` subdirectory. Relative paths
SHALL resolve against the hook working directory.

#### Scenario: Discover Default Storage

- **WHEN** Pi has sessions under its default agent directory
- **THEN** the provider finds them without creating files

#### Scenario: Prefer the Environment Override

- **WHEN** `PI_CODING_AGENT_SESSION_DIR` and `settings.json` specify different session roots
- **THEN** the provider scans only the environment root

### Requirement: Read Pi Metadata Without Mutation

The provider SHALL scan `.jsonl` files recursively below the session root and open
them read-only. Missing storage SHALL produce empty session lists. Invalid
settings SHALL cause a nonzero exit with diagnostics only on standard error. The
provider SHALL NOT create, migrate, or repair Pi storage.

#### Scenario: Report Invalid Settings

- **WHEN** the agent `settings.json` is unreadable or has a non-string `sessionDir`
- **THEN** the provider exits nonzero and writes the diagnostic to standard error

### Requirement: Match Pi Workspace Roots

The provider SHALL read the first valid session header and match its canonical
working directory exactly to a requested workspace root. Sessions in nested
directories SHALL be excluded unless that directory is independently requested.

#### Scenario: Avoid Incorrect Workspace Attribution

- **WHEN** a Pi session was started in a child of a requested workspace root
- **THEN** it is excluded unless that child is independently requested as an exact root

### Requirement: Derive Pi Session Titles and Times

The provider SHALL prefer the latest `session_info` name, the first nonempty user
message text, and `Untitled session`, in that order. Update time SHALL use the
latest user or assistant message timestamp, then the header timestamp, then the
file modification time. Forked and cloned sessions SHALL be included.

#### Scenario: Preserve a Named Session

- **WHEN** a session has a latest nonempty `session_info` name and an earlier user message
- **THEN** the returned title uses the session name

### Requirement: Tolerate Partial Pi Storage

Malformed lines and unreadable individual session files SHALL be skipped so that
valid metadata in the same session still produces an entry.

#### Scenario: Skip a Truncated Write

- **WHEN** a session file ends with an incomplete JSON line
- **THEN** the remaining valid metadata still produces a session entry

### Requirement: Merge Providers Deterministically

The provider SHALL merge Codex and Pi sessions into one ordered list per requested
workspace. It SHALL order merged sessions by descending update time, then
descending session ID. Missing Codex or Pi storage SHALL contribute no sessions
without suppressing the other provider. Pi entries SHALL use agent `pi`.

#### Scenario: Merge Both Agents

- **WHEN** Codex and Pi have sessions for the same requested workspace
- **THEN** both appear in one list and the newest update is first

#### Scenario: Keep One Provider Without the Other

- **WHEN** only Pi storage exists and Codex has no state database
- **THEN** the response contains the Pi sessions and no failure
