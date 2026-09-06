## ADDED Requirements

### Requirement: Select Automatic Pool Allocation or Manual Provisioning

The CLI SHALL support two creation forms. Automatic creation SHALL be
`trees create --repo <repository-path>...` without a positional workspace path.
It SHALL resolve a repository-set pool backed by a UUID, using a non-unique
hash lookup index and the exact sorted origin repository ID set. Pool identity
SHALL be independent of the workspace root. It SHALL reuse a safe idle automatic
workspace referencing that pool when one exists, regardless of that slot's
persisted root. Otherwise, it SHALL generate a workspace path below the
currently configured Trees-managed workspace root and provision a new
automatic workspace. Manual creation SHALL be `trees create
<workspace-path> --repo <repository-path>...` and SHALL retain the existing
  direct-child worktree structure and detached initial worktree behavior. The
presence of a positional workspace path SHALL be the mode discriminator; no
additional `--mode` flag is required or accepted.

#### Scenario: Allocate an Existing Automatic Workspace

- **WHEN** the caller runs automatic create with repository directories whose
  canonical Git common-directory identities match an idle reusable pool slot
- **THEN** Trees acquires that existing workspace path with a new claim
  identifier without creating another workspace or Git worktree

#### Scenario: Provision a New Automatic Slot

- **WHEN** the caller runs automatic create and no reusable automatic workspace
  matches the exact repository set
- **THEN** Trees generates a path under its currently configured managed
  workspace root, creates the direct-child detached worktrees, records the
  pool UUID, and returns the new workspace with an active claim

#### Scenario: Create a Manual Workspace

- **WHEN** the caller provides an explicit workspace path
- **THEN** the workspace is recorded as `manual`, its worktrees are created
  with the existing create semantics, and automatic pool allocation and GC do
  not select it

#### Scenario: Reject Conflicting Creation Forms

- **WHEN** the caller supplies a mode-selection flag or otherwise attempts to
  combine automatic repository-only arguments with a positional workspace
  path
- **THEN** argument validation fails before any database, Git, or filesystem
  mutation
