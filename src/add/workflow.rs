use std::path::Path;

use diesel::SqliteConnection;
use serde_json::json;
use snafu::{ensure, ResultExt};

use super::*;
use crate::domain::{LeaseId, OperationState, RepoWorktreeState};
use crate::git;
use crate::reconciliation::RecoveryOutcome;

pub fn execute(db: &mut SqliteConnection, request: AddRequest) -> Result<AddResult, AddError> {
    ensure!(!request.repositories.is_empty(), EmptySnafu);
    let target = recover_target(db, &request)?;
    let operation = persistence::admit(db, &target, &request, None)?;
    let plan = prepare_operation(db, &operation.lease_id, &target, &request)?;
    match apply(db, &operation.lease_id, &plan) {
        Ok(pool) => Ok(result(&plan, operation.id, pool)),
        Err(error) => fail_execution(db, &operation, &plan, error),
    }
}

fn recover_target(db: &mut SqliteConnection, request: &AddRequest) -> Result<AddTarget, AddError> {
    let target = locate_target(db, &request.selector)?;
    let outcome = crate::reconciliation::recover_expired_operation(db, &target.workspace.id)?;
    ensure!(!matches!(outcome, RecoveryOutcome::LeaseActive), BusySnafu);
    let current = locate_target(db, &request.selector)?;
    ensure!(
        current.workspace.id == target.workspace.id && current.claim_id == target.claim_id,
        ClaimChangedSnafu
    );
    if let Some(original) = persistence::unresolved(db, &target.workspace.id)? {
        recover_residual(db, &current, request, original)?;
    }
    // Keep the claim captured before recovery; never adopt a later allocation.
    Ok(current)
}

fn recover_residual(
    db: &mut SqliteConnection,
    target: &AddTarget,
    request: &AddRequest,
    original: OperationId,
) -> Result<(), AddError> {
    let recovery = persistence::admit(db, target, request, Some(original))?;
    let plan = persistence::load_plan(db, &original)?.ok_or_else(|| JournalSnafu.build())?;
    persistence::event(
        db,
        &recovery.lease_id,
        "workspace_add_planned",
        document(&plan)?,
    )?;
    // Recovery events retain the original step ownership evidence.
    persistence::event(
        db,
        &recovery.lease_id,
        "workspace_add_recovery_source",
        document(&json!({"operation_id": original}))?,
    )?;
    compensate_or_retain(db, &recovery.lease_id, &plan, &original)?;
    Ok(())
}

fn prepare_operation(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    target: &AddTarget,
    request: &AddRequest,
) -> Result<AddPlan, AddError> {
    let prepared = planning::prepare(db, lease, target, request).and_then(|plan| {
        persistence::event(db, lease, "workspace_add_planned", document(&plan)?)?;
        Ok(plan)
    });
    match prepared {
        Ok(plan) => Ok(plan),
        Err(error) => {
            persistence::terminal(db, lease, OperationState::Failed, Some(&error))?;
            Err(error)
        }
    }
}

fn fail_execution(
    db: &mut SqliteConnection,
    operation: &storage::OperationIntent,
    plan: &AddPlan,
    error: AddError,
) -> Result<AddResult, AddError> {
    // A lost acknowledgement must not compensate a committed addition.
    if storage::operation_state(db, &operation.id)? == Some(OperationState::Succeeded) {
        let pool = storage::find_workspace(db, &plan.workspace_id)?.pool_id;
        return Ok(result(plan, operation.id, pool));
    }
    persistence::event(
        db,
        &operation.lease_id,
        "workspace_add_failure",
        document(&json!({"error": error.to_string()}))?,
    )?;
    match compensate_or_retain(db, &operation.lease_id, plan, &operation.id) {
        Ok(()) => Err(error),
        Err(cleanup) => Err(CompensationSnafu {
            cleanup: Box::new(cleanup),
        }
        .into_error(Box::new(error))),
    }
}

fn result(plan: &AddPlan, operation_id: OperationId, pool_id: Option<PoolId>) -> AddResult {
    let repositories = plan
        .requested
        .iter()
        .filter_map(|id| {
            let added = plan
                .additions
                .iter()
                .find(|repo| repo.origin_repository_id == *id);
            let repo = added.or_else(|| {
                plan.existing
                    .iter()
                    .find(|repo| repo.origin_repository_id == *id)
            })?;
            Some(RepositoryResult {
                origin_repository_id: *id,
                worktree_id: repo.worktree_id,
                worktree_path: final_path(plan, repo).clone(),
                result: if added.is_some() {
                    RepositoryOutcome::Added
                } else {
                    RepositoryOutcome::AlreadyPresent
                },
            })
        })
        .collect();
    AddResult {
        schema_version: 1,
        operation_id,
        workspace_id: plan.workspace_id,
        workspace_path: plan.workspace_path.clone(),
        claim_id: plan.claim_id,
        previous_pool_id: plan.previous_pool_id,
        pool_id,
        repositories,
        relocated: plan
            .relocation
            .iter()
            .map(|move_| RelocatedResult {
                worktree_id: move_.worktree_id,
                previous_path: move_.previous_path.clone(),
                worktree_path: move_.worktree_path.clone(),
            })
            .collect(),
    }
}

fn final_path<'a>(plan: &'a AddPlan, repo: &'a RepositoryPlan) -> &'a CanonicalPath {
    plan.relocation
        .as_ref()
        .filter(|move_| move_.worktree_id == repo.worktree_id)
        .map_or(&repo.worktree_path, |move_| &move_.worktree_path)
}

fn move_original(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    repo: &RepositoryPlan,
    from: &CanonicalPath,
    to: &CanonicalPath,
) -> Result<(), AddError> {
    planning::observe(db, lease, repo, from)?;
    planning::absent(to.as_path())?;
    persistence::step(
        db,
        lease,
        "move original worktree",
        document(&json!({"from": from, "to": to, "worktree_id": repo.worktree_id}))?,
    )?;
    git::move_worktree_with_heartbeat(&repo.source_path, from.as_path(), to.as_path(), || {
        persistence::renew(db, lease)
    })?;
    persistence::event(
        db,
        lease,
        "worktree_relocated",
        document(&json!({"from": from, "to": to, "worktree_id": repo.worktree_id}))?,
    )
}

fn apply(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    plan: &AddPlan,
) -> Result<Option<PoolId>, AddError> {
    provision(db, lease, plan)?;
    let observed = final_observations(db, lease, plan)?;
    persistence::finish(db, lease, plan, &observed, false)
}

fn verify_plan(db: &mut SqliteConnection, lease: &LeaseId, plan: &AddPlan) -> Result<(), AddError> {
    let operation = storage::repository::operation_lease_for_mutation(db, lease)?;
    let persisted =
        persistence::load_plan(db, &operation.operation_id)?.ok_or_else(|| JournalSnafu.build())?;
    ensure!(
        operation.workspace_id == plan.workspace_id && document(&persisted)? == document(plan)?,
        JournalSnafu
    );
    Ok(())
}

fn provision(db: &mut SqliteConnection, lease: &LeaseId, plan: &AddPlan) -> Result<(), AddError> {
    verify_plan(db, lease, plan)?;
    if let Some(move_) = &plan.relocation {
        let original = plan
            .existing
            .iter()
            .find(|repo| repo.worktree_id == move_.worktree_id)
            .ok_or_else(|| JournalSnafu.build())?;
        move_original(
            db,
            lease,
            original,
            &move_.previous_path,
            &move_.staging_path,
        )?;
        persistence::step(db, lease, "create workspace container", document(move_)?)?;
        std::fs::create_dir(plan.workspace_path.as_path()).context(IoSnafu {
            path: plan.workspace_path.as_path(),
        })?;
        persistence::event(
            db,
            lease,
            "workspace_container_created",
            document(&directory_identity(plan.workspace_path.as_path())?)?,
        )?;
        move_original(
            db,
            lease,
            original,
            &move_.staging_path,
            &move_.worktree_path,
        )?;
    }
    for repo in &plan.additions {
        planning::absent(repo.worktree_path.as_path())?;
        persistence::step(
            db,
            lease,
            &format!("add:{}", repo.worktree_id),
            document(repo)?,
        )?;
        persistence::event(db, lease, "worktree_add_intended", document(repo)?)?;
        persistence::step(db, lease, "create new worktree directory", document(repo)?)?;
        std::fs::create_dir(repo.worktree_path.as_path()).context(IoSnafu {
            path: repo.worktree_path.as_path(),
        })?;
        persistence::event(
            db,
            lease,
            "worktree_directory_created",
            document(&json!({
                "worktree_id": repo.worktree_id, "identity": directory_identity(repo.worktree_path.as_path())?,
            }))?,
        )?;
        persistence::step(db, lease, "attach new worktree", document(repo)?)?;
        persistence::renew(db, lease)?;
        git::add_detached_worktree_at_with_heartbeat(
            &repo.source_path,
            repo.worktree_path.as_path(),
            &repo.head,
            || persistence::renew(db, lease),
        )?;
        persistence::event(db, lease, "worktree_add_completed", document(repo)?)?;
    }
    Ok(())
}

fn owns_directory(
    db: &mut SqliteConnection,
    operation: &OperationId,
    repo: &RepositoryPlan,
) -> Result<bool, AddError> {
    let actual = directory_identity(repo.worktree_path.as_path())?;
    for event in storage::list_events_for_operation(db, operation)? {
        if event.event_type == "worktree_directory_created" {
            if let Some(details) = event.details_json {
                let value: serde_json::Value =
                    serde_json::from_str(&details.to_string()).context(DecodeSnafu)?;
                if value["worktree_id"] == repo.worktree_id.to_string() {
                    let expected: DirectoryIdentity =
                        serde_json::from_value(value["identity"].clone()).context(DecodeSnafu)?;
                    return Ok(expected == actual);
                }
            }
        }
    }
    Ok(false)
}

fn final_observations(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    plan: &AddPlan,
) -> Result<Vec<(RepoWorktreeId, RepoWorktreeState)>, AddError> {
    let mut states = Vec::new();
    for repo in &plan.existing {
        states.push((
            repo.worktree_id,
            planning::observe(db, lease, repo, final_path(plan, repo))?,
        ));
    }
    let operation = storage::repository::operation_lease_for_mutation(db, lease)?.operation_id;
    let evidence = evidence_operation(db, &operation)?;
    for repo in &plan.additions {
        ensure!(
            owns_directory(db, &evidence, repo)?,
            UnsafeSnafu {
                path: repo.worktree_path.as_path()
            }
        );
        let state = planning::observe(db, lease, repo, &repo.worktree_path)?;
        ensure!(
            state == RepoWorktreeState::Attached,
            UnsafeSnafu {
                path: repo.worktree_path.as_path()
            }
        );
        states.push((repo.worktree_id, state));
    }
    Ok(states)
}

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[serde(default)]
    created: Option<std::time::SystemTime>,
}

fn directory_identity(path: &Path) -> Result<DirectoryIdentity, AddError> {
    let metadata = std::fs::symlink_metadata(path).context(IoSnafu { path })?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        UnsafeSnafu { path }
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(DirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            created: metadata.created().ok(),
        })
    }
    #[cfg(not(unix))]
    {
        Ok(DirectoryIdentity {
            created: Some(metadata.created().context(IoSnafu { path })?),
        })
    }
}

fn evidence_operation(
    db: &mut SqliteConnection,
    operation: &OperationId,
) -> Result<OperationId, AddError> {
    let mut current = *operation;
    let mut seen = std::collections::HashSet::new();
    loop {
        ensure!(seen.insert(current), JournalSnafu);
        let intent = persistence::request_intent(db, &current)?;
        match intent.recovery_of {
            Some(parent) => current = parent,
            None => return Ok(current),
        }
    }
}

fn intended(
    db: &mut SqliteConnection,
    operation: &OperationId,
    repo: &RepositoryPlan,
) -> Result<bool, AddError> {
    for event in storage::list_events_for_operation(db, operation)? {
        if event.event_type == "worktree_add_intended" {
            if let Some(details) = event.details_json {
                let planned: RepositoryPlan =
                    serde_json::from_str(&details.to_string()).context(DecodeSnafu)?;
                if planned.worktree_id == repo.worktree_id {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn remove_container(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    operation: &OperationId,
    plan: &AddPlan,
) -> Result<(), AddError> {
    if !plan.workspace_path.as_path().exists() {
        return Ok(());
    }
    let actual = directory_identity(plan.workspace_path.as_path())?;
    let events = storage::list_events_for_operation(db, operation)?;
    let owned = events
        .iter()
        .filter(|event| event.event_type == "workspace_container_created")
        .filter_map(|event| event.details_json.as_ref())
        .any(|details| {
            serde_json::from_str::<DirectoryIdentity>(&details.to_string())
                .is_ok_and(|expected| expected == actual)
        });
    ensure!(
        owned,
        UnsafeSnafu {
            path: plan.workspace_path.as_path()
        }
    );
    persistence::step(
        db,
        lease,
        "remove empty workspace container",
        document(&plan.workspace_path)?,
    )?;
    std::fs::remove_dir(plan.workspace_path.as_path()).context(IoSnafu {
        path: plan.workspace_path.as_path(),
    })?;
    persistence::event(
        db,
        lease,
        "workspace_container_removed",
        document(&plan.workspace_path)?,
    )
}

fn compensate(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    plan: &AddPlan,
    operation: &OperationId,
) -> Result<(), AddError> {
    verify_plan(db, lease, plan)?;
    let evidence = evidence_operation(db, operation)?;
    let events = storage::list_events_for_operation(db, &evidence)?;
    let mutated = events.iter().any(|event| {
        matches!(
            event.event_type.as_str(),
            "worktree_relocated" | "workspace_container_created" | "worktree_directory_created"
        )
    }) || plan
        .relocation
        .as_ref()
        .is_some_and(|relocation| relocation.staging_path.as_path().exists());
    persistence::event(db, lease, "workspace_add_rollback", document(plan)?)?;
    for repo in plan.additions.iter().rev() {
        remove_addition(db, lease, &evidence, repo)?;
    }
    restore_original(db, lease, &evidence, plan)?;
    record_restoration(db, lease, plan, mutated)
}

fn remove_addition(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    evidence: &OperationId,
    repo: &RepositoryPlan,
) -> Result<(), AddError> {
    if !intended(db, evidence, repo)? {
        return Ok(());
    }
    if !repo.worktree_path.as_path().exists() {
        let found = git::list_worktrees_with_heartbeat(&repo.source_path, || {
            persistence::renew(db, lease)
        })?;
        ensure!(
            !found.iter().any(|tree| tree.path == repo.worktree_path),
            UnsafeSnafu {
                path: repo.worktree_path.as_path()
            }
        );
        return Ok(());
    }
    ensure!(
        owns_directory(db, evidence, repo)?,
        UnsafeSnafu {
            path: repo.worktree_path.as_path()
        }
    );
    let trees =
        git::list_worktrees_with_heartbeat(&repo.source_path, || persistence::renew(db, lease))?;
    if !trees.iter().any(|tree| tree.path == repo.worktree_path) {
        persistence::step(db, lease, "remove empty new directory", document(repo)?)?;
        std::fs::remove_dir(repo.worktree_path.as_path()).context(IoSnafu {
            path: repo.worktree_path.as_path(),
        })?;
        persistence::event(db, lease, "worktree_directory_removed", document(repo)?)?;
        return Ok(());
    }
    ensure!(
        planning::observe(db, lease, repo, &repo.worktree_path)? == RepoWorktreeState::Attached,
        UnsafeSnafu {
            path: repo.worktree_path.as_path()
        }
    );
    ensure!(
        !git::has_ignored_files_with_heartbeat(repo.worktree_path.as_path(), || {
            persistence::renew(db, lease)
        })?,
        UnsafeSnafu {
            path: repo.worktree_path.as_path()
        }
    );
    persistence::step(db, lease, "remove new worktree", document(repo)?)?;
    git::remove_clean_worktree_with_heartbeat(
        &repo.source_path,
        repo.worktree_path.as_path(),
        || persistence::renew(db, lease),
    )?;
    persistence::event(db, lease, "worktree_add_rolled_back", document(repo)?)?;
    Ok(())
}

fn restore_original(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    evidence: &OperationId,
    plan: &AddPlan,
) -> Result<(), AddError> {
    if let Some(move_) = &plan.relocation {
        let repo = plan
            .existing
            .iter()
            .find(|repo| repo.worktree_id == move_.worktree_id)
            .ok_or_else(|| JournalSnafu.build())?;
        let trees = git::list_worktrees_with_heartbeat(&repo.source_path, || {
            persistence::renew(db, lease)
        })?;
        if trees
            .iter()
            .any(|tree| tree.path == move_.previous_path && tree.prunable.is_none())
        {
            planning::observe(db, lease, repo, &move_.previous_path)?;
            planning::absent(move_.staging_path.as_path())?;
        } else {
            if trees
                .iter()
                .any(|tree| tree.path == move_.worktree_path && tree.prunable.is_none())
            {
                move_original(db, lease, repo, &move_.worktree_path, &move_.staging_path)?;
            }
            planning::observe(db, lease, repo, &move_.staging_path)?;
            remove_container(db, lease, evidence, plan)?;
            move_original(db, lease, repo, &move_.staging_path, &move_.previous_path)?;
        }
    }
    Ok(())
}

fn record_restoration(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    plan: &AddPlan,
    mutated: bool,
) -> Result<(), AddError> {
    let owned = storage::repository::operation_lease_for_mutation(db, lease)?;
    let mut health = WorkspaceState::Ready;
    for repo in &plan.existing {
        let state = planning::observe(db, lease, repo, &repo.worktree_path)?;
        if state != RepoWorktreeState::Attached {
            health = WorkspaceState::Degraded;
        }
        storage::record_repo_worktree_transition(
            db,
            &repo.worktree_id,
            &owned.operation_id,
            state,
            Some(repo.head.clone()),
            storage::TransitionMetadata::new("worktree_restored", "trees"),
        )?;
    }
    storage::record_workspace_transition(
        db,
        &plan.workspace_id,
        &owned.operation_id,
        health,
        storage::TransitionMetadata::new("workspace_add_restored", "trees"),
    )?;
    let state = if mutated {
        OperationState::RolledBack
    } else {
        OperationState::Failed
    };
    if persistence::request_intent(db, &owned.operation_id)?
        .recovery_of
        .is_some()
    {
        persistence::event(
            db,
            lease,
            "operation_recovered",
            document(&json!({"outcome": state}))?,
        )?;
    }
    persistence::compensated(db, lease, plan, state)
}

fn compensate_or_retain(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    plan: &AddPlan,
    operation: &OperationId,
) -> Result<(), AddError> {
    match compensate(db, lease, plan, operation) {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("Addition recovery is incomplete for workspace {} at {}. Inspect the reported paths, preserve local work, then retry trees add --workspace-id {} --repo PATH.",
                plan.workspace_id, plan.workspace_path, plan.workspace_id);
            if let Some(relocation) = &plan.relocation {
                eprintln!(
                    "Original worktree paths: original={} staging={} intended={}",
                    relocation.previous_path, relocation.staging_path, relocation.worktree_path
                );
            }
            // Residual snapshots describe only paths that this addition attempted to create.
            let evidence = evidence_operation(db, operation)?;
            let mut remaining = Vec::new();
            for repo in &plan.additions {
                if intended(db, &evidence, repo)?
                    && std::fs::symlink_metadata(repo.worktree_path.as_path()).is_ok()
                {
                    remaining.push(repo.clone());
                }
            }
            persistence::retain_residuals(db, lease, plan, &remaining, &error)?;
            Err(error)
        }
    }
}

pub(crate) fn recover(
    db: &mut SqliteConnection,
    operation: &storage::OperationRow,
    lease: &LeaseId,
) -> Result<RecoveryOutcome, AddError> {
    let Some(plan) = persistence::load_plan(db, &operation.id)? else {
        persistence::event(
            db,
            lease,
            "operation_recovered",
            document(&json!({"outcome": "failed", "reason": "no mutation plan"}))?,
        )?;
        persistence::terminal(db, lease, OperationState::Failed, None)?;
        return Ok(RecoveryOutcome::Failed);
    };
    ensure!(plan.workspace_id == operation.workspace_id, JournalSnafu);
    if can_publish_recovery(db, operation, lease, &plan)? {
        return Ok(RecoveryOutcome::Succeeded);
    }
    persistence::event(
        db,
        lease,
        "operation_recovered",
        document(&json!({"decision": "rollback"}))?,
    )?;
    match compensate_or_retain(db, lease, &plan, &operation.id) {
        Ok(()) => Ok(
            if storage::operation_state(db, &operation.id)? == Some(OperationState::RolledBack) {
                RecoveryOutcome::RolledBack
            } else {
                RecoveryOutcome::Failed
            },
        ),
        Err(error) => {
            if storage::operation_state(db, &operation.id)? == Some(OperationState::Failed) {
                Ok(RecoveryOutcome::Failed)
            } else {
                Err(error)
            }
        }
    }
}

fn can_publish_recovery(
    db: &mut SqliteConnection,
    operation: &storage::OperationRow,
    lease: &LeaseId,
    plan: &AddPlan,
) -> Result<bool, AddError> {
    let rollback = persistence::has_event(db, &operation.id, "workspace_add_rollback")?
        || persistence::request_intent(db, &operation.id)?
            .recovery_of
            .is_some();
    let evidence = evidence_operation(db, &operation.id)?;
    let mut owns_additions = true;
    for repo in &plan.additions {
        owns_additions &= intended(db, &evidence, repo)?;
    }
    if rollback || !owns_additions {
        return Ok(false);
    }
    let Ok(observed) = final_observations(db, lease, plan) else {
        return Ok(false);
    };
    persistence::finish(db, lease, plan, &observed, true)?;
    Ok(true)
}

use snafu::IntoError;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
