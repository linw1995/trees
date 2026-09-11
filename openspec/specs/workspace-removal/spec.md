# Workspace Removal Specification

## Purpose

This capability defines explicit physical removal of one managed workspace
without deleting its durable lifecycle history.

## Requirements

### Requirement: Remove One Workspace by Stable Id

The CLI SHALL provide `trees remove <workspace-id> [--dry-run] [--yes]
[--force]`. The ID SHALL be a UUID v7 workspace identifier. Explicit removal
SHALL accept automatic and manual workspaces and SHALL NOT apply an idle-age
threshold.

#### Scenario: Remove a Manual Workspace

- **WHEN** a caller confirms removal for a safe manual workspace by its ID
- **THEN** Trees removes the workspace's managed worktrees and directory and
  records removed tombstones

#### Scenario: Remove an Automatic Workspace

- **WHEN** a caller confirms removal for a safe unclaimed automatic workspace
- **THEN** Trees removes that workspace without evaluating its release age

### Requirement: Preserve Removal Admission Guards

Explicit removal SHALL reject unknown and already removed workspaces. It
SHALL reject a workspace with an active claim or unexpired operation lease.
It SHALL recover an expired operation before starting execution and SHALL
serialize the new operation through the per-workspace operation lease.

#### Scenario: Preserve an Active Claim

- **WHEN** removal targets a claimed automatic workspace, including with
  `--force`
- **THEN** it fails without removing physical state, the claim, or history

#### Scenario: Reject Concurrent Mutation

- **WHEN** removal targets a workspace with an unexpired operation lease
- **THEN** it fails without changing Git or filesystem state

### Requirement: Validate Physical Removal

Normal removal SHALL require a ready workspace whose recorded worktrees are
clean, detached, present, identity-matched, at their recorded revisions, and
whose workspace directory contains no unexpected entries. `--force` MAY remove
unhealthy, dirty, changed, missing, or extra physical content, but SHALL NOT
bypass claims, unexpired operations, path containment, source repository
identity, present worktree identity, or branch-attachment checks.

#### Scenario: Reject Unsafe Normal Remove

- **WHEN** normal removal observes dirty worktree or unexpected workspace
  content
- **THEN** it reports the ineligible reason without physical removal

#### Scenario: Force Remove Local Content

- **WHEN** forced removal targets an unclaimed detached workspace with local
  changes and all hard guards pass
- **THEN** it removes the workspace and records that the operation was forced

### Requirement: Support Read-Only Preflight

`--dry-run` SHALL resolve and validate the target through a read-only database
connection and SHALL NOT recover operations, append events, mutate Git, or
modify filesystem content.

#### Scenario: Inspect Removal Eligibility

- **WHEN** remove is invoked with `--dry-run`
- **THEN** it reports the target path and current eligibility reason without
  changing lifecycle or physical state

### Requirement: Confirm Explicit Removal

Execution SHALL require interactive confirmation unless `--yes` or `--force`
is supplied. A noninteractive invocation without either flag SHALL fail.
`--force` SHALL imply confirmation and print a destructive-content warning.

#### Scenario: Reject Unconfirmed Noninteractive Removal

- **WHEN** execution has candidates but standard input is noninteractive and
  neither `--yes` nor `--force` is present
- **THEN** it fails without starting an operation or removing content

### Requirement: Preserve Tombstones and Failure History

Explicit removal SHALL persist operation intent before external mutation,
renew its lease during physical removal, and mark the workspace and its
repo-worktrees removed only after physical removal succeeds. It SHALL retain
prior lifecycle history. A partial or failed removal SHALL not mark the
workspace removed and SHALL remain auditable.

#### Scenario: Persist Successful Explicit Removal

- **WHEN** physical removal completes successfully
- **THEN** the workspace and repo-worktrees are removed tombstones and the
  explicit removal operation is recorded as succeeded

#### Scenario: Preserve Failed Explicit Removal

- **WHEN** physical removal fails after the operation starts
- **THEN** Trees records the failed operation and current observed state without
  reporting successful removal
