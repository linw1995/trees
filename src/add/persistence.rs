use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use snafu::{ensure, ResultExt};

use super::*;
use crate::domain::{JsonDocument, LeaseId, OperationState, RepoWorktreeState, Timestamp};
use crate::schema::{lifecycle_events, operations, repo_worktrees, workspaces};
use crate::storage::{EventDraft, OperationIntent, TransitionMetadata};

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestIntent {
    pub version: u32,
    pub repositories: Vec<PathBuf>,
    pub offline: bool,
    pub claim_id: Option<ClaimId>,
    pub previous_pool_id: Option<PoolId>,
    pub recovery_of: Option<OperationId>,
}

pub fn admit(
    db: &mut SqliteConnection,
    target: &AddTarget,
    request: &AddRequest,
    recovery_of: Option<OperationId>,
) -> Result<OperationIntent, AddError> {
    let intent = OperationIntent::new(
        target.workspace.id,
        "add",
        Timestamp::after_seconds(300),
        "resolve repositories",
        document(&RequestIntent {
            version: 1,
            repositories: request.repositories.clone(),
            offline: request.offline,
            claim_id: target.claim_id,
            previous_pool_id: target.workspace.pool_id,
            recovery_of,
        })?,
    );
    let result = storage::with_trying_short_transaction(db, |db| {
        let current = storage::find_workspace(db, &target.workspace.id)?;
        let claim = storage::find_workspace_claim(db, &target.workspace.id)?.map(|row| row.id);
        if current.state == WorkspaceState::Removed
            || current.pool_id != target.workspace.pool_id
            || claim != target.claim_id
            || (current.management_mode == WorkspaceManagementMode::Automatic && claim.is_none())
        {
            return Err(diesel::result::Error::RollbackTransaction);
        }
        storage::persist_operation_intent(db, &intent)?;
        Ok(())
    });
    match result {
        Ok(()) => Ok(intent),
        Err(diesel::result::Error::RollbackTransaction) => ClaimChangedSnafu.fail(),
        Err(diesel::result::Error::DatabaseError(kind, info))
            if kind == diesel::result::DatabaseErrorKind::UniqueViolation
                || info.message().contains("locked")
                || info.message().contains("busy") =>
        {
            BusySnafu.fail()
        }
        Err(source) => Err(source.into()),
    }
}

pub fn renew(db: &mut SqliteConnection, lease: &LeaseId) -> Result<(), crate::git::GitError> {
    match storage::renew_operation_lease(db, lease)
        .map_err(crate::git::GitError::from_heartbeat_source)?
    {
        true => Ok(()),
        false => Err(crate::git::GitError::Heartbeat {
            message: "addition lease is no longer owned".to_owned(),
        }),
    }
}

pub fn event(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    kind: &str,
    details: JsonDocument,
) -> Result<(), AddError> {
    storage::with_short_transaction(db, |db| {
        let owned = storage::repository::operation_lease_for_mutation(db, lease)?;
        storage::append_event(
            db,
            &EventDraft {
                operation_id: owned.operation_id,
                entity_type: "workspace".to_owned(),
                entity_id: owned.workspace_id.to_string(),
                event_type: kind.to_owned(),
                source: "trees".to_owned(),
                occurred_at: Timestamp::now(),
                previous_state: None,
                current_state: None,
                details_json: Some(details),
                error_json: None,
            },
        )?;
        Ok(())
    })?;
    Ok(())
}

pub fn step(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    name: &str,
    details: JsonDocument,
) -> Result<(), AddError> {
    storage::persist_operation_step_intent(db, lease, name, details)?;
    Ok(())
}

pub fn load_plan(
    db: &mut SqliteConnection,
    operation: &OperationId,
) -> Result<Option<AddPlan>, AddError> {
    let events = storage::list_events_for_operation(db, operation)?;
    let row = events
        .iter()
        .find(|event| event.event_type == "workspace_add_planned");
    row.map(|event| {
        let details = event
            .details_json
            .as_ref()
            .ok_or_else(|| JournalSnafu.build())?;
        let plan: AddPlan = serde_json::from_str(&details.to_string()).context(DecodeSnafu)?;
        ensure!(plan.version == 1, JournalSnafu);
        Ok(plan)
    })
    .transpose()
}

pub fn has_event(
    db: &mut SqliteConnection,
    operation: &OperationId,
    kind: &str,
) -> Result<bool, AddError> {
    Ok(storage::list_events_for_operation(db, operation)?
        .iter()
        .any(|event| event.event_type == kind))
}

pub fn unresolved(
    db: &mut SqliteConnection,
    workspace: &WorkspaceId,
) -> Result<Option<OperationId>, AddError> {
    let latest = lifecycle_events::table
        .filter(lifecycle_events::entity_id.eq(workspace.to_string()))
        .filter(
            lifecycle_events::event_type
                .eq_any(["workspace_add_unresolved", "workspace_add_resolved"]),
        )
        .order((
            lifecycle_events::occurred_at.desc(),
            lifecycle_events::event_id.desc(),
        ))
        .select((lifecycle_events::operation_id, lifecycle_events::event_type))
        .first::<(OperationId, String)>(db)
        .optional()?;
    Ok(latest.and_then(|(id, kind)| (kind == "workspace_add_unresolved").then_some(id)))
}

pub fn ensure_resolved(db: &mut SqliteConnection, workspace: &WorkspaceId) -> Result<(), AddError> {
    ensure!(unresolved(db, workspace)?.is_none(), UnresolvedSnafu);
    Ok(())
}

pub fn active_repositories(
    db: &mut SqliteConnection,
    workspace: &WorkspaceId,
) -> Result<Vec<storage::RepoWorktreeRow>, AddError> {
    Ok(storage::list_repo_worktrees(db, workspace)?
        .into_iter()
        .filter(|row| row.state != RepoWorktreeState::Removed)
        .collect())
}

pub fn finish(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    plan: &AddPlan,
    observed: &[(RepoWorktreeId, RepoWorktreeState)],
    recovered: bool,
) -> Result<Option<PoolId>, AddError> {
    db.immediate_transaction(|db| {
        let owned = storage::repository::operation_lease_for_mutation(db, lease)?;
        ensure!(owned.workspace_id == plan.workspace_id, JournalSnafu);
        let workspace = storage::find_workspace(db, &plan.workspace_id)?;
        ensure!(
            workspace.pool_id == plan.previous_pool_id,
            ClaimChangedSnafu
        );
        let claim = storage::find_workspace_claim(db, &plan.workspace_id)?.map(|row| row.id);
        ensure!(claim == plan.claim_id, ClaimChangedSnafu);
        for repository in &plan.additions {
            storage::insert_repo_worktree(
                db,
                &storage::NewRepoWorktree {
                    id: repository.worktree_id,
                    workspace_id: plan.workspace_id,
                    origin_repository_id: repository.origin_repository_id,
                    worktree_path: repository.worktree_path.clone(),
                    state: RepoWorktreeState::Attached,
                    last_head: Some(repository.head.clone()),
                    last_observed_at: Timestamp::now(),
                },
            )?;
            storage::record_repo_worktree_transition(
                db,
                &repository.worktree_id,
                &owned.operation_id,
                RepoWorktreeState::Attached,
                Some(repository.head.clone()),
                TransitionMetadata::new("worktree_added", "trees")
                    .with_details(document(repository)?),
            )?;
        }
        if let Some(relocation) = &plan.relocation {
            diesel::update(repo_worktrees::table.find(relocation.worktree_id))
                .set(repo_worktrees::worktree_path.eq(&relocation.worktree_path))
                .execute(db)?;
        }
        for (id, state) in observed {
            let repo = plan
                .existing
                .iter()
                .chain(&plan.additions)
                .find(|repo| repo.worktree_id == *id)
                .ok_or_else(|| JournalSnafu.build())?;
            storage::record_repo_worktree_transition(
                db,
                id,
                &owned.operation_id,
                *state,
                Some(repo.head.clone()),
                TransitionMetadata::new("worktree_observed", "trees"),
            )?;
        }
        let pool_id = if workspace.management_mode == WorkspaceManagementMode::Automatic {
            let ids = plan
                .existing
                .iter()
                .chain(&plan.additions)
                .map(|repo| repo.origin_repository_id)
                .collect::<Vec<_>>();
            let pool = storage::ensure_workspace_pool(
                db,
                &crate::pool::RepositorySetKey::from_repository_ids(&ids),
            )?;
            storage::insert_workspace_pool_repositories(
                db,
                &ids.into_iter()
                    .map(|repository_id| storage::NewWorkspacePoolRepository {
                        pool_id: pool.id,
                        repository_id,
                    })
                    .collect::<Vec<_>>(),
            )?;
            diesel::update(workspaces::table.find(plan.workspace_id))
                .set(workspaces::pool_id.eq(pool.id))
                .execute(db)?;
            Some(pool.id)
        } else {
            workspace.pool_id
        };
        let health = if observed
            .iter()
            .all(|(_, state)| *state == RepoWorktreeState::Attached)
        {
            WorkspaceState::Ready
        } else {
            WorkspaceState::Degraded
        };
        storage::record_workspace_transition(
            db,
            &plan.workspace_id,
            &owned.operation_id,
            health,
            TransitionMetadata::new("workspace_repositories_added", "trees").with_details(
                document(&json!({
                    "plan": plan, "previous_pool_id": plan.previous_pool_id, "pool_id": pool_id,
                    "claim_id": plan.claim_id,
                }))?,
            ),
        )?;
        storage::record_operation_transition(
            db,
            lease,
            OperationState::Succeeded,
            TransitionMetadata::new(
                if recovered {
                    "operation_recovered"
                } else {
                    "operation_succeeded"
                },
                "trees",
            ),
        )?;
        Ok(pool_id)
    })
}

pub fn terminal(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    state: OperationState,
    error: Option<&AddError>,
) -> Result<(), AddError> {
    let mut metadata = TransitionMetadata::new(
        match state {
            OperationState::RolledBack => "operation_rolled_back",
            OperationState::Failed => "operation_failed",
            _ => "operation_succeeded",
        },
        "trees",
    );
    if let Some(error) = error {
        metadata = metadata.with_error(document(&json!({"error": error.to_string()}))?);
    }
    storage::record_operation_transition(db, lease, state, metadata)?;
    Ok(())
}

pub fn retain_residuals(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    plan: &AddPlan,
    remaining: &[RepositoryPlan],
    error: &AddError,
) -> Result<(), AddError> {
    db.immediate_transaction(|db| {
        let owned = storage::repository::operation_lease_for_mutation(db, lease)?;
        for repo in remaining {
            diesel::insert_into(repo_worktrees::table)
                .values(&storage::NewRepoWorktree {
                    id: repo.worktree_id,
                    workspace_id: plan.workspace_id,
                    origin_repository_id: repo.origin_repository_id,
                    worktree_path: repo.worktree_path.clone(),
                    state: RepoWorktreeState::Failed,
                    last_head: Some(repo.head.clone()),
                    last_observed_at: Timestamp::now(),
                })
                .on_conflict(repo_worktrees::id)
                .do_update()
                .set(repo_worktrees::state.eq(RepoWorktreeState::Failed))
                .execute(db)?;
        }
        event(db, lease, "workspace_add_unresolved", document(plan)?)?;
        storage::record_workspace_transition(
            db,
            &plan.workspace_id,
            &owned.operation_id,
            WorkspaceState::Degraded,
            TransitionMetadata::new("workspace_add_failed", "trees")
                .with_error(document(&json!({"error": error.to_string()}))?),
        )?;
        terminal(db, lease, OperationState::Failed, Some(error))
    })
}

pub fn compensated(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    plan: &AddPlan,
) -> Result<(), AddError> {
    db.immediate_transaction(|db| {
        for repo in &plan.additions {
            // These are failed, unpublished additions; their full history remains in the journal.
            diesel::delete(
                repo_worktrees::table
                    .find(repo.worktree_id)
                    .filter(repo_worktrees::state.eq(RepoWorktreeState::Failed)),
            )
            .execute(db)?;
        }
        event(db, lease, "workspace_add_resolved", document(plan)?)?;
        terminal(db, lease, OperationState::RolledBack, None)
    })
}

pub fn request_intent(
    db: &mut SqliteConnection,
    id: &OperationId,
) -> Result<RequestIntent, AddError> {
    let value = operations::table
        .find(id)
        .select(operations::intent_json)
        .first::<JsonDocument>(db)?;
    serde_json::from_str(&value.to_string()).context(DecodeSnafu)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (SqliteConnection, AddTarget, AddRequest) {
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let path = CanonicalPath::from_absolute("/workspace").unwrap();
        let workspace = storage::insert_workspace(
            &mut db,
            &storage::NewWorkspace {
                id: WorkspaceId::new(),
                canonical_path: path.clone(),
                state: WorkspaceState::Ready,
                created_at: Timestamp::now(),
                updated_at: Timestamp::now(),
                last_reconciled_at: None,
            },
        )
        .unwrap();
        (
            db,
            AddTarget {
                workspace,
                claim_id: None,
            },
            AddRequest {
                selector: WorkspaceSelector::ExactPath(path),
                repositories: vec![PathBuf::from("api")],
                offline: true,
            },
        )
    }

    #[test]
    fn admission_is_exclusive_and_intent_remains_immutable() {
        let (mut db, target, request) = fixture();
        let intent = admit(&mut db, &target, &request, None).unwrap();
        assert!(matches!(
            admit(&mut db, &target, &request, None),
            Err(AddError::Busy)
        ));
        step(
            &mut db,
            &intent.lease_id,
            "prepare",
            document(&json!({"path": "/workspace/api"})).unwrap(),
        )
        .unwrap();
        terminal(&mut db, &intent.lease_id, OperationState::Failed, None).unwrap();
        let row = storage::find_operation(&mut db, &intent.id).unwrap();
        assert_eq!(row.intent_json.to_string(), intent.intent_json.to_string());
        assert_eq!(
            storage::operation_state(&mut db, &intent.id).unwrap(),
            Some(OperationState::Failed)
        );
        assert!(
            storage::find_running_operation(&mut db, &target.workspace.id)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn stale_claim_is_rejected_before_an_operation_is_inserted() {
        let (mut db, mut target, request) = fixture();
        target.claim_id = Some(ClaimId::new());
        assert!(matches!(
            admit(&mut db, &target, &request, None),
            Err(AddError::ClaimChanged)
        ));
        assert_eq!(
            operations::table
                .count()
                .get_result::<i64>(&mut db)
                .unwrap(),
            0
        );
    }

    #[test]
    fn publication_failure_does_not_publish_membership_or_release_lease() {
        let (mut db, target, request) = fixture();
        let intent = admit(&mut db, &target, &request, None).unwrap();
        let plan = AddPlan {
            version: 1,
            workspace_id: target.workspace.id,
            workspace_path: target.workspace.canonical_path.clone(),
            claim_id: Some(ClaimId::new()),
            previous_pool_id: None,
            existing: vec![],
            additions: vec![],
            requested: vec![],
            relocation: None,
        };
        assert!(matches!(
            finish(&mut db, &intent.lease_id, &plan, &[], false),
            Err(AddError::ClaimChanged)
        ));
        assert!(
            storage::find_running_operation(&mut db, &target.workspace.id)
                .unwrap()
                .is_some()
        );
        assert!(!has_event(&mut db, &intent.id, "workspace_repositories_added").unwrap());
    }
}
