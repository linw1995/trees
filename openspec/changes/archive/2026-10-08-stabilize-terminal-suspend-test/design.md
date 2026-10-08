# Design

## Context

Main CI run `37778442446` passed 424 tests and timed out waiting for a shell
marker in the suspension test. The same commit passed that test during the Nix
package build. A Linux `PTY` reproduction confirmed that a descendant blocked in
terminal input could consume the shell's marker command: resuming it without
additional input printed the consumed command and allowed it to finish.

An interactive shell waits for its direct child, the supervisor. That child's
stopped status does not acknowledge suspension of every descendant in the
foreground process group. The shell prompt therefore cannot fence the helper's
pending terminal read.

## Decisions

- Reuse the existing helper selector with a `continue` mode. Before announcing
  readiness, install a process-wide `SIGCONT` handler that writes the completion
  marker and exits using only async-signal-safe calls. Other helper invocations
  retain their terminal read and interrupt coverage.
- Keep the actual `Ctrl-Z` input, the shell marker command, and `fg`. Resume
  finishes the helper without requiring a competing terminal input consumer.
- Name the expected marker in the existing terminal deadline assertions.
- Keep the existing suspension test rather than add a second lifecycle fixture.
  Runtime behavior and the synchronized specifications remain unchanged.

## Ablation Review

Experiments and the final review are recorded in `verification.md` after they
complete. Retain only synchronization needed to prevent input competition and
early signal loss. A thread-local signal mask with `sigwait` was rejected after
the actual macOS suspension test timed out: another harness thread can receive
the process-directed signal. Do not add retries, fixed sleeps, terminal flags,
or a new signal abstraction.

## Risks / Trade-Offs

The handler must precede readiness so an early resume cannot be lost. It can run
on any harness thread and therefore avoids Rust output locks, allocation, and
normal exit cleanup. The helper is a dedicated process; it uses raw `write` and
`_exit`, as the interrupt tests likewise allow signal termination. Native Linux
and macOS checks are required.

## Migration Plan

No migration is needed. Review the test against `AGENTS.md`, run ablations and
repository checks, archive this tooling change with spec updates skipped, and
commit the reviewed result.
