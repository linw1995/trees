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
- **THEN** remove performs origin record removal rather than workspace removal

#### Scenario: Reject Ambiguous Entity Identity

- **WHEN** the same ID exists in both workspace and origin records
- **THEN** remove fails without changing either entity

## ADDED Requirements

### Requirement: Apply Remove Options to Origin Records

For repo IDs, `--dry-run` SHALL report the source path, record-removal action,
and counts of worktree and pool references using read-only storage. Confirmed
execution SHALL reject referenced records and otherwise delete only the origin
row. `--yes` and `--force` SHALL skip confirmation but never override references
or delete source files. Unknown IDs SHALL fail. Workspace options SHALL remain
unchanged.

#### Scenario: Preview Origin Record Removal

- **WHEN** remove receives a repo ID with `--dry-run`
- **THEN** it reports reference counts without database or filesystem mutation

#### Scenario: Force Preserves Referenced Origins

- **WHEN** remove receives a referenced repo ID with `--force`
- **THEN** it fails and preserves the origin record and source files
