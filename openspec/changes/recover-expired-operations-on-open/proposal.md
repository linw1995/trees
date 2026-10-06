# Proposal

## Why

An interrupted release retains an expired operation lease. Open rejects every
retained lease, so a workspace remains inaccessible even though observation-only
recovery can retire the operation while preserving local work and its claim.

## What Changes

- Recover eligible expired operations before opening a workspace.
- Require explicit lifecycle recovery for structural or unknown operations.
- Recheck removal and lease admission after recovery, outside Git observations.
- Review the implementation through isolated ablations and remove unnecessary
  abstractions, repeated fixtures, and redundant assertions.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-open`: Recover expired observation-only operations before handoff
  while preserving workspace contents and ownership.

## Impact

Shared lifecycle recovery, workspace opening, CLI storage access, integration
tests, and status documentation change. No migration or new CLI option is needed.
