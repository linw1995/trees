# Design

## Context

Lifecycle commands already recover expired operations. Non-creation recovery
observes worktrees and fails the interrupted operation without modifying files.
Create and add recovery can roll back or move paths and must remain explicit
when the user only wants to open a program.

## Goals / Non-Goals

**Goals:** Reuse lifecycle recovery, preserve files and claims, keep Git outside
SQLite transactions, and check admission again before handoff.

**Non-Goals:** Resume release, recover structural changes implicitly, expire
claims, or change status into a mutating command.

## Decisions

- Use one shared recovery implementation for lifecycle and access callers.
- Allow access recovery only for `acquire`, `claim`, `release`, `gc`, and `remove`.
  Classify the operation before taking over its lease and reject unknown kinds.
- Use the existing storage without creating or migrating it. Resolve IDs and
  selectors in short transactions, recover outside them, then recheck the
  selected workspace ID before closing storage and launching the program.
- Preserve the existing public lifecycle recovery API and expose a named
  access recovery function. Both delegate to one private implementation; callers
  do not need a public policy type. Keep mode selection inside these entry points.
- Reuse one real automatic workspace for all eligible operation kinds. Use
  metadata fixtures for admission and persistence tests, where Git state does
  not affect the behavior being tested. Existing selector integration tests cover
  the named selectors; this regression exercises the positional ID path.

## Risks / Trade-Offs

Recovery writes lifecycle observations and terminal audit events. Failure must
prevent program launch and retain an operation lease. Another writer can gain
admission after recovery, so checking only the initial snapshot is insufficient.
A passing suite after deleting a test is not evidence that its coverage was
redundant; identify the retained behavior coverage before removing assertions.

## Migration Plan

No schema migration is needed. Run focused and complete tests, repository hooks,
and strict specification validation. Archive only after final review passes.
