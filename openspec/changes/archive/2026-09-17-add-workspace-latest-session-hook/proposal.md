## Why

Workspace identifiers and repository sets do not explain what a coding agent most recently worked on. A user-configured query hook can supply session titles without coupling Trees to agent-specific storage formats or duplicating mutable titles in lifecycle storage.

## What Changes

- Configure one batch hook under `[status.latest_session_hook]` in the existing user configuration, with one executable `program` and optional `timeout_ms`; no argument list or language-specific interpreter configuration.
- Invoke the hook once for a nonempty workspace inventory, after closing lifecycle storage, for both human and JSON output.
- Exchange version-1 JSON over standard input and output, associating ordered session lists with workspace IDs.
- Append `LATEST SESSION` to the workspace table when the hook is enabled, displaying the first returned session; retain complete provider-ordered lists in version-2 status JSON.
- Add `status --no-hooks`, bounded execution and output, and nonfatal reporting of configuration, execution, and protocol failures.
- Document configuration and provide an executable protocol example without an agent-specific integration.

## Capabilities

### New Capabilities

- `workspace-session-hook`: User configuration, batch query protocol, execution limits, observation states, and presentation of ordered session lists and their first entries.

### Modified Capabilities

- `workspace-status`: Allow the optional workspace session column and external observation after persisted snapshot loading while retaining existing lifecycle guarantees.

## Impact

Changes affect `src/config.rs`, status CLI arguments, `src/status/report.rs`, status rendering, a session-hook module, focused tests, and configuration/status documentation. There is no database migration, built-in Codex or Claude integration, cache, remote operation, or new daemon. The implementation must use typed Snafu errors and preserve existing module boundaries.
