# Workspace Reclaim Specification

## Purpose

This capability defines explicit physical reclamation of one managed workspace
without deleting its durable lifecycle history.

## ADDED Requirements

### Requirement: Reclaim One Workspace by Stable ID

The CLI SHALL provide `trees reclaim <workspace-id> [--dry-run] [--yes]
[--force]`. The ID SHALL be a UUID v7 workspace identifier. Explicit reclaim
SHALL accept automatic and manual workspaces and SHALL NOT apply an idle-age
threshold.

#### Scenario: Reclaim a Manual Workspace

- **WHEN** a caller confirms reclaim for a safe manual workspace by its ID
- **THEN** Trees removes its managed worktrees and workspace directory and
  records reclaimed tombstones

#### Scenario: Reclaim an Automatic Workspace

- **WHEN** a caller confirms reclaim for a safe unclaimed automatic workspace
- **THEN** Trees removes that workspace without evaluating its release age

### Requirement: Preserve Reclaim Admission Guards

Explicit reclaim SHALL reject unknown and already reclaimed workspaces. It
SHALL reject a workspace with an active claim or unexpired operation lease.
It SHALL recover an expired operation before starting execution and SHALL
serialize the new operation through the per-workspace operation lease.

#### Scenario: Preserve an Active Claim

- **WHEN** reclaim targets a claimed automatic workspace, including with
  `--force`
- **THEN** it fails without removing physical state, the claim, or history

#### Scenario: Reject Concurrent Mutation

- **WHEN** reclaim targets a workspace with an unexpired operation lease
- **THEN** it fails without changing Git or filesystem state

### Requirement: Validate Physical Removal

Normal reclaim SHALL require a ready workspace whose recorded worktrees are
clean, detached, present, identity-matched, at their recorded revisions, and
whose workspace directory contains no unexpected entries. `--force` MAY remove
unhealthy, dirty, changed, missing, or extra physical content, but SHALL NOT
bypass claims, unexpired operations, path containment, source repository
identity, present worktree identity, or branch-attachment checks.

#### Scenario: Reject Unsafe Normal Reclaim

- **WHEN** normal reclaim observes dirty worktree or unexpected workspace
  content
- **THEN** it reports the ineligible reason without physical removal

#### Scenario: Force Reclaim Local Content

- **WHEN** forced reclaim targets an unclaimed detached workspace with local
  changes and all hard guards pass
- **THEN** it removes the workspace and records that the operation was forced

### Requirement: Support Read-Only Preflight

`--dry-run` SHALL resolve and validate the target through a read-only database
connection and SHALL NOT recover operations, append events, mutate Git, or
modify filesystem content.

#### Scenario: Inspect Reclaim Eligibility

- **WHEN** reclaim is invoked with `--dry-run`
- **THEN** it reports the target path and current eligibility reason without
  changing lifecycle or physical state

### Requirement: Confirm Explicit Reclamation

Execution SHALL require interactive confirmation unless `--yes` or `--force`
is supplied. A non-interactive invocation without either flag SHALL fail.
`--force` SHALL imply confirmation and print a destructive-content warning.

#### Scenario: Reject Unconfirmed Non-Interactive Reclaim

- **WHEN** execution has candidates but standard input is non-interactive and
  neither `--yes` nor `--force` is present
- **THEN** it fails without starting an operation or removing content

### Requirement: Preserve Tombstones and Failure History

Explicit reclaim SHALL persist operation intent before external mutation,
renew its lease during physical removal, and mark the workspace and its
repo-worktrees reclaimed only after physical removal succeeds. It SHALL retain
prior lifecycle history. A partial or failed removal SHALL not mark the
workspace reclaimed and SHALL remain auditable.

#### Scenario: Persist Successful Explicit Reclaim

- **WHEN** physical removal completes successfully
- **THEN** the workspace and repo-worktrees are reclaimed tombstones and the
  explicit reclaim operation is recorded as succeeded

#### Scenario: Preserve Failed Explicit Reclaim

- **WHEN** physical removal fails after the operation starts
- **THEN** Trees records the failed operation and current observed state without
  reporting successful reclamation
