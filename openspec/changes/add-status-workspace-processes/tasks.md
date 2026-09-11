## 1. Platform Observation

- [ ] 1.1 Validate the narrow `sysinfo` adapter against `cwd/UID` visibility and error classification on Linux and macOS; record findings and the dependency or native-adapter decision in `design.md` with real-child evidence.
- [x] 1.2 Implement typed observation models, issue aggregation, and Snafu source errors in `src/status/processes.rs`; verify `complete/partial/unavailable` serialization and count invariants with unit tests.
- [ ] 1.3 Implement Linux and macOS enumeration with current-user filtering, thread exclusion, `self/helper` exclusion, and race handling; verify controlled child processes and injected failures without elevated privileges.

## 2. Workspace Attribution and Orchestration

- [ ] 2.1 Implement component-based nearest-boundary attribution; verify root, nested, removed-boundary, prefix collision, physical symlink, `missing/deleted` `cwd`, and other-user cases with focused tests.
- [x] 2.2 Load minimal boundary context in the existing SQLite snapshot; verify concurrent registration changes cannot split target selection and attribution boundaries and preserve inventory consistency tests.
- [ ] 2.3 Invoke the observer once after dropping the read-only connection and only for a valid target; verify fake-observer call `counts/order` for every view, no target, unknown ID, missing storage, and database failure.

## 3. Output

- [ ] 3.1 Add the top-level target_processes presentation field while preserving `WorkspaceStatus` and schema version 2; verify all view contracts, `null/no-target` behavior, independent timestamps, and unchanged `target/inventory` equality.
- [ ] 3.2 Render Processes between Repos and Reconciled with sorted `PID` / NAME / `CWD` rows; verify the design examples, root dot, null-name placeholder, all completeness states, and no silent truncation.
- [ ] 3.3 Escape process text and deterministic issue reasons using existing display-width rules; verify control characters, non-`UTF-8` display conversion, Unicode alignment, NO_COLOR, and piped output.

## 4. Integration and Documentation

- [ ] 4.1 Add CLI integration coverage with synchronized child startup and guaranteed cleanup on `Linux/macOS`; verify `inferred/explicit` targets, nested `cwd`, other-workspace exclusion, and process exit without timing-based sleeps.
- [ ] 4.2 Verify observation failures preserve successful persisted status and valid single-document JSON; verify storage contents, lifecycle events, Git metadata, and workspace contents remain unchanged.
- [ ] 4.3 Update `docs/status.md` with output examples, JSON fields, `effective-UID/cwd` ownership, visibility limits, and separate observation timing; verify statements agree with the delta spec and no longer describe status as exclusively persisted data.
- [ ] 4.4 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, existing repository checks, and strict OpenSpec validation; record results and platform coverage before marking implementation complete.
