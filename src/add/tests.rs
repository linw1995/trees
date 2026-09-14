use std::fs;
use std::path::PathBuf;

use crate as trees;
use crate as trees_library;
use crate::add::{self, persistence, planning, AddRequest};
use crate::domain::CanonicalPath;
use crate::storage;

#[path = "../../tests/support/add.rs"]
mod support;
use support::{clear_event_failure, git, install_event_failure, Fixture};

impl Fixture {
    fn plan(&mut self, request: &AddRequest) -> (storage::OperationIntent, add::AddPlan) {
        let target = add::locate_target(&mut self.db, &request.selector).unwrap();
        let intent = persistence::admit(&mut self.db, &target, request, None).unwrap();
        let plan = planning::prepare(&mut self.db, &intent.lease_id, &target, request).unwrap();
        (intent, plan)
    }
}

#[test]
fn planning_deduplicates_existing_inputs_without_fetching_or_moving() {
    let mut fixture = Fixture::new();
    git(
        &fixture.api,
        &["remote", "add", "origin", "/missing/remote"],
    );
    let mut request = fixture.request(vec![fixture.api.clone(), fixture.path.as_path().to_owned()]);
    request.offline = false;
    let (_, plan) = fixture.plan(&request);
    assert!(plan.additions.is_empty());
    assert!(plan.relocation.is_none());
    assert_eq!(plan.requested.len(), 1);
    assert!(fixture.path.as_path().join(".git").is_file());
}

#[test]
fn planning_and_git_moves_preserve_original_work() {
    let mut fixture = Fixture::new();
    fs::write(fixture.path.as_path().join("README"), "staged\n").unwrap();
    git(fixture.path.as_path(), &["add", "README"]);
    fs::write(fixture.path.as_path().join("README"), "unstaged\n").unwrap();
    fs::write(fixture.path.as_path().join("local"), "local\n").unwrap();
    let before = git(fixture.path.as_path(), &["status", "--porcelain"]);
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    let relocation = plan.relocation.unwrap();
    let source = CanonicalPath::resolve(&fixture.api).unwrap();
    trees::git::move_worktree_with_heartbeat(
        &source,
        fixture.path.as_path(),
        relocation.staging_path.as_path(),
        || persistence::renew(&mut fixture.db, &intent.lease_id),
    )
    .unwrap();
    fs::create_dir(fixture.path.as_path()).unwrap();
    trees::git::move_worktree_with_heartbeat(
        &source,
        relocation.staging_path.as_path(),
        relocation.worktree_path.as_path(),
        || persistence::renew(&mut fixture.db, &intent.lease_id),
    )
    .unwrap();
    assert_eq!(
        git(
            relocation.worktree_path.as_path(),
            &["status", "--porcelain"]
        ),
        before
    );
    assert_eq!(
        fs::read_to_string(relocation.worktree_path.as_path().join("README")).unwrap(),
        "unstaged\n"
    );
}

fn expire(db: &mut diesel::SqliteConnection, lease: trees::domain::LeaseId) {
    use diesel::prelude::*;
    use trees::schema::operation_leases;
    diesel::update(operation_leases::table.find(lease))
        .set(
            operation_leases::lease_expires_at.eq("2000-01-01T00:00:00Z"
                .parse::<trees::domain::Timestamp>()
                .unwrap()),
        )
        .execute(db)
        .unwrap();
}

#[test]
fn recovery_restores_original_worktree_when_workspace_root_is_absent() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_planned",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    let relocation = plan.relocation.as_ref().unwrap();
    trees::git::move_worktree_with_heartbeat(
        &plan.existing[0].source_path,
        fixture.path.as_path(),
        relocation.staging_path.as_path(),
        || Ok(()),
    )
    .unwrap();
    expire(&mut fixture.db, intent.lease_id);
    let outcome =
        trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace)
            .unwrap();
    assert!(matches!(
        outcome,
        trees::reconciliation::RecoveryOutcome::RolledBack
    ));
    assert!(fixture.path.as_path().join(".git").is_file());
    assert!(!relocation.staging_path.as_path().exists());
}

#[test]
fn recovery_publishes_complete_layout_after_interruption() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_planned",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan).unwrap();
    assert_eq!(
        storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .len(),
        1
    );
    expire(&mut fixture.db, intent.lease_id);
    assert!(matches!(
        trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace)
            .unwrap(),
        trees::reconciliation::RecoveryOutcome::Succeeded
    ));
    assert_eq!(
        storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn recovery_preserves_new_user_content_and_retry_uses_a_new_operation() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_planned",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan).unwrap();
    let local = fixture.path.as_path().join("web/local");
    fs::write(&local, "keep me").unwrap();
    expire(&mut fixture.db, intent.lease_id);
    assert!(matches!(
        trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace)
            .unwrap(),
        trees::reconciliation::RecoveryOutcome::Failed
    ));
    assert_eq!(fs::read_to_string(&local).unwrap(), "keep me");
    assert!(persistence::unresolved(&mut fixture.db, &fixture.workspace)
        .unwrap()
        .is_some());
    fs::remove_file(&local).unwrap();
    let request = fixture.request(vec![fixture.web.clone()]);
    let result = add::execute(&mut fixture.db, request).unwrap();
    assert_ne!(result.operation_id, intent.id);
    assert!(persistence::unresolved(&mut fixture.db, &fixture.workspace)
        .unwrap()
        .is_none());
    assert_eq!(
        storage::operation_state(&mut fixture.db, &intent.id).unwrap(),
        Some(trees::domain::OperationState::Failed)
    );
}

#[test]
fn compensation_intent_prevents_recovery_from_publishing_success() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_planned",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan).unwrap();
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_rollback",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    expire(&mut fixture.db, intent.lease_id);
    assert!(matches!(
        trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace)
            .unwrap(),
        trees::reconciliation::RecoveryOutcome::RolledBack
    ));
    assert!(fixture.path.as_path().join(".git").is_file());
    assert!(!fixture.path.as_path().join("web").exists());
}

#[test]
fn busy_and_replaced_leases_cannot_mutate_worktrees() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    let other = fixture.request(vec![fixture.web.clone()]);
    assert!(matches!(
        add::execute(&mut fixture.db, other),
        Err(add::AddError::Busy)
    ));
    expire(&mut fixture.db, intent.lease_id);
    let replacement = trees::domain::LeaseId::new();
    let lease = storage::find_operation_lease(&mut fixture.db, &intent.id)
        .unwrap()
        .unwrap();
    assert!(storage::claim_expired_operation(
        &mut fixture.db,
        &intent.lease_id,
        &lease.lease_expires_at,
        &replacement,
        &trees::domain::Timestamp::after_seconds(300)
    )
    .unwrap());
    assert!(add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan).is_err());
    assert!(fixture.path.as_path().join(".git").is_file());
    assert!(!plan.relocation.unwrap().staging_path.as_path().exists());
}

#[test]
fn failed_compensation_keeps_ignored_content_and_original_pool() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_planned",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan).unwrap();
    let added = fixture.path.as_path().join("web");
    let info = git(&added, &["rev-parse", "--git-path", "info/exclude"]);
    let exclude = PathBuf::from(info);
    let previous = fs::read_to_string(&exclude).unwrap_or_default();
    fs::write(&exclude, format!("{previous}\ncache\n")).unwrap();
    fs::write(added.join("cache"), "keep ignored content").unwrap();
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_rollback",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    expire(&mut fixture.db, intent.lease_id);
    assert!(matches!(
        trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace)
            .unwrap(),
        trees::reconciliation::RecoveryOutcome::Failed
    ));
    assert_eq!(
        fs::read_to_string(added.join("cache")).unwrap(),
        "keep ignored content"
    );
    assert_eq!(
        storage::find_workspace(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .pool_id,
        plan.previous_pool_id
    );
}

#[test]
fn database_failure_during_compensation_can_be_recovered_again() {
    for event in [
        "workspace_add_rollback",
        "worktree_add_rolled_back",
        "workspace_container_removed",
        "worktree_restored",
        "workspace_add_resolved",
        "operation_rolled_back",
    ] {
        let mut fixture = Fixture::new();
        let request = fixture.request(vec![fixture.web.clone()]);
        let (intent, plan) = fixture.plan(&request);
        persistence::event(
            &mut fixture.db,
            &intent.lease_id,
            "workspace_add_planned",
            add::document(&plan).unwrap(),
        )
        .unwrap();
        add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan).unwrap();
        // Select compensation before injecting a failure in its progress records.
        persistence::event(
            &mut fixture.db,
            &intent.lease_id,
            "workspace_add_rollback",
            add::document(&plan).unwrap(),
        )
        .unwrap();
        install_event_failure(&mut fixture.db, "fail_compensation", event);
        expire(&mut fixture.db, intent.lease_id);
        assert!(matches!(
            trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace)
                .unwrap(),
            trees::reconciliation::RecoveryOutcome::Failed
        ));
        clear_event_failure(&mut fixture.db, "fail_compensation");
        let request = fixture.request(vec![fixture.web.clone()]);
        add::execute(&mut fixture.db, request).unwrap_or_else(|error| panic!("{event}: {error:?}"));
        assert_eq!(
            fs::read_to_string(fixture.path.as_path().join("api/README")).unwrap(),
            "initial\n"
        );
    }
}

#[test]
fn unresolved_addition_rejects_release_without_aligning_original_work() {
    let mut fixture = Fixture::automatic();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_planned",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan).unwrap();
    let user_file = fixture.path.as_path().join("web/local");
    fs::write(&user_file, "user work").unwrap();
    expire(&mut fixture.db, intent.lease_id);
    trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace).unwrap();
    let claim = plan.claim_id.unwrap();
    assert!(
        trees::workspace::release_automatic_workspace(&mut fixture.db, &fixture.path, claim)
            .is_err()
    );
    assert_eq!(
        storage::find_workspace_claim(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .unwrap()
            .id,
        claim
    );
    assert_eq!(fs::read_to_string(user_file).unwrap(), "user work");
}

#[test]
fn recovery_does_not_adopt_a_foreign_worktree_at_an_intended_path() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    add::execute(&mut fixture.db, request).unwrap();
    let shared = Fixture::repository(&fixture.root, "shared");
    let request = fixture.request(vec![shared.clone()]);
    let (intent, plan) = fixture.plan(&request);
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_planned",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "worktree_add_intended",
        add::document(&plan.additions[0]).unwrap(),
    )
    .unwrap();
    let foreign = plan.additions[0].worktree_path.as_path();
    git(
        &shared,
        &[
            "worktree",
            "add",
            "--detach",
            foreign.to_str().unwrap(),
            "HEAD",
        ],
    );
    expire(&mut fixture.db, intent.lease_id);
    assert!(matches!(
        trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace)
            .unwrap(),
        trees::reconciliation::RecoveryOutcome::Failed
    ));
    assert!(foreign.join(".git").is_file());
    assert_eq!(
        fs::read_to_string(foreign.join("README")).unwrap(),
        "initial\n"
    );
}

#[test]
fn recovery_does_not_delete_a_replacement_of_an_owned_directory() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    persistence::event(
        &mut fixture.db,
        &intent.lease_id,
        "workspace_add_planned",
        add::document(&plan).unwrap(),
    )
    .unwrap();
    add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan).unwrap();
    let path = plan.additions[0].worktree_path.as_path();
    git(
        &fixture.web,
        &["worktree", "remove", path.to_str().unwrap()],
    );
    git(
        &fixture.web,
        &[
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    expire(&mut fixture.db, intent.lease_id);
    assert!(matches!(
        trees::reconciliation::recover_expired_operation(&mut fixture.db, &fixture.workspace)
            .unwrap(),
        trees::reconciliation::RecoveryOutcome::Failed
    ));
    assert!(path.join(".git").is_file());
}

#[test]
fn provisioning_refuses_an_unrecorded_plan() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    let (intent, plan) = fixture.plan(&request);
    assert!(matches!(
        add::workflow::provision(&mut fixture.db, &intent.lease_id, &plan),
        Err(add::AddError::Journal)
    ));
    assert!(fixture.path.as_path().join(".git").is_file());
}
