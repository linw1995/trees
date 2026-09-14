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

fn cli(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
    command
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("APPDATA", root.join("app-data"));
    command
}

fn json_output(command: &mut Command) -> serde_json::Value {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn cli_migrates_pool_and_release_reuses_the_expanded_workspace() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("home")).unwrap();
    let created =
        json_output(cli(&fixture.root).args(["create", "--repo", "api", "--offline", "--json"]));
    let workspace_path = created["workspace_path"].as_str().unwrap();
    let claim = created["claim_id"].as_str().unwrap();
    let added = json_output(cli(&fixture.root).args([
        "add",
        "--claim-id",
        claim,
        "--repo",
        "web",
        "--offline",
        "--json",
    ]));
    assert_eq!(added["claim_id"], created["claim_id"]);
    assert_eq!(added["previous_pool_id"], created["pool_id"]);
    assert_ne!(added["pool_id"], created["pool_id"]);
    assert_eq!(added["workspace_path"], created["workspace_path"]);
    assert_eq!(added["schema_version"], 1);
    let status = json_output(cli(&fixture.root).args([
        "status",
        "--workspace-dir",
        workspace_path,
        "--view",
        "workspaces",
        "--json",
    ]));
    assert!(status
        .to_string()
        .contains(added["pool_id"].as_str().unwrap()));
    let released = cli(&fixture.root)
        .args(["release", "--claim-id", claim])
        .output()
        .unwrap();
    assert!(
        released.status.success(),
        "{}",
        String::from_utf8_lossy(&released.stderr)
    );
    let idle = cli(&fixture.root)
        .args([
            "add",
            "--workspace-dir",
            workspace_path,
            "--repo",
            "web",
            "--offline",
        ])
        .output()
        .unwrap();
    assert!(!idle.status.success());
    let reused = json_output(cli(&fixture.root).args([
        "create",
        "--repo",
        "api",
        "--repo",
        "web",
        "--offline",
        "--json",
    ]));
    assert_eq!(reused["workspace_path"], created["workspace_path"]);
    assert_eq!(reused["pool_id"], added["pool_id"]);
    assert_ne!(reused["claim_id"], created["claim_id"]);
    let released = cli(&fixture.root)
        .args([
            "release",
            "--claim-id",
            reused["claim_id"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        released.status.success(),
        "{}",
        String::from_utf8_lossy(&released.stderr)
    );
    let removed = cli(&fixture.root)
        .args(["remove", added["workspace_id"].as_str().unwrap(), "--yes"])
        .output()
        .unwrap();
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!Path::new(workspace_path).exists());
}

#[test]
fn cli_selectors_and_text_output_preserve_idempotency() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("home")).unwrap();
    let created = json_output(cli(&fixture.root).args([
        "create",
        "manual",
        "--repo",
        "api",
        "--offline",
        "--json",
    ]));
    let path = created["workspace_path"].as_str().unwrap();
    let added =
        json_output(cli(&fixture.root).args(["add", path, "--repo", "web", "--offline", "--json"]));
    let id = added["workspace_id"].as_str().unwrap();
    for selection in [
        vec![path],
        vec!["--workspace-id", id],
        vec!["--workspace-dir", path],
    ] {
        let mut command = cli(&fixture.root);
        command
            .arg("add")
            .args(selection)
            .args(["--repo", "web", "--offline"]);
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("repo_result=already_present"));
    }
    let nested = Path::new(path).join("web");
    let output = cli(&fixture.root)
        .current_dir(nested)
        .args(["add", "--repo", "web", "--offline", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["workspace_id"],
        id
    );
}

#[test]
fn consumers_read_committed_child_paths() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    add::execute(&mut fixture.db, request).unwrap();
    let opened =
        trees::workspace_open::resolve_target(&mut fixture.db, &fixture.workspace).unwrap();
    assert_eq!(opened, fixture.path);
    let prepared =
        trees::codex::workspace::prepare(&mut fixture.db, fixture.path.as_path()).unwrap();
    assert_eq!(prepared.roots.len(), 2);
    assert!(prepared.roots.contains(&fixture.path.as_path().join("api")));
    assert!(prepared.roots.contains(&fixture.path.as_path().join("web")));
}
