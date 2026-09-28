# Proposal

## Why

The disk usage feature accumulated temporary designs as its implementation
changed from live status scans to a persisted cache. Review each feature commit and remove
residual machinery and tests that do not protect an observable behavior.

## What Changes

- Record isolated ablation results for the feature commits.
- Replace the unused multi-counter refresh result with a complete or incomplete
  outcome. Per-path standard error logs continue to carry measurement details.
- Remove tests whose assertions duplicate stronger integration coverage.
- Keep the externally specified JSON fields, race-safe traversal, path
  invalidation, removal invalidation, and five-second debounce.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. The change alters internal code and test coverage without changing the
documented disk usage behavior.

## Impact

The change affects internal size refresh code, focused tests, and this review
record. It adds no migration, command, or status schema change.
