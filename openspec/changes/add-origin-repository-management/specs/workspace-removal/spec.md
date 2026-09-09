## MODIFIED Requirements

### Requirement: Remove One Workspace by Stable Id

The CLI SHALL provide `trees remove <workspace-or-repo-id> [--dry-run] [--yes] [--force]`. The ID SHALL be a full UUID v7 identifier. The command SHALL resolve it
against workspace and origin records, dispatch exactly one match, and reject
unknown or cross-entity ambiguous IDs without mutation. Workspace removal
SHALL accept automatic and manual workspaces and SHALL NOT apply an idle-age
threshold.

#### Scenario: Remove a Manual Workspace

- **WHEN** a caller confirms removal for a safe manual workspace by its ID
- **THEN** Trees removes the workspace's managed worktrees and directory and
  records reclaimed tombstones

#### Scenario: Remove an Automatic Workspace

- **WHEN** a caller confirms removal for a safe unclaimed automatic workspace
- **THEN** Trees removes that workspace without evaluating its release age

#### Scenario: Dispatch an Origin Identifier

- **WHEN** the ID matches only an origin repository
- **THEN** remove performs repository registration removal rather than workspace removal

#### Scenario: Reject Ambiguous Entity Identity

- **WHEN** the same ID exists in both workspace and origin records
- **THEN** remove fails without changing either entity

## ADDED Requirements

### Requirement: Apply Remove Options to Repository Targets

For repo targets, `--dry-run` SHALL use read-only storage and report entity type,
path, and the registration removal action without mutation or recovery. Execution
SHALL use the existing confirmation policy, explicitly identifying repository
registration removal. Neither `--yes` nor `--force` SHALL authorize source file deletion;
`--force` SHALL only imply confirmation for repository targets. Workspace option
semantics SHALL remain unchanged. Already unregistered origins SHALL succeed
without additional changes.

#### Scenario: Preview Repository Registration Removal

- **WHEN** remove receives a repo ID with `--dry-run`
- **THEN** it reports repository registration removal and preserves both database and source files

#### Scenario: Force Does Not Delete an Origin

- **WHEN** remove receives an automatic repo ID with `--force`
- **THEN** it skips confirmation and removes registration for the origin while preserving source files and references
