## 1. CLI Contract

- [x] 1.1 Add `--release-on-exit` to create with required `--open` and conflicts for workspace path and JSON; verify parser tests cover explicit/default programs and rejection before mutation.
- [x] 1.2 Route only opted-in automatic allocations into supervised execution and close the allocation connection first; verify existing create/open tests preserve exec and claim retention behavior.

## 2. Process Supervision

- [x] 2.1 Add typed session identity, initial outcome, and Snafu launch/wait errors in a focused module; verify unit tests preserve source chains and outcome precedence.
- [ ] 2.2 Implement child execution with canonical `cwd`, inherited streams/environment, direct executable invocation, reaping, and interrupted-wait handling; verify controlled child tests cover exit 0, exit 7, launch failure, and uncertain wait failure without release.
- [ ] 2.3 Implement scoped `UNIX` signal handling and `SIGTERM` forwarding while preserving foreground terminal behavior; verify `PTY` tests cover `Ctrl-C`, `SIGQUIT`, `Ctrl-Z/resume`, recovery shell job control, and no cleanup before child termination on Linux and macOS.

## 3. Release and Recovery

- [ ] 3.1 Implement release retries using the captured path and claim with fresh short-lived connections and typed ownership classification; verify clean release, dirty rejection, stale claim, replacement during admission, and database read failure.
- [ ] 3.2 Implement `$SHELL -i` recovery, `stderr` explanation, and retry after every shell exit; verify repair success, repeated dirty exits, nonzero shell exit, and manual release inside the shell.
- [ ] 3.3 Handle unavailable shell, shell launch failure, missing `cwd`, noninteractive streams, and unverifiable ownership; verify termination without repeated launches or forced cleanup and diagnostics containing path, claim ID, and manual release command.
- [ ] 3.4 Preserve the initial program outcome through all recovery paths; verify exit-code and signal mapping, initial launch failure after successful cleanup, and cleanup failure after both successful and failed programs.

## 4. Integration and Documentation

- [ ] 4.1 Add synchronized `CLI/PTY` integration tests using isolated repositories and controlled helper programs; verify new and reused allocations, multi-repository dirty protection, lifecycle audit records, repeated recovery, claim replacement, and absence of held database transactions during children. Use startup handshakes rather than timing-based sleeps and guarantee helper cleanup.
- [ ] 4.2 Update `README.md` and `docs/workspaces.md` with opt-in examples, retry-on-shell-exit behavior, preserved program status, noninteractive fallback, direct-child lifetime, concurrent manual-release limits, and abrupt supervisor termination; verify examples match the delta specifications and existing existing behavior remains documented.
- [ ] 4.3 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, required repository checks, and `openspec validate add-create-release-on-exit --strict`; record results and native platform coverage before marking implementation complete.
