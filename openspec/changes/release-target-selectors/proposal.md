## Why

Release currently requires both a workspace path and claim identifier even though either value can identify the active workspace claim. This makes interactive release cumbersome and couples automation to redundant state while the command does not explicitly define fail-fast behavior for concurrent release attempts.

## What Changes

- Allow `trees release` to select exactly one target by positional workspace path, `--cwd`, or `--claim-id`.
- Map path and current-directory targets to the active claim at command start, and map claim identifiers directly to their workspaces.
- Make release operation admission try-once: an active workspace operation or SQLite writer conflict returns busy immediately without waiting or retrying.
- Update release documentation and tests for all target forms, invalid combinations, path resolution, and contention.
- **BREAKING**: Reject the previous combined `trees release <workspace-path> --claim-id <claim-id>` form because release targets are now mutually exclusive.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-reuse`: Change the release CLI target contract and concurrent admission behavior.
- `workspace-lifecycle`: Clarify how release resolves and removes the active claim under per-workspace operation exclusion.

## Impact

The CLI parser, release dispatch, workspace target resolution, operation admission, README usage, OpenSpec contracts, and release tests change. The persistence schema and external dependencies remain unchanged.
