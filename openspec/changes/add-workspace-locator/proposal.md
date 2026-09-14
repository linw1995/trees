## Why

Workspace selection is implemented independently by status, release, open,
and managed-session preparation. Status scans all workspace rows for a nearest
ancestor, while release queries ancestors individually. Exact path, workspace
ID, and claim ID lookups are also coupled to command-specific behavior.

## What Changes

- Introduce a shared `workspace_locator` module with mutually exclusive typed
  selectors for workspace ID, exact path, containing directory, and claim ID.
- Separate path normalization and workspace lookup from lifecycle admission,
  reconciliation, presentation, and process launch.
- Migrate existing consumers without changing their existing selection defaults,
  error messages, transaction boundaries, or claim ownership checks.
- Add mutually exclusive `--workspace-id`, `--workspace-dir`, and `--claim-id`
  options to status, open, and release while preserving existing positional forms.
- Centralize CLI argument definitions, mutual-exclusion groups, input conversion,
  and default selection; commands provide only positional compatibility and
  explicit-selection policy.

## Capabilities

### New Capabilities

- `workspace-locator`: shared typed workspace selection and lookup semantics.

### Modified Capabilities

- `workspace-status`: accept explicit path and claim selectors.
- `workspace-open`: accept explicit path and claim selectors.
- `workspace-reuse`: accept a workspace ID release selector.

## Impact

Primary code: `src/cli.rs`, `src/main.rs`, `src/workspace.rs`,
`src/workspace_open.rs`, `src/status/target.rs`, `src/status/combined.rs`,
`src/status/report.rs`, `src/status/summary.rs`, and `src/codex/workspace.rs`.
Reuse storage queries and existing domain types. No schema migration or new
dependency is required. Implementation and command specification deltas are tracked in the task list.

## Non-Goals

- Generalize repository lookup, automatic workspace allocation, or creation.
- Change remove's workspace-or-repository ID dispatch or add implicit deletion.
- Change forwarded session arguments or their existing `-C` / `--cd` behavior.
- Introduce a provider registry, async abstraction, or trait hierarchy for one
  SQLite-backed implementation.
