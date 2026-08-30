## ADDED Requirements

### Requirement: Select Automatic Pool Allocation or Manual Provisioning

The CLI SHALL support two creation forms. Automatic creation SHALL be
`trees create --repo <repository-path>...` without a positional workspace path.
It SHALL derive a repository-set pool key, reuse a safe idle automatic
workspace with the exact key when one exists, and otherwise generate a
workspace path below the Trees-managed workspace root and provision a new
automatic workspace. Manual creation SHALL be `trees create
<workspace-path> --repo <repository-path>...` and SHALL retain the existing
direct-child worktree layout and detached initial checkout behavior. The
presence of a positional workspace path SHALL be the mode discriminator; no
additional `--mode` flag is required or accepted.

#### Scenario: Allocate an Existing Automatic Workspace

- **WHEN** the caller runs automatic create with repository directories whose
  canonical Git common-directory identities match an idle reusable pool slot
- **THEN** Trees returns that existing workspace path with a new checkout
  identifier without creating another workspace or Git worktree

#### Scenario: Provision When the Pool Has No Safe Slot

- **WHEN** the caller runs automatic create and no reusable automatic workspace
  matches the exact repository set
- **THEN** Trees generates a path under its managed workspace root, creates the
  direct-child detached worktrees, records the pool key, and returns the new
  workspace already checked out

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
