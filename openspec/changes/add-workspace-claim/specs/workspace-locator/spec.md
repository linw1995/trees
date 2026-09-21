# Spec Delta

## ADDED Requirements

### Requirement: Select an Explicit Claim Target

The claim command SHALL accept an optional positional workspace directory or `--workspace-id`, and
SHALL default to the nearest registered workspace containing the canonical current directory when
neither is supplied. Positional paths SHALL select exact canonical roots, resolving relative paths
against the current directory. Claim SHALL NOT expose `--workspace-dir` or `--claim-id`. Selector
conflicts SHALL fail before filesystem or database access, including directly constructed inputs.
Explicit workspace IDs SHALL NOT require current-directory resolution. The command SHALL use shared
locator semantics without changing existing command selectors.

#### Scenario: Claim from a Descendant

- **WHEN** `trees claim` runs from a directory below a registered workspace
- **THEN** it selects the nearest registered ancestor
- **AND** an ineligible inner workspace does not cause fallback to an outer workspace

#### Scenario: Select an Exact Root

- **WHEN** a positional path resolves to a child directory rather than a registered root
- **THEN** claim reports no matching workspace instead of selecting its ancestor

#### Scenario: Select by Workspace Identifier

- **WHEN** `trees claim --workspace-id WORKSPACE_ID` receives a valid identifier
- **THEN** it locates the persisted workspace without resolving the current directory
- **AND** command eligibility still verifies the selected workspace's physical structure

#### Scenario: Reject Redundant or Unsupported Selectors

- **WHEN** claim receives both a positional path and an ID, or either unsupported named selector
- **THEN** argument parsing fails before any workspace access

#### Scenario: Reject Invalid Direct Inputs

- **WHEN** directly constructed claim arguments contain both a path and an ID
- **THEN** selector conversion rejects the conflict before filesystem or database access
