# Tasks

## 1. Persistent Cache

- [ ] 1.1 Add nullable disk usage observation columns to workspace, worktree,
  and origin tables with reversible migrations. Verify migration tests keep
  existing rows and initialize every cache to unknown without scanning.
- [ ] 1.2 Add typed cache serialization, reads, and path-checked writes. Verify
  round trips for complete, partial, unavailable, and unknown observations,
  malformed stored data, and a path changed before persistence.
- [ ] 1.3 Document the migration and unknown initial state in `docs/status.md`.
  Verify the documented upgrade path matches the read-only database behavior.

## 2. Explicit Refresh

- [ ] 2.1 Add `trees size refresh` selectors for current workspace, workspace
  ID, origin ID, and all entities. Verify parser conflicts and missing targets.
- [ ] 2.2 Scan selected workspace, worktree, and origin paths once per entity
  outside write transactions, then persist conditional results. Verify shared
  origins, missing directories, partial scans, changed paths, and summary
  counts with controlled fixtures.
- [ ] 2.3 Document refresh commands and measurement age in `docs/status.md`.
  Verify command examples against CLI tests.

## 3. Cached Status Output

- [ ] 3.1 Load only stored size observations in the read-only status snapshot
  and remove status directory scans. Verify observer hooks cannot run from
  status, including no target, `--all`, and every JSON view.
- [ ] 3.2 Show cached or unknown workspace sizes in summary and inventory,
  show source repository `SIZE`, and show target worktree size details. Verify
  complete, partial, unavailable, unknown, and no-color human output.
- [ ] 3.3 Extend JSON with cached worktree and origin observations while
  retaining target and inventory usage fields. Verify array order, matching
  target values, nullable times for unknown values, and version-2 output.
- [ ] 3.4 Update `docs/status.md` examples and cache semantics. Verify
  documentation fixtures and Markdown lint.

## 4. Lifecycle Refresh

- [ ] 4.1 Refresh affected caches after successful create, add, and release
  without holding a long write transaction. Verify CLI integration tests read
  newly stored sizes and a scan failure cannot reverse completed operations.
- [ ] 4.2 Invalidate removed workspace and worktree caches and avoid refresh
  for claim and open. Verify removal and selector tests preserve lifecycle
  behavior and do not expose stale pre-removal sizes.
- [ ] 4.3 Document automatic refresh boundaries and warning behavior in
  `docs/status.md`. Verify the documented cases against integration tests.

## 5. Verification

- [ ] 5.1 Run formatting, Clippy, the full test suite, repository hooks, and
  strict OpenSpec validation. Record commands, outcomes, and platform coverage
  in `verification.md`.
- [ ] 5.2 Measure status over a large and a many-workspace inventory and verify
  no directory traversal or database writes occur. Record representative
  latency and the remaining cache freshness limits in `verification.md`.
