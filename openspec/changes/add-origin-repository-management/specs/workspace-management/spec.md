## ADDED Requirements

### Requirement: Resolve Paths URLs and Directory Names in Repo Inputs

`trees create [WORKSPACE_PATH] --repo <PATH|URL|NAME>...` SHALL accept local paths,
supported remote URLs, and unambiguous registered directory base names. Explicit
absolute, dot-relative, and Windows drive paths SHALL be paths. Supported URI
schemes and `SCP`-style host paths SHALL be URLs. Other inputs SHALL be paths;
a nonexistent single-component relative path SHALL resolve by registered
primary directory base name only when exactly one origin matches. Existing local
directories SHALL take precedence. NAME SHALL match the exact stored primary
directory base name across registered manual and automatic origins and reuse
that origin without cloning a new source. Unknown or ambiguous names SHALL fail.
Create SHALL retain at least one required repo input and SHALL NOT introduce
an origin selector option or separate repository command group.

Local inputs SHALL register or reuse origins; URLs SHALL provision or reuse
automatic origins. Both workspace modes SHALL use existing layout, pool,
revision, source-preservation, open, and JSON rules after input resolution.
All inputs SHALL be parsed and predictable local errors rejected before cloning.
Duplicate identities SHALL fail before workspace creation. Successfully
published origins SHALL survive later failure, while partial workspace setup
SHALL follow existing rollback behavior.

#### Scenario: Create Directly from a Remote URL

- **WHEN** either create mode receives an unknown remote URL through `--repo`
- **THEN** Trees provisions an automatic source and creates or allocates the workspace from it

#### Scenario: Mix a Local Checkout and a Remote

- **WHEN** create receives a local checkout and a distinct remote URL
- **THEN** both sources participate in one workspace and retain their own management modes

#### Scenario: Resolve a Directory Name

- **WHEN** `api` is not an existing relative path and exactly one registered source has base name `api`
- **THEN** `--repo` `api` selects that source identity

#### Scenario: Reject an Ambiguous Directory Name

- **WHEN** a bare name matches multiple registered sources and no local path takes precedence
- **THEN** create fails and identifies paths the caller can use explicitly

#### Scenario: Preserve Explicit Path Interpretation

- **WHEN** the caller prefixes a colon-bearing local path with `./`
- **THEN** create treats it as a path and never invokes remote provisioning for it

#### Scenario: Reject Duplicate Resolved Origins

- **WHEN** path, URL, or directory-name inputs resolve to the same Git common directory
- **THEN** create fails before workspace mutation

#### Scenario: Respect Offline Mode

- **WHEN** `--offline` receives an unknown URL requiring a clone
- **THEN** create fails before any cloning or workspace mutation
- **AND** a known valid automatic origin remains usable offline at its primary local HEAD

#### Scenario: Preserve Existing Path-Based Behavior

- **WHEN** create uses only local paths
- **THEN** existing manual and automatic workspace selection, default fetch, offline, layout, and opening behavior remain unchanged

#### Scenario: Reject an Unknown Name

- **WHEN** a bare NAME is neither an existing local path nor a registered source base name
- **THEN** create reports no matching repository without attempting a clone

#### Scenario: Look up Either Management Mode by Name

- **WHEN** NAME uniquely matches a registered manual or automatic source
- **THEN** create reuses its stored identity and retains its management mode
