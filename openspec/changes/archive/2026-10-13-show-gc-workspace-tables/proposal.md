# Proposal

## Why

GC prints workspace paths and individual counters, making cleanup decisions
harder to inspect than the workspace status table. Removed history also inflates
the scan inventory.

## What Changes

- Render scan counts, candidate rows, and execution totals as aligned tables.
- Show repository state, cached size, idle time, and workspace ID without paths.
- Exclude removed history from candidate rows and scan counts.
- Keep display data in the CLI and simplify repeated regression assertions.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-reuse`: Define the GC table output and current candidate inventory.

## Impact

GC filtering, CLI presentation, shared status rendering, user documentation,
and focused tests change. No migration or new dependency is needed.
