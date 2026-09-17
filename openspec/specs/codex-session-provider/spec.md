# Codex Session Provider Specification

## Purpose

Provide real Codex conversation metadata through the existing executable workspace session hook without changing agent or workspace storage.

## Requirements

### Requirement: Execute as an Offline UV Script

The provider SHALL be directly executable through `uv` with inline Python metadata, require Python 3.11 or newer, and use no third-party Python dependencies. It SHALL consume and produce the existing version-1 hook JSON protocol without extra command-line arguments or network access.

#### Scenario: Execute a Batch

- **WHEN** Trees sends workspace IDs and absolute paths to the executable
- **THEN** it returns a session list for every requested ID in one JSON document

### Requirement: Read Codex Metadata Without Mutation

The provider SHALL resolve the Codex home and configured SQLite home as documented, select the highest numeric state database version, and open it in read-only mode. Missing storage SHALL produce empty session lists. Unsupported schemas and read failures SHALL cause a nonzero exit with diagnostics only on standard error. The provider SHALL NOT create, migrate, or repair agent storage.

#### Scenario: Detect Incompatible Storage

- **WHEN** the newest database lacks required thread metadata columns
- **THEN** the provider fails without falling back to an older database or emitting partial JSON

#### Scenario: Inspect Missing Storage

- **WHEN** no versioned state database exists
- **THEN** the provider returns empty lists without creating files

### Requirement: Select and Order Workspace Sessions

The provider SHALL match canonical session working directories exactly to requested workspace roots,
exclude archived sessions and child-agent sources, and order sessions by update time descending then
ID descending. It SHALL prefer database names, indexed names, stored titles, previews, first user
messages, and `Untitled session`, in that order. Name-index lookup SHALL use the last valid nonempty
name for each ID and ignore malformed lines. Timestamps SHALL be returned in RFC 3339 UTC form,
supporting available millisecond precision.

#### Scenario: Preserve a Named Session

- **WHEN** a session has both a database name and an indexed or stored title
- **THEN** its returned title uses the database name

#### Scenario: Avoid Incorrect Workspace Attribution

- **WHEN** a session directory is a child of a requested root or merely shares its string prefix
- **THEN** it is excluded unless that directory is independently requested as an exact root

#### Scenario: Follow Renames and Updates

- **WHEN** a database without names has several indexed names for a session
- **THEN** the last valid name is returned and list order still follows the database update time
