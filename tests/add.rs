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
