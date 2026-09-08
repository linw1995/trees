use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use diesel::sqlite::SqliteConnection;

use trees::codex::workspace::{prepare, WorkspacePreparationError};
use trees::domain::{CanonicalPath, RepoWorktreeState, WorkspaceState};
use trees::git;
use trees::storage::list_repo_worktrees;
use trees::workspace::{create_with_connection, prepare_create, CreateRequest};

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!("trees-codex-workspace-{}", uuid::Uuid::now_v7()))
}

fn run_git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).expect("repository should be created");
    run_git(&path, &["init", "-q"]);
    run_git(&path, &["config", "user.email", "trees@example.invalid"]);
    run_git(&path, &["config", "user.name", "trees tests"]);
    fs::write(path.join("README"), format!("{name}\n")).expect("file should be written");
    run_git(&path, &["add", "README"]);
    run_git(&path, &["commit", "-qm", "initial"]);
    path
}

fn managed_workspace(root: &Path) -> (PathBuf, PathBuf, SqliteConnection) {
    let first = repository(root, "alpha");
    let second = repository(root, "beta");
    let workspace_path = root.join("workspace");
    let plan = prepare_create(&CreateRequest {
        workspace_path: workspace_path.clone(),
        repositories: vec![first, second],
    })
    .expect("creation plan should be prepared");
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");
    create_with_connection(&mut connection, plan).expect("workspace should be created");
    (workspace_path, database_path, connection)
}

#[test]
fn prepares_managed_worktree_paths_as_codex_roots() {
    let root = test_root();
    let (workspace_path, database_path, mut connection) = managed_workspace(&root);
    let prepared = prepare(&mut connection, &workspace_path).expect("workspace should prepare");

    assert_eq!(prepared.roots.len(), 2);
    assert!(prepared
        .roots
        .iter()
        .all(|path| path.parent() == Some(prepared.path.as_path())));
    assert!(prepared.roots.iter().all(|path| {
        !path.starts_with(root.join("alpha")) && !path.starts_with(root.join("beta"))
    }));

    drop(connection);
    fs::remove_file(database_path).expect("database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn rejects_a_worktree_removed_outside_trees() {
    let root = test_root();
    let (workspace_path, database_path, mut connection) = managed_workspace(&root);
    let workspace = trees::storage::find_workspace_by_path(
        &mut connection,
        &CanonicalPath::resolve(&workspace_path).expect("workspace should resolve"),
    )
    .expect("workspace lookup should succeed")
    .expect("workspace should exist");
    let worktree = list_repo_worktrees(&mut connection, &workspace.id)
        .expect("worktrees should list")
        .into_iter()
        .next()
        .expect("one worktree should exist");
    assert_eq!(worktree.state, RepoWorktreeState::Attached);
    git::remove_worktree(&worktree.source_path, worktree.worktree_path.as_path())
        .expect("external worktree removal should succeed");

    let error = prepare(&mut connection, &workspace_path)
        .expect_err("removed worktree should prevent Codex launch");
    assert!(matches!(
        error,
        WorkspacePreparationError::NotReady {
            state: WorkspaceState::Degraded,
            ..
        }
    ));

    drop(connection);
    fs::remove_file(database_path).expect("database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn rejects_an_unmanaged_workspace() {
    let root = test_root();
    fs::create_dir_all(&root).expect("test root should be created");
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");

    let error = prepare(&mut connection, &root).expect_err("workspace should be unmanaged");
    assert!(matches!(
        error,
        WorkspacePreparationError::NotManaged { .. }
    ));

    drop(connection);
    fs::remove_file(database_path).expect("database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}
