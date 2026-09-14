## ADDED Requirements

### Requirement: Admit Additions Under the Existing Workspace Claim

The `add` command SHALL accept manual workspaces without creating a claim and automatic workspaces
only with an active claim. It SHALL snapshot and check again the selected claim during workspace
operation admission, preserve that claim throughout success or failure, and use the existing
per-workspace operation exclusion. A concurrent operation SHALL produce an immediate busy failure
without retrying into a later operation slot. Expired operations SHALL be recovered before a fresh
addition is admitted.

#### Scenario: Reject an Idle Automatic Workspace

- **WHEN** `add` targets an automatic workspace without an active claim
- **THEN** it fails without allocating a claim or changing its repository set

#### Scenario: Reject a Changed Claim

- **WHEN** the selected claim is released or replaced before `add` admission
- **THEN** `add` fails without modifying the later allocation

#### Scenario: Serialize Addition with Other Mutations

- **WHEN** `add` races another `add`, release, removal, acquisition, or integration operation on the same workspace
- **THEN** at most one operation owns the mutation lease and `add` returns busy if it cannot acquire admission

### Requirement: Migrate Expanded Workspaces to the Exact Repository Pool

A successful addition to an automatic workspace SHALL assign it to the pool for the exact union of
its prior active origin IDs and newly added origin IDs, using existing hash lookup and exact
sorted-set verification. Pool creation or reuse, final repository membership, relocated paths, and
the successful `add` lifecycle events SHALL be committed atomically. The prior pool and other
workspaces SHALL retain their repository definitions and membership. Workspace identity, path,
management mode, claim identity and timestamp, and last release timestamp SHALL remain unchanged.
Manual additions SHALL remain manual without entering automatic pools. Failed or rolled-back
additions SHALL retain the prior pool and claim and SHALL NOT become reusable with inconsistent
membership.

#### Scenario: Expand and Reuse an Automatic Workspace

- **WHEN** a claimed `{api}` workspace successfully adds `web` and is later released
- **THEN** it belongs to `{api, web}` and can be reused for that exact set but not for `{api}`

#### Scenario: Reuse an Existing Destination Pool

- **WHEN** the expanded exact repository set already has a pool
- **THEN** `add` references that pool without changing any other workspace or pool definition

#### Scenario: Retain Manual Retention Policy

- **WHEN** repositories are added to a manual workspace
- **THEN** its mode and null pool remain unchanged and it remains excluded from automatic reuse and GC

#### Scenario: Preserve the Prior Pool on Failure

- **WHEN** addition fails or its compensation cannot safely finish
- **THEN** the original claim and pool remain, and unresolved membership prevents release or reuse until repaired
