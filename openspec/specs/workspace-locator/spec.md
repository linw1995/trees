# Workspace Locator Specification

## Purpose

Define shared workspace lookup and mutually exclusive CLI selection while
preserving command-specific lifecycle and transaction boundaries.

## Requirements

### Requirement: Locate One Workspace

The locator SHALL accept exactly one typed selection strategy: workspace ID,
exact canonical path, containing canonical directory, or claim ID. It SHALL
return the matching workspace and preserve the selected claim ID for a claim
lookup. It SHALL distinguish an absent target from a storage failure using typed
errors and preserve source errors. A dangling claim SHALL be an integrity error.

#### Scenario: Select the Nearest Registered Boundary

- **WHEN** a directory is contained by nested registered workspaces
- **THEN** the nearest registered ancestor is selected, including removed records
- **AND** an ineligible inner workspace does not cause fallback to an outer one

#### Scenario: Keep Exact Paths Exact

- **WHEN** an explicit path is a child of a workspace but is not a registered root
- **THEN** exact-path lookup returns no target

#### Scenario: Preserve Claim Identity

- **WHEN** a workspace is selected by a claim ID
- **THEN** the result retains that exact claim ID for subsequent validation

### Requirement: Share CLI Selection

Status, open, release, and `add` SHALL share named selector definitions, mutual
exclusion, and conversion. Their positional selectors SHALL participate
in the same exclusion rule. The `add` command SHALL accept the three named selectors and an
optional positional workspace directory, defaulting to containing-directory
selection when all selectors are omitted. All conflicts SHALL fail before filesystem or
storage access, including inputs constructed without the command parser.
Explicit ID and claim inputs SHALL NOT require current-directory resolution.
The locator SHALL NOT own transactions, mutate lifecycle state, or enforce
operation eligibility. Callers SHALL retain their existing snapshot and
admission boundaries.

#### Scenario: Reject Redundant Selectors

- **WHEN** two selectors identify even the same workspace
- **THEN** parsing fails instead of applying a precedence rule

#### Scenario: Preserve Read-Only Snapshots

- **WHEN** status or open resolves a workspace and loads related state
- **THEN** those reads occur in the caller's consistent read-only transaction

#### Scenario: Resolve Selections Through Shared Selectors

- **WHEN** the `add` command receives a workspace ID, exact directory, or claim selector
- **THEN** it uses shared conversion and lookup and preserves selected-claim provenance
- **AND** `add` retains responsibility for snapshot consistency, eligibility, recovery, and lease admission

#### Scenario: Reject Conflicting Add Inputs Before Access

- **WHEN** the `add` command receives any two explicit selectors through parsing or direct argument construction
- **THEN** conversion fails before filesystem or storage access even if both identify the same workspace

#### Scenario: Recover Add Without an Existing Workspace Directory

- **WHEN** an interrupted `add` has temporarily moved its root worktree and the caller selects its workspace by ID or active claim ID
- **THEN** lookup returns the persisted workspace without requiring its directory to exist
- **AND** the addition workflow handles recovery before physical eligibility checks

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
