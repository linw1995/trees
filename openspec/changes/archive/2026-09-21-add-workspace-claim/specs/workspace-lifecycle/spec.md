# Spec Delta

## ADDED Requirements

### Requirement: Publish Explicit Claims Atomically

An explicit claim SHALL use per-workspace operation admission and SHALL verify current lease
ownership and claim absence at publication. The new claim, immutable claim event, terminal success
event, successful operation state, and lease removal SHALL commit atomically. Git inspection SHALL
NOT hold a database transaction open. Failure before this commit SHALL leave no new claim; failure
to deliver output after commit SHALL NOT undo the claim. Existing workspace identity, membership,
management mode, pool, and last release timestamp SHALL remain unchanged.

#### Scenario: Reject a Competing Mutation

- **WHEN** claim races with another claim, allocation, addition, release, GC, or removal on the selected workspace
- **THEN** only the admitted operation can publish its result
- **AND** claim cannot publish after losing its lease or after the target becomes ineligible

#### Scenario: Roll Back Failed Publication

- **WHEN** claim insertion or either successful lifecycle event fails inside publication
- **THEN** the entire publication rolls back without a claim or successful outcome

#### Scenario: Retain a Committed Claim After Output Loss

- **WHEN** publication commits but the caller receives no success output
- **THEN** the claim and terminal success remain committed
- **AND** another claim attempt reports already claimed without changing that claim

### Requirement: Defer Recovery During Explicit Claim Admission

As an exception to recovery before relevant mutation, explicit claim SHALL reject any retained
operation lease, including an expired lease, and unresolved mutation state without initiating
recovery of those operations. It SHALL report the blocking operation so the caller can use its
existing recovery workflow. Existing recovery of an interrupted claim operation SHALL preserve
worktrees and SHALL NOT grant a claim. A committed explicit claim SHALL remain active independently
of process liveness.

#### Scenario: Preserve an Interrupted Addition

- **WHEN** the selected workspace has an expired addition operation or unresolved addition journal
- **THEN** claim refuses admission without moving worktrees, compensating the addition, or granting a claim

#### Scenario: Recover an Interruption Before Publication

- **WHEN** a claim process dies before publication and an existing recovery workflow later handles its expired operation
- **THEN** recovery records a terminal failure without deleting user content or creating a claim
- **AND** a later claim can proceed if ordinary eligibility checks pass
