## ADDED Requirements

### Requirement: Release the Original Session Claim

After the initial program terminates in `create --release-on-exit`, Trees SHALL
attempt ordinary release using the workspace identity, canonical path, and claim
identifier acquired by this invocation. This SHALL apply to successful exit,
nonzero exit, signal termination, and failure to start the initial program.
Existing release admission, reconciliation, Git preservation, and lifecycle
recording requirements SHALL apply to every attempt. Trees SHALL NOT force
cleanup, delete workspace contents, or adopt a replacement claim.

If persisted state confirms that the original claim is no longer active, Trees
SHALL treat its ownership obligation as finished without releasing another
claim or launching another recovery shell. A database read failure SHALL NOT
be interpreted as evidence that ownership has ended.

#### Scenario: Return a Clean Workspace

- **WHEN** the initial program terminates and ordinary release succeeds
- **THEN** the original claim is released and the workspace becomes available to the pool

#### Scenario: Release After Program Failure

- **WHEN** the initial program exits nonzero, terminates from a signal, or cannot start
- **THEN** Trees attempts to release its original claim

#### Scenario: Preserve Dirty Work

- **WHEN** release rejects staged, unstaged, or untracked changes
- **THEN** ordinary release retains the claim and work, and the session enters recovery

#### Scenario: Finish After Manual Release or Reallocation

- **WHEN** persisted state confirms that the original claim has disappeared, including after manual release in a recovery shell
- **THEN** Trees ends its session cleanup without changing a replacement claim or launching another shell

#### Scenario: Recheck Ownership During Release

- **WHEN** the claim changes between a session ownership check and release admission
- **THEN** release refuses the stale claim and the session does not adopt the new claim

### Requirement: Recover Failed Release in the Workspace Shell

When release fails and ownership has not been confirmed ended, Trees SHALL
print the failure reason and explain that exiting the recovery shell retries
release. It SHALL launch the nonempty `$SHELL` executable directly with `-i`,
inherited environment and standard streams, and the original workspace as its
working directory. Trees SHALL wait without holding a database connection or
transaction. After each shell terminates, regardless of its exit status, Trees
SHALL retry the release with the original claim and repeat recovery on failure.
Before each recovery launch, Trees SHALL check ownership; a confirmed ended
claim SHALL finish cleanup instead. A failed ownership check SHALL be reported
and SHALL stop recovery without assuming the workspace is still owned.

If `$SHELL` is missing or empty, shell startup fails, or interactive terminal
input/output is unavailable, Trees SHALL stop automatic recovery with a failure
and report the workspace path, original claim ID, and
`trees release --claim-id <original-claim-id>` for manual intervention. Trees
SHALL NOT force release or loop spawning shells that immediately consume EOF
from redirected input. An explicitly selected initial program SHALL NOT require
`$SHELL` unless recovery is needed.

#### Scenario: Repair and Retry

- **WHEN** release fails and the original claim remains active with an interactive terminal available
- **THEN** Trees prints the cause, opens `$SHELL -i` in that workspace, and retries the release after the shell exits

#### Scenario: Repeat Recovery After an Unsuccessful Repair

- **WHEN** a recovery shell exits nonzero or leaves work that still prevents release
- **THEN** Trees retries the release and opens another recovery shell if release still fails

#### Scenario: Fail Gracefully Without a Recovery Shell

- **WHEN** recovery is required but `$SHELL` is missing, empty, or cannot start
- **THEN** Trees stops with a nonzero outcome and prints manual recovery details without forcing release

#### Scenario: Avoid Noninteractive Recovery Loops

- **WHEN** release fails with redirected input or no interactive terminal output
- **THEN** Trees reports manual recovery details and exits without launching a recovery shell

#### Scenario: Report Unverifiable Ownership

- **WHEN** a database failure prevents the session from confirming current ownership before recovery
- **THEN** Trees reports that failure and manual recovery details without launching a shell or reporting successful cleanup

### Requirement: Preserve Session Outcomes and Terminal Behavior

After cleanup succeeds or ownership is confirmed ended, Trees SHALL preserve
the initial program's exit code; recovery shell statuses SHALL NOT replace it.
On `UNIX`, signal termination SHALL map to `128 + signal`. An initial launch or
supervision error SHALL remain a failure even if release later succeeds. If
recovery cannot complete, Trees SHALL preserve an already nonzero program
outcome and otherwise return failure. Diagnostics SHALL go to `stderr`.

The initial program and recovery shells SHALL remain usable interactively.
On `UNIX`, terminal-generated `SIGINT` and `SIGQUIT` SHALL reach the foreground
child without prematurely terminating its supervisor; release SHALL execute after
the child terminates. A `SIGTERM` delivered to the supervisor SHALL be forwarded
to its active child; the supervisor SHALL wait for child termination and then
perform release. Interrupted waits SHALL NOT imply child termination. If child
termination cannot be established, Trees SHALL report failure without releasing
its workspace. Abrupt supervisor termination, including `SIGKILL`, SHALL NOT promise
cleanup. The session SHALL track the launched child, not detached descendants.

#### Scenario: Preserve the Initial Failure After Repair

- **WHEN** the initial program exits with status 7 and recovery eventually releases its claim
- **THEN** Trees exits with status 7 regardless of recovery shell exit codes

#### Scenario: Report Unresolved Cleanup

- **WHEN** the initial program exits successfully but recovery cannot run or complete
- **THEN** Trees exits nonzero with manual recovery details on `stderr`

#### Scenario: Recover After Terminal Interrupt

- **WHEN** `Ctrl-C` terminates the initial foreground child
- **THEN** the supervisor waits for termination and attempts to release, entering recovery if needed

#### Scenario: Forward Supervisor Termination

- **WHEN** the supervisor receives `SIGTERM` while a child is running
- **THEN** it forwards `SIGTERM` to that child and waits for termination before attempting to release

#### Scenario: Wait for Actual Child Termination

- **WHEN** waiting is interrupted or the child has not been confirmed terminated
- **THEN** Trees does not release the workspace while that child may still be running

#### Scenario: Receive Inherited Blocked Child Notifications

- **WHEN** the caller has blocked `SIGCHLD` before launching a supervised session
- **THEN** Trees still observes child termination and attempts to release the original claim
