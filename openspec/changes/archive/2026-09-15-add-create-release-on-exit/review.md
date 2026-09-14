## Review Scope

Reviewed the implementation against both delta specifications, including argument
validation, child supervision, release admission, recovery shells, ownership
changes, exit status precedence, and compatibility without the new flag.
The latest main branch was merged before final validation.

## Finding and Resolution

A caller can block `SIGCHLD` before executing Trees. Installing a signal handler
does not remove that inherited mask. The original implementation could therefore
wait forever after its child terminated, preventing the required release.

A regression test reproduced the failure: the child completed successfully but
the supervised session exceeded its terminal deadline. Supervision now unblocks
notification, interrupt, and termination signals after installing handlers and restores the caller's signal
mask when the session ends. The regression passes on macOS and Linux; a unit
test also verifies restoration of the previous mask.

## Specification Mapping

| Requirement | Review result |
| --- | --- |
| Optional automatic creation | Invalid option combinations fail before mutation; default opening remains compatible. |
| Original session claim | Release receives the captured claim and canonical path; admission rechecks ownership. |
| Git preservation | Existing preflight, alignment, and audit operations are reused; dirty work retains its claim. |
| Recovery shell | Failed release opens an interactive shell when possible and retries after each exit. |
| Ownership loss | A missing or replaced claim ends recovery; unreadable storage produces a failure. |
| Process outcomes | Original failures survive recovery; uncertain child termination prevents release. |
| Terminal behavior | Native terminal tests cover interrupts, termination forwarding, suspension, and recovery job control. |

No blocking specification discrepancies remain after the signal-mask fix.
The documented direct-child lifetime and concurrent manual-release boundaries
remain applicable. See `verification.md` for validation results.
