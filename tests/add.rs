use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use trees::add::{self, persistence, planning, AddRequest};
use trees::domain::{CanonicalPath, WorkspaceId};
use trees::storage;
use trees::workspace_locator::WorkspaceSelector;

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    root: PathBuf,
    db: diesel::SqliteConnection,
    workspace: WorkspaceId,
    path: CanonicalPath,
    api: PathBuf,
    web: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("trees-add-{}", WorkspaceId::new()));
        fs::create_dir(&root).unwrap();
        let api = Self::repository(&root, "api");
        let web = Self::repository(&root, "web");
        let mut db = trees::database::connect(&root.join("db.sqlite")).unwrap();
        let result = trees::workspace::create_with_connection(
            &mut db,
            trees::workspace::prepare_create(&trees::workspace::CreateRequest {
                workspace_path: root.join("workspace"),
                repositories: vec![api.clone()],
                offline: true,
            })
            .unwrap(),
        )
        .unwrap();
        let workspace = storage::find_workspace_by_path(&mut db, &result.workspace_path)
            .unwrap()
            .unwrap()
            .id;
        Self {
            root,
            db,
            workspace,
            path: result.workspace_path,
            api,
            web,
        }
    }

    fn repository(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q"]);
        git(&path, &["config", "user.email", "trees@example.invalid"]);
        git(&path, &["config", "user.name", "trees tests"]);
        fs::write(path.join("README"), "initial\n").unwrap();
        git(&path, &["add", "README"]);
        git(&path, &["commit", "-qm", "initial"]);
        path
    }

    fn request(&self, repositories: Vec<PathBuf>) -> AddRequest {
        AddRequest {
            selector: WorkspaceSelector::Id(self.workspace),
            repositories,
            offline: true,
        }
    }

    fn plan(&mut self, request: &AddRequest) -> (storage::OperationIntent, add::AddPlan) {
        let target = add::locate_target(&mut self.db, &request.selector).unwrap();
        let intent = persistence::admit(&mut self.db, &target, request, None).unwrap();
        let plan = planning::prepare(&mut self.db, &intent.lease_id, &target, request).unwrap();
        (intent, plan)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
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

#[test]
fn execute_promotes_dirty_workspace_and_retries_without_duplicate_membership() {
    let mut fixture = Fixture::new();
    fs::write(fixture.path.as_path().join("README"), "changes\n").unwrap();
    let original = storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace).unwrap()[0].id;
    let request = fixture.request(vec![fixture.web.clone()]);
    let result = add::execute(&mut fixture.db, request).unwrap();
    assert_eq!(result.relocated.len(), 1);
    assert_eq!(result.relocated[0].worktree_id, original);
    assert_eq!(
        fs::read_to_string(fixture.path.as_path().join("api/README")).unwrap(),
        "changes\n"
    );
    let request = fixture.request(vec![fixture.web.clone()]);
    let again = add::execute(&mut fixture.db, request).unwrap();
    assert!(matches!(
        again.repositories[0].result,
        add::RepositoryOutcome::AlreadyPresent
    ));
    assert_eq!(
        storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .len(),
        2
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
