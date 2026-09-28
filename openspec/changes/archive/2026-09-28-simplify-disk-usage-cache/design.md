# Design

## Context

The current branch has a complete cached disk usage implementation and two
archived OpenSpec changes. The former explicit refresh command and live status
scan have already been removed. The internal refresh result still carries four
counters designed for the removed command; lifecycle callers use only whether
any entity was incomplete.

## Goals / Non-Goals

**Goals:** Reduce internal surface and redundant test setup while retaining
observable output, failure handling, and safety properties.

**Non-Goals:** Change cache freshness, the five-second window, JSON shape,
filesystem accounting, or cross-platform support.

## Decisions

Use an isolated detached worktree to change one behavior at a time and run its
focused regression test. Restore each ablation before the next. Keep code when
removal breaks a required behavior or removes a small safety guard. Remove the
four-counter result because there is no result-printing CLI; return a typed
complete or incomplete outcome and retain detailed per-path standard error logs.

Do not remove thin all-cache read helpers only to force many tests to duplicate
Diesel queries. Their ablation leaves production compiling but breaks 24 test
call sites; replacement test scaffolding would add more complexity than it
removes.

## Risks / Trade-Offs

- A green test suite after deleting a test does not prove the test was
  unnecessary. Remove only cases whose behavior is covered by another focused
  test, and identify that coverage in `review.md`.
- The symlink race test also passes without one no-follow flag because inode
  verification provides a second guard. Retain both low-cost checks because
  filesystem containment is an explicit contract.

## Migration Plan

No migration is needed. Validate the final code and archive this change only
after the commit review and full test suite pass.
