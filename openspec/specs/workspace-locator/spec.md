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

Status, open, and release SHALL share named selector definitions, mutual
exclusion, and conversion. Their legacy positional selectors SHALL participate
in the same exclusion rule. All conflicts SHALL fail before filesystem or
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
