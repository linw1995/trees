## 1. Codex Project Identity and Recovery

- [x] 1.1 Define the deterministic project idempotency key and workspace ownership metadata without adding a Trees database table or migration; verify stable values for repeated launches and distinct workspace identities
- [x] 1.2 Define app-server project response models and ownership/root comparison primitives; verify ordered roots, workspace ownership matching, and ambiguous ownership detection

## 2. App-Server Protocol Client

- [x] 2.1 Implement a bounded newline-delimited JSON app-server transport that spawns `codex app-server --stdio`, performs the experimental `initialize` handshake, matches responses by request ID while tolerating notifications, and reports malformed-message, timeout, EOF, and process errors; verify behavior with a fake app-server fixture
- [x] 2.2 Implement project synchronization using `project/create`, `project/list`, and `project/update`; verify initial creation, idempotent reuse, complete root replacement, and recreation with a fresh key after a deleted project response
- [x] 2.3 Implement project-bound thread startup with `thread/start`, using the first worktree root as `cwd` and all worktree roots as runtime workspace roots; verify the emitted JSON request contains the expected project identifier and ordered roots
- [ ] 2.4 Ensure the setup process shuts down after standard input closes, waits for confirmed process exit before handoff, bounds captured standard error, and terminates or reaps failed children; verify no child process remains and no `resume` occurs after an unconfirmed shutdown

## 3. CLI Integration

- [ ] 3.1 Add `Codex` command arguments to the Clap boundary with a required workspace path and optional executable override; verify valid parsing and rejection of missing paths in CLI unit tests
- [ ] 3.2 Reconcile the workspace before resolving managed-worktree roots, then validate attached worktrees and reuse canonical path and repository ordering rules; verify unknown, non-ready, empty, externally removed, and invalid-root cases fail before spawning Codex
- [ ] 3.3 Wire `trees codex` dispatch to project synchronization and thread startup without changing Git or lifecycle snapshot state; verify the command produces no Git mutations in an integration test
- [ ] 3.4 Hand the returned thread identifier to the configured executable's `resume` command with terminal inheritance and the primary root as the working directory; verify child arguments, inherited configuration, and exit-status propagation with a fake executable
- [ ] 3.5 Report missing binaries, unsupported app-server methods, malformed responses, and failed handoff with actionable errors while retaining already-created Codex state for retry; verify each failure path with process-boundary tests

## 4. Documentation and Verification

- [ ] 4.1 Document `trees codex <workspace-path>`, managed-worktree root behavior, project reuse, executable override, and the terminal-only launch boundary in `README.md`; verify the examples match the implemented CLI help
- [ ] 4.2 Add integration coverage for repeated launch, changed roots, external project deletion, externally removed worktrees, multiple Codex homes, and user security defaults; verify the full scenario suite passes without requiring real Codex authentication
- [ ] 4.3 Run `openspec validate codex-multi-root-project --strict` and resolve all structural or scenario errors
- [ ] 4.4 Run `cargo test --all-targets --all-features`, `prek -a`, and `nix flake check --no-build`; verify the repository remains clean apart from the intended change artifacts
