## MODIFIED Requirements

### Requirement: Remove Idle Automatic Workspaces

The CLI SHALL provide `trees gc --older-than <duration> [--dry-run] [--yes]
[--force]`. GC SHALL calculate a UTC cutoff from the current time minus the
supplied duration and consider only non-removed `automatic` workspaces. The idle
timestamp SHALL be the last successful release time, or `created_at` when the
workspace has never been released.
Each candidate SHALL use `canonical_path.parent()` for root-containment and
unexpected-content checks; the current configuration root SHALL not filter
reuse or GC candidates.
GC SHALL report the total automatic workspaces, the number currently not
claimed, the number currently claimed, the number older than the
cutoff, and the number selected for removal. A normal non-dry-run SHALL
request confirmation before mutation unless `--yes` or `--force` is supplied.
`--yes` SHALL skip only the confirmation and SHALL retain the normal safety
filter. A dry run SHALL perform read-only inspection and SHALL NOT modify Git,
SQLite, or the filesystem.

The `--force` policy SHALL be scoped to the current GC invocation and SHALL
not be persisted as candidate or workspace snapshot state.

Without `--force`, GC SHALL select only workspaces with no active operation,
no claim, `ready` health, clean reusable worktrees, and no unexpected root
content. `--force` SHALL imply `--yes` and permit cleanup of age-qualified
automatic workspaces with dirty, diverged, missing, prunable, or unexpected
content. `--force` SHALL still refuse manual workspaces, young workspaces,
active claims, active operations, and paths whose source repository identity
cannot be verified. Both modes SHALL refuse a worktree that is branch-attached
or not detached. Forced cleanup SHALL record that it was forced.

#### Scenario: Preview GC Candidates Safely

- **WHEN** a caller runs `trees gc --older-than 30d --dry-run`
- **THEN** the command reports automatic, unclaimed, claimed,
  age-qualified, safe-to-remove, and selected counts in a table, followed by
  candidate reasons, repositories, cached sizes, idle times, and stable IDs,
  without removing worktrees, directories, claims, rows, or events

#### Scenario: Confirm a Normal GC Run

- **WHEN** a caller runs a non-dry-run GC without `--yes` or `--force` and the
  scan finds removable automatic workspaces
- **THEN** the command displays how many workspaces are currently unclaimed
  and how many will be removed, and performs no mutation until the caller
  confirms

#### Scenario: Skip Confirmation Without Forcing Cleanup

- **WHEN** a caller runs `trees gc --older-than 30d --yes`
- **THEN** the command displays the same counts, performs no interactive
  prompt, and removes only the normal clean automatic candidates

#### Scenario: Require Authorization Without Confirmation

- **WHEN** a caller runs a non-dry-run GC without confirmation, `--yes`, or
  `--force`
- **THEN** the command fails before mutation and instructs the caller to use
  `--dry-run`, `--yes`, or `--force`

#### Scenario: Never Collect a Manual Workspace

- **WHEN** an automatic GC scan finds an idle manual workspace older than the
  cutoff
- **THEN** it excludes the workspace from candidates and counts and does not invoke
  any Git or filesystem removal for that workspace

#### Scenario: Remove an Idle Workspace

- **WHEN** an automatic workspace is older than the cutoff, has no active
  claim or operation, and final reconciliation confirms every worktree is
  clean, detached, present, non-prunable, identity-matched, and at its
  recorded revision
- **THEN** after confirmation GC removes each worktree without a force flag,
  removes the empty workspace directory, marks the workspace and
  repo-worktree snapshots as `removed`, and records a successful GC
  operation and removal event

#### Scenario: Preserve an Unsafe GC Candidate

- **WHEN** a candidate is dirty, diverged, missing, claimed, or the
  workspace root contains an unexpected entry
- **THEN** normal GC skips or fails that candidate, reports the reason, and
  leaves every remaining file and worktree association in place

#### Scenario: Force Remove an Unsafe Automatic Workspace

- **WHEN** a caller runs `trees gc --older-than 30d --force` and an
  age-qualified automatic workspace has dirty or diverged worktrees, or
  unexpected content below its managed root
- **THEN** GC may use forced worktree removal and remove that workspace's
  unexpected content, records `forced: true`, and marks the workspace
  `removed` only after all requested physical removal succeeds

#### Scenario: Keep Force Within Ownership and Identity Boundaries

- **WHEN** a forced GC scan encounters a manual workspace, an active claim, an
  active operation, a young workspace, or a path whose source repository
  identity cannot be verified
- **THEN** it skips the workspace, reports the reason, and does not remove its
  files or worktree metadata

#### Scenario: Record a Partial GC Failure

- **WHEN** one worktree is removed but a later worktree or the empty workspace
  root cannot be removed
- **THEN** GC stops, records the partial removal and failure details, marks
  the workspace degraded or failed as appropriate, and does not report the
  workspace as successfully removed

## ADDED Requirements

### Requirement: Present GC Decisions as Workspace Tables

The CLI SHALL render GC scans and execution summaries using the shared workspace
table layout. Candidate rows SHALL contain `REASON`, `REPOS`, `SIZE`,
`IDLE SINCE`, and `ID`, without workspace paths. Repository and cached-size cells
SHALL use workspace status formatting. Display metadata SHALL be read only and
SHALL NOT affect core GC scan or execution APIs.

#### Scenario: Explain a Retained Workspace

- **WHEN** an automatic workspace is young, claimed, or unsafe to remove
- **THEN** its row includes the reason and repository summary even though it
  will not be removed

#### Scenario: Display a Cached Workspace Size

- **WHEN** GC previews a workspace with cached disk usage
- **THEN** its size cell uses the same binary units and completeness markers as
  workspace status, with `unknown` for an unmeasured workspace
- **AND** the preview does not refresh the cache or traverse files for size

#### Scenario: Omit Removed History

- **WHEN** stored history includes removed automatic workspaces
- **THEN** those workspaces appear in neither candidate rows nor scan counts
- **AND** an empty current inventory reports no automatic workspaces

#### Scenario: Report the Current Execution

- **WHEN** a confirmed GC execution finishes
- **THEN** its summary reports current unclaimed, claimed, removed, skipped, and
  failed counts
- **AND** skipped counts retained candidates from that execution, while removed
  counts only successful removals from that execution
