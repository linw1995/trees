## Why

The existing workspace session hook provider only reads Codex metadata, so Pi
sessions do not appear in the `LATEST SESSION` column even when Pi stores them for
the same workspace. Add Pi as a second read-only source in the existing executable
provider so one hook configuration covers both agents.

## What Changes

- Read Pi session files from the configured agent and session directories.
- Match canonical Pi session working directories exactly to requested workspace
  roots and derive titled, ordered session entries.
- Merge Codex and Pi sessions into the existing version-1 hook response.
- Document Pi storage discovery, selection rules, merging, and compatibility.

## Capabilities

### New Capabilities

- `pi-session-provider`: Read-only Pi session metadata through the existing
  workspace session hook protocol.

### Modified Capabilities

None. The existing Codex provider behavior and hook protocol remain unchanged.

## Impact

Updates the executable provider script, its configuration documentation, and the
manual verification record. Rust status behavior, lifecycle storage, and hook
protocol fields are unchanged.
