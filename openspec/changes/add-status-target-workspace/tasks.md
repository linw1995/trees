## 1. Target Selection

- [x] 1.1 Add optional `WORKSPACE_ID` parsing while retaining all view and `--all`
  rules; verify parser tests cover valid IDs, invalid IDs, each view, and invalid
  `--all` combinations.
- [x] 1.2 Implement ID and canonical current directory selection using typed errors and nearest
  path-component ancestry; verify nested, symlinked, prefix-only, explicit-ID,
  unknown-ID, and removed-boundary cases.

## 2. Combined Snapshot

- [x] 2.1 Refactor target and inventory loading under one read-only transaction
  with one timestamp, reusing workspace snapshot assembly; verify a controlled
  concurrent writer cannot produce mismatched claim and pool results.
- [x] 2.2 Load target relationships independently of `--all` without requiring
  target path existence; verify removed and missing-directory targets retain
  complete claim, operation, repository, and timestamp data.
- [x] 2.3 Add nullable `target_workspace` to every version-2 JSON envelope; verify
  all three views preserve their existing fields, null targets, removed targets,
  and original JSON path and timestamp values.

## 3. Human Output

- [x] 3.1 Render the agreed field order, heading, status lock, mode emoji, optional
  operation, and repo summary; verify claimed/unclaimed, automatic/manual, and
  active/expired/inconsistent operation cases with representative fixtures.
- [x] 3.2 Implement summary local-time formatting without timezone text and with
  UTC fallback; verify a non-UTC date boundary, offset at the historical instant,
  unavailable local timezone, and `never` deterministically.
- [x] 3.3 Apply terminal escaping and display-width alignment to summary values;
  verify control characters cannot inject output and `NO_COLOR` and pipes emit
  no generated ANSI sequences.
- [x] 3.4 Compose summary and inventory with one separating blank line and the
  appropriate inventory heading; verify every view, duplicate target rows,
  empty inventories, and byte-for-byte unchanged no-target human output.

## 4. CLI Integration and Documentation

- [ ] 4.1 Route status through the combined snapshot before printing anything;
  verify unknown IDs, current directory resolution failures, missing storage, and pending
  migrations emit no partial standard output and preserve typed error sources.
- [ ] 4.2 Extend CLI regression coverage for current directory and explicit IDs across all views
  and output modes; verify no Git invocation, workspace mutation, database write,
  claim change, or operation recovery occurs.
- [ ] 4.3 Update `docs/status.md` with current directory/ID examples, summary layout, silent
  no-target behavior, persisted-health semantics, and additive JSON contract;
  verify examples match the rendered fixtures and Markdown lint passes.
- [ ] 4.4 Run workspace tests, formatting, Clippy, repository hooks, and strict
  OpenSpec validation; verify all pass and preserve the existing repos-view
  requirements when syncing the completed delta into the main spec.
