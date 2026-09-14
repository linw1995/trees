## 1. Specify the Contract

- [x] Add the workspace-locator specification from the design decisions.
- [x] Add status, open, and workspace-reuse specification deltas for new named selectors, mutual exclusion, defaults, and error behavior.
- [x] Pin existing diagnostics, exact-path semantics, removed-boundary behavior, and release claim-race behavior in focused regression coverage.

## 2. Implement Shared Lookup

- [x] Add and export `workspace_locator` with typed selectors, result provenance, and Snafu errors.
- [x] Implement ID, exact-path, nearest-ancestor, and claim lookup using existing storage queries without owning transactions.
- [x] Cover absent IDs/claims/paths, query failures, nearest nested boundaries, removed inner boundaries, and component-prefix collisions with in-memory storage tests.
- [x] Cover symlink normalization, relative exact paths, and supported missing final path components at the normalization boundary.

## 3. Migrate Existing Behavior

- [ ] Replace status target lookup while retaining no-database behavior, one snapshot, existing headings, and post-connection process observation.
- [ ] Replace release identity lookup while preserving claim provenance, active-claim snapshotting, and fail-fast transactional validation.
- [ ] Route open's ID selection through the locator within its existing snapshot and preserve all eligibility checks.
- [ ] Replace managed-session preparation's initial exact-path lookup without changing argument forwarding or reconciliation.
- [ ] Delete duplicated ancestor algorithms and obsolete selector types; keep storage-level collision checks and admission rereads.

## 4. Expose Mutually Exclusive CLI Selectors

- [ ] Add `cli::workspace_locator` with shared named arguments, typed explicit inputs, and an explicit-selection/default-current directory policy.
- [ ] Centralize Clap group construction, including legacy positional membership and required selection; commands provide only their positional metadata and default policy.
- [ ] Implement one conversion path for conflict validation, ID parsing, current directory fallback, and path normalization; reject conflicts before filesystem access, including outside Clap parsing.
- [ ] Add thin typed adapters for legacy ID/path positional inputs and remove command-local selector branching and duplicate normalization.
- [ ] Wire new status/open/release forms, preserving required explicit selection for open and current directory defaults for status/release.
- [ ] Extend status human headings for path and claim selection without changing JSON or inventory behavior.
- [ ] Test all pairwise selector conflicts, repeated named selectors, malformed IDs, defaults, explicit misses, and successful selection through each command.
- [ ] Exercise shared conversion directly and verify parser/help construction uses the same selection policy; ensure explicit ID/claim selection never requires current directory resolution.
- [ ] Update README examples and command specifications to match implemented syntax.

## 5. Verify and Review

- [ ] Run formatting, focused locator/CLI/status/open/reuse/session/removal tests, then the repository's required Rust checks and full test suite.
- [ ] Verify release rejects a claim replaced between lookup and admission and never acts on an outer workspace after an inner-boundary rejection.
- [ ] Verify status/open snapshot consistency and that read-only selection neither creates storage nor writes lifecycle data.
- [ ] Verify remove still rejects cross-table ID collisions and session forwarding arguments retain their current interpretation.
- [ ] Review the final diff for duplicate lookup implementations, unintended error-text changes, and unnecessary database/API changes.
- [ ] Validate the completed OpenSpec change with the available project tooling and record actual verification results.
