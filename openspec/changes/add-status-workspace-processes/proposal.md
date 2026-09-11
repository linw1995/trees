## Why

Target workspace status exposes persisted lifecycle state but cannot show which
local processes are currently working inside that workspace. A process count
and list make active shells and development commands visible during inspection.

## What Changes

- Add Processes between Repos and Reconciled in every target summary, with a
  count and an indented `PID` / NAME / `CWD` table when matches exist.
- Match current-user processes by their current working directory and the
  nearest registered workspace boundary; exclude the observer and its helpers.
- Distinguish complete, partial, and unavailable observations without failing
  an otherwise valid status command.
- Add nullable top-level target_processes to all version-2 JSON views, with
  an independent observation time and structured issues.
- Observe processes after the read-only database transaction; preserve global
  inventory behavior, target selection, and lifecycle state.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-status`: Observe target processes, render their count and list,
  extend JSON, and separate live observation from persisted snapshot semantics.

## Impact

Changes affect `src/status/processes.rs` (new), status projection and summary
modules, `src/main.rs` orchestration, platform dependencies, tests, and
`docs/status.md.` Linux and macOS are the supported observation platforms.
There is no storage migration or new command-line flag. Process observation
adds no lifecycle admission checks and does not establish safe removal or reuse.
