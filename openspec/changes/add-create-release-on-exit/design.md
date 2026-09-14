## Context

See `proposal.md` for motivation. `src/main.rs` currently uses `UNIX` `exec()` for
workspace opening, so no Trees process remains to release a claim. Automatic
allocation already returns a canonical path and claim ID. The release API
accepts that exact pair and rechecks ownership during operation admission.
Release already protects dirty work and records rejected attempts.

## Goals / Non-Goals

**Goals:** Keep process supervision separate from workspace mutation, reuse
release safety checks, and make the repair loop testable without launching a
real user shell in unit tests.

**Non-Goals:** Automatic removal, forced release, descendant process tracking,
background monitoring, guaranteed cleanup after supervisor termination, changes to
standalone open/release, and migration of existing claims.

## Decisions

### 1. Explicit Session Selection

Use `--release-on-exit` with clap requirements/conflicts. This composes with the
existing `--open` option without changing default claim retention. A nested
subcommand would duplicate allocation options and complicate positional path
parsing; automatically releasing every invocation of `--open` would
change established behavior.

### 2. A Small Supervised Session Orchestrator

Add a focused module, such as `src/workspace_session.rs`, with typed session
identity and initial process outcome. The globally unique claim ID and canonical
path identify the session; the workspace ID is obtained from the claim when
checking ownership, avoiding an additional allocation lookup. Main validates options, allocates, closes
the allocation connection, and hands the captured identity to the orchestrator.
Leave the existing execution path for calls without the flag.

Use this state flow:

```text
Allocate -> RunInitial -> ReleaseOriginal -> Done
                              |
                              v
                         CheckOwnership
                         /      |      \
                     Ended    Active   Unknown
                       |        |         |
                      Done   Recovery    Fail
                                |
                           ReleaseOriginal
```

Release and ownership reads use fresh, short-lived connections. Release opens
the existing database with `mode=rw`, so a missing database cannot be silently
recreated and mistaken for an ended claim. Neither initial
program execution nor recovery holds a connection, transaction, or operation
lease from the supervisor. Represent launch, wait, ownership, and recovery
errors with Snafu source chains; stringify only at the CLI boundary. Preserve
both initial failure and later recovery diagnostics instead of overwriting one.
A small process runner boundary permits deterministic state transition tests;
real CLI tests cover the integration.

### 3. Original Claim Is the Authority

Call the existing exact-path/exact-claim release API, never resolve the current
claim by workspace path for a retry. After release failure and before opening a
shell, inspect the original workspace claim in a consistent read. Confirmed
absence or replacement ends this session's responsibility; unreadable state
ends with an error. Every actual release still validates the captured claim
under operation admission, covering races after the read.

A shell cannot be fully fenced against another authorized process explicitly
releasing and reallocating the workspace after the check. Do not hold a lease
through the shell: that would block the user's manual `trees release`. Document
this concurrency boundary and stop further recovery on observed ownership loss.

### 4. Recovery Is Interactive and Repeatable

Resolve `$SHELL` from the supervisor environment when needed and execute it as
one executable with `-i`; do not parse shell command text. Require terminal
`stdin` and `stdout` for recovery, preventing repeated EOF shells in automation.
Report each release failure and the retry-on-exit instruction on `stderr` before
launch. Retry after any shell termination, even a nonzero status. Missing shell,
launch failure, lost workspace directory, or unverifiable ownership ends with
workspace path, original claim ID, and a manual release command.

Do not require a recovery shell before allocation for an explicit program:
a clean unattended run can complete without one. Default `--open` still
validates `$SHELL` before allocation as it does today.

### 5. Process and Signal Handling Are Part of Correctness

In supervised `UNIX` mode keep the child in the inherited foreground process
group so terminal input and output and shell job control remain usable. Install scoped
supervisor handlers for `SIGINT/SIGQUIT` rather than ignored dispositions; children
must receive default dispositions when spawned. The supervisor survives terminal
interrupts while the foreground child receives them. Forward supervisor `SIGTERM`
to the active child from normal control flow, not by performing lifecycle work
inside a signal handler. Reap each child before releasing and retry interrupted
waits. Restore original handlers only when the session ends. Keep handlers
active through recovery and release so
a second terminal interrupt cannot bypass ownership cleanup unexpectedly.

Use the existing `libc` dependency where appropriate and a minimal signal-safe
notification mechanism. Verify the actual process-group behavior with a `PTY` on
Linux and macOS, including `Ctrl-C`, `Ctrl-Z/foreground` resume, and recovery shells.
A wait failure with uncertain child state is terminal and does not authorize
release. Detached children and `SIGKILL` remain outside the guarantee.

### 6. Preserve the Initial Outcome

Store initial exit/launch outcome once. Successful recovery or an already-ended
claim returns the initial exit code (`UNIX` signal: `128 + signal`); launch/wait
errors stay failures. Unresolved cleanup preserves an initial nonzero code,
otherwise returns failure. Recovery shell statuses only drive another release
attempt. This avoids presenting an unsuccessful original command as successful
merely because a repair shell exited zero.

## Risks / Trade-Offs

- Repeated dirty exits keep opening shells -> Explain the loop before each shell;
  manual release inside the shell ends it. No automatic force or discard exists.
- Missing interactive terminal -> Fail with manual instructions only if recovery
  is needed; successful unattended release remains supported.
- Partial Git alignment or active operation leases -> Preserve existing release
  failure behavior and surface the actual reason; do not invent automatic repair.
- Explicit concurrent release during a shell -> Preserve exact-claim mutations
  and check ownership again before the next shell; no exclusive shell lease.
- Platform signal differences -> Require `UNIX` `PTY` verification and platform-gate
  signal behavior; test portable child waiting and exit behavior separately.
- Abrupt supervisor termination or programs that detach -> Document retained claims
  and direct-child lifetime; users must choose a foreground program mode.

## Migration Plan

No database migration is needed. Ship as an optional flag with examples and
recovery documentation. Rolling back removes the new orchestration and flag;
retained claims remain manageable through existing release commands. Complete
strict OpenSpec validation now; implementation checks remain unchecked in `tasks.md`.
