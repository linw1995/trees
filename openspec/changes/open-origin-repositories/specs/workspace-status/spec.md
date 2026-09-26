## ADDED Requirements

### Requirement: Opening Registered Origins

The open command SHALL accept a registered origin repository ID as its positional
argument and start the selected program in the stored source path. Named
workspace selectors SHALL continue to resolve only workspaces. Opening SHALL
remain read-only and SHALL NOT clone or fetch repositories.

#### Scenario: Open a Source Repository

- **WHEN** a positional ID identifies only a registered origin
- **THEN** the command starts the explicit program or default shell in its source path
- **AND** the repository identity path is not used as the working directory

#### Scenario: Ambiguous Identifiers

- **WHEN** a positional ID identifies both a workspace and an origin
- **THEN** the command fails with an ambiguity error
- **AND** an explicit workspace ID selector can still select the workspace

#### Scenario: Reject a Missing Source Directory

- **WHEN** the registered source directory is missing
- **THEN** program startup fails without deleting or repairing the origin record
