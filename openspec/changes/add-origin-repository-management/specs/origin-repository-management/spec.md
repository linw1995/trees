## Purpose

Manage source repositories through existing commands using their existing Git
identity and path, without introducing persistent repository management modes.

## ADDED Requirements

### Requirement: Reuse Existing Origin Records

Origins SHALL retain only their existing ID, canonical Git common-directory
identity, and primary source path. Local registration and automatic cloning
SHALL converge on the same origin model. Registration SHALL reuse an existing
ID for the same identity. No management mode, soft-delete state, source root,
or remote URL SHALL be added to origin records. Directory placement SHALL NOT
imply ownership or deletion policy.

#### Scenario: Register a Local Source Inside the Clone Root

- **WHEN** create receives a local source inside the configured clone root
- **THEN** it registers the same kind of origin record as any other local or cloned source

#### Scenario: Reuse a Linked Worktree Input

- **WHEN** a local input resolves to an existing origin through a linked worktree
- **THEN** it retains that origin ID and primary source path

### Requirement: Configure Future Clone Placement

`trees config set origins-dir <PATH>` SHALL store `repository.origins_dir`,
with the existing platform default. Relative paths SHALL resolve against the
configuration file directory. The setting SHALL affect future allocations
without moving existing sources. Local registration SHALL NOT create the root.

#### Scenario: Change the Root

- **WHEN** the root is changed before cloning a new URL
- **THEN** the new clone uses that root and existing sources remain in place

### Requirement: Reuse Sources Through Git Remote Configuration

URL lookup SHALL inspect local, readable, identity-matching origin repositories
and compare their Git `remote.origin.url` values exactly. A unique matching
source SHALL be reused regardless of creation method. Multiple matching origins
SHALL fail with candidate paths. Missing or identity-mismatched origins SHALL
not match but SHALL retain their records. Unknown URLs SHALL be cloned into
an exclusive contained destination and registered after usable HEAD validation.
Remote configuration changes SHALL take effect without updating origin rows.

#### Scenario: Reuse a Manually Registered Source by URL

- **WHEN** one existing source has the requested Git remote URL
- **THEN** create reuses its ID and pool without cloning

#### Scenario: Observe a Changed Remote

- **WHEN** the user changes a source's Git remote URL
- **THEN** subsequent URL lookup uses the changed configuration

#### Scenario: Reject Ambiguous URLs

- **WHEN** two recorded sources have the requested URL
- **THEN** create fails with their paths instead of choosing one silently

### Requirement: Recover Partial Clone Operations

The clone intent SHALL be persisted separately from origin records before file
creation. Per-URL locks SHALL serialize provisioning and the lookup SHALL be
repeated after acquiring the lock. The cleanup SHALL affect only proven operation
files. Successful publication SHALL consume pending intent; published sources
SHALL survive subsequent workspace failure. Recovery SHALL retain evidence
when ownership cannot be proven and SHALL NOT take over live operations.

#### Scenario: Recover an Interrupted Clone

- **WHEN** create retries a URL with abandoned operation intent
- **THEN** it acquires the lock and recovers only owned partial files

#### Scenario: Preserve a Published Source

- **WHEN** workspace creation fails after an origin was published
- **THEN** its files and origin ID remain available for retry

### Requirement: Remove Only Origin Records Without References

Origin removal SHALL delete the database row only when no repo-worktrees or
pool memberships reference it. All retained references SHALL count, including
historical ones. Source files SHALL remain untouched. Checks SHALL be repeated
within a transaction before deletion. Force SHALL NOT bypass reference guards.
A deleted ID SHALL be unknown; subsequent registration SHALL allocate a new ID.

#### Scenario: Reject a Referenced Origin

- **WHEN** remove targets an origin with any worktree or pool reference
- **THEN** it reports the references and changes neither the origin nor source files

#### Scenario: Remove an Origin Without References

- **WHEN** confirmed removal targets an origin with no references
- **THEN** its row is deleted and its source files remain intact
