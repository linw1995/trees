# Proposal

## Why

The terminal suspension test can time out after the outer shell reports a
stopped supervisor. Its descendant helper can still consume the command meant
for the shell before that helper stops.

## What Changes

- Wait for `SIGCONT` in the existing process helper for the suspension test.
- Preserve terminal input in the interrupt tests and retain real shell job control.
- Include the expected marker in terminal timeout diagnostics.
- Validate the change through native platform checks and ablation experiments.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change repairs test synchronization without changing runtime behavior
or the existing workspace session requirements. It explicitly skips spec deltas.

## Impact

Only `tests/workspace_session.rs` and this change record are affected. No runtime
code, dependency, signal policy, or public API changes are required.
