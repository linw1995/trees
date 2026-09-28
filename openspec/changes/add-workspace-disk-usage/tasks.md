# Tasks

## 1. Disk Usage Observer

- [ ] 1.1 Add a typed disk usage observation model with stable issues and
  serialization; verify unit tests for complete, partial, unavailable, issue
  ordering, and nullable byte counts.
- [ ] 1.2 Implement Linux and macOS allocated-block traversal with checked
  arithmetic, inode deduplication, and directory-relative no-follow access;
  verify fixtures with sparse files, hard links, hidden entries, symlinks, and
  a concurrent symlink replacement cannot escape the target tree.
- [ ] 1.3 Classify missing or unreadable roots, descendant failures, races,
  overflow, and unsupported platforms; verify injected error tests keep partial
  sums or null unavailable results as specified.

## 2. Status Report Integration

- [ ] 2.1 Observe displayed workspace rows and the selected target after
  closing lifecycle storage, deduplicating by workspace ID. Verify fake-observer
  tests cover all views, shared and excluded targets, no target, and load errors.
- [ ] 2.2 Add top-level `target_disk_usage` and ordered `workspace_disk_usage`
  JSON. Verify CLI tests cover all views, removed rows, shared observations,
  partial and unavailable results, unchanged workspace objects, and successful
  version-2 reports after scan failures.
- [ ] 2.3 Document both JSON fields, independent observation timestamps,
  non-atomic measurement, and allocated-versus-reclaimable semantics in
  `docs/status.md`; verify field names and array order against CLI fixtures.

## 3. Human Status Output

- [ ] 3.1 Render Disk usage between Repos and Processes with binary units and
  stable incomplete reasons; verify exact output for bytes, KiB, higher units,
  partial and unavailable results, alignment, and `NO_COLOR` or piped output.
- [ ] 3.2 Add `SIZE` between `REPOS` and `RECONCILED` in each workspace row,
  retaining `LATEST SESSION` at the end; verify complete, partial, unavailable,
  and removed rows remain single-line and that pools and repos tables keep
  their columns.
- [ ] 3.3 Update target and workspace-table examples in `docs/status.md`;
  verify the rendered fixtures match both examples and no-target output has
  no extra heading or blank line.

## 4. Integration Checks

- [ ] 4.1 Run formatting, Clippy, the relevant status and CLI tests, and strict
  OpenSpec validation; verify all checks pass.
- [ ] 4.2 Measure representative large-workspace and many-workspace scans,
  confirm bounded scan concurrency and unchanged database, Git metadata, and
  workspace contents, and record timing and platform coverage in
  `verification.md`.
