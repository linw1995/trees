# Tasks

## 1. Commit Review

- [x] 1.1 Inspect every feature commit in order and record the current value
  of its changes in `review.md`; verify the commit list matches the branch log.
- [x] 1.2 Run isolated ablations of scanner deduplication, JSON compatibility,
  debounce, migration invalidation, verbose logging, and test-only cache APIs;
  record focused test or compile outcomes and restore each experiment.

## 2. Simplification

- [x] 2.1 Replace the unused refresh counters with a typed completeness
  outcome; verify lifecycle warning and focused cache tests.
- [x] 2.2 Remove duplicated unit and integration test setup; verify remaining
  tests still cover missing paths, shared origins, and lifecycle behavior.

## 3. Final Review

- [x] 3.1 Run formatting, Clippy, the complete test suite, repository hooks,
  and strict OpenSpec validation; record results in `review.md`.
- [x] 3.2 Inspect the final diff and working tree, then archive this change
  without a spec delta; verify the main specifications remain valid.
