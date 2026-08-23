## 1. Codex Project Identity and Recovery

- [x] 1.1 Define the deterministic project idempotency key and workspace ownership metadata without adding a Trees database table or migration; verify stable values for repeated launches and distinct workspace identities
- [x] 1.2 Define app-server project response models and ownership/root comparison primitives; verify ordered roots, workspace ownership matching, and ambiguous ownership detection

## 2. App-Server Protocol Client

- [x] 2.1 Implement a bounded newline-delimited JSON app-server transport that spawns `codex app-server --stdio`, performs the experimental `initialize` handshake, matches responses by request ID while tolerating notifications, and reports malformed-message, timeout, EOF, and process errors; verify behavior with a fake app-server fixture
- [x] 2.2 Implement project synchronization using `project/create`, `project/list`, and `project/update`; verify initial creation, idempotent reuse, complete root replacement, and recreation with a fresh key after a deleted project response
- [x] 2.3 Implement project-bound thread startup with `thread/start`, using the workspace container as `cwd` and all worktree roots as runtime workspace roots; verify the emitted JSON request contains the expected project identifier and ordered roots
- [x] 2.4 Ensure the setup process shuts down after standard input closes, waits for confirmed process exit before handoff, bounds captured standard error, and terminates or reaps failed children; verify no child process remains and no `resume` occurs after an unconfirmed shutdown

## 3. CLI Integration

- [ ] 3.1 Add the Codex command boundary that captures native argument vectors, exposes the optional `resume` wrapper, and keeps the executable override; verify direct native-argument parsing in CLI unit tests
- [x] 3.2 Reconcile the workspace before resolving managed-worktree roots, then validate attached worktrees and reuse canonical path and repository ordering rules; verify unknown, non-ready, empty, externally removed, and invalid-root cases fail before spawning Codex
- [x] 3.3 Wire `trees codex` dispatch to project synchronization and thread startup without changing Git or lifecycle snapshot state; verify the command produces no Git mutations in an integration test
- [x] 3.4 Hand the returned thread identifier to the configured executable's `resume` command with terminal inheritance, the workspace container as both the working directory and `--cd`, and every worktree root as `--add-dir`.
  Verify child arguments and inherited configuration. Verify exit-status propagation and runtime-root preservation with a fake executable
- [x] 3.5 Report missing binaries, unsupported app-server methods, malformed responses, and failed handoff with actionable errors while retaining already-created Codex state for retry; verify each failure path with process-boundary tests
- [x] 3.6 Append a model-visible logical monorepo manifest to effective developer instructions before `thread/start` and resend it through the `resume` handoff; verify all ordered roots are named and existing instructions are preserved
- [ ] 3.7 Add `trees codex resume [codex-args...]` with workspace derivation from forwarded `-C`/`--cd`, current-directory default, native picker delegation without a session identifier, workspace locking, and reuse of the complete resume handoff context; verify picker invocation, explicit path, no-session, and lock-conflict behavior
- [ ] 3.8 Derive the workspace for both commands from forwarded `-C`/`--cd` values, including separated and equals forms, with current-directory fallback and final-Codex-consistent repeated-value semantics
- [ ] 3.9 Forward native Codex arguments directly without a separator; parse and merge repeatable `--add-dir`, developer-instructions, and native selection arguments, preserve the original argument vector, and verify merged roots and prompts reach the final Codex process unchanged

## 4. Documentation and Verification

- [ ] 4.1 Document `trees codex [codex-args...]`, direct native-argument forwarding, managed-worktree root behavior, project reuse, executable override, and the terminal-only launch boundary in `README.md`; verify the examples match the final CLI help
- [x] 4.2 Add integration coverage for repeated launch, changed roots, external project deletion, externally removed worktrees, multiple Codex homes, user security defaults, and model-visible multi-repository context; verify the full scenario suite passes without requiring real Codex authentication
- [x] 4.3 Run `openspec validate codex-multi-root-project --strict` and resolve all structural or scenario errors
- [x] 4.4 Run `cargo test --all-targets --all-features`, `prek -a`, and `nix flake check --no-build`; verify the repository remains clean apart from the intended change artifacts
- [ ] 4.5 Document the no-identifier `trees codex resume [codex-args...]` flow, direct native argument forwarding, workspace derivation from `-C`/`--cd`, and replace the future-resume limitation wording with the managed resume path; verify the examples match the final CLI help
