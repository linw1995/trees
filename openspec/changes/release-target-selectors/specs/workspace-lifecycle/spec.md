## MODIFIED Requirements

### Requirement: Persist Active Workspace Claims

In addition to the workspace health snapshot, the system SHALL persist the
current usage claim in a `workspace_claims` table. The table SHALL contain at
most one row for each workspace, with a UUID v7 claim identifier, workspace
foreign key, and claim timestamp. A workspace with no active
claim is unclaimed; a workspace with an active claim is claimed. The claim
records persistent usage state for the workspace. It is not a database
transaction or a database lock and remains until the caller releases it. Access
availability SHALL remain independent from
`WorkspaceState` so a degraded workspace cannot become an eligible reusable
workspace merely by having no claim.

#### Scenario: Create an Active Claim

- **WHEN** a reusable workspace is successfully acquired
- **THEN** SQLite contains exactly one active claim for its workspace ID and
  the claim records the returned claim identifier

#### Scenario: Release an Active Claim

- **WHEN** a release target resolves an active claim and release succeeds
- **THEN** the selected active claim row is removed atomically with the
  terminal operation and release event, while workspace and repo-worktree
  identities remain intact

### Requirement: Serialize Access Operations with Workspace Operations

Acquire and release SHALL use the existing per-workspace operation exclusion.
A request SHALL NOT replace an active claim or run concurrently with another
non-terminal workspace operation. Release SHALL attempt admission once and
return busy immediately when admission is unavailable; it SHALL NOT wait or
retry into a later operation slot. Claim changes, operation lease changes, and
access lifecycle events SHALL use the existing short Diesel transaction
boundaries. Git and filesystem work SHALL occur outside those transactions.

#### Scenario: Reject Access During an Active Operation

- **WHEN** a workspace has a non-terminal operation or an active claim with a
  different claim identifier
- **THEN** the access request fails without changing Git or the active claim

#### Scenario: Do Not Serialize Concurrent Releases

- **WHEN** two release requests concurrently target the same workspace
- **THEN** at most one request starts a release operation and the other exits
  busy without waiting for the first request to finish
