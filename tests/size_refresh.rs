use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use trees::database;
use trees::domain::{
    CanonicalPath, RepoWorktreeId, RepoWorktreeState, Timestamp, WorkspaceId,
    WorkspaceManagementMode, WorkspaceState,
};
use trees::status::disk_usage::Completeness;
use trees::storage::disk_usage_cache;
use trees::storage::{
    ensure_origin_repository, insert_managed_workspace, insert_repo_worktree, NewManagedWorkspace,
    NewRepoWorktree,
};

fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("trees-size-cli-{}", WorkspaceId::new()));
    fs::create_dir_all(&path).unwrap();
    fs::canonicalize(path).unwrap()
}

fn command(root: &Path) -> Command {
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
    command
        .env("HOME", home)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("APPDATA", root.join("app-data"));
    command
}

#[cfg(target_os = "linux")]
fn database_path(root: &Path) -> PathBuf {
    root.join("state/trees/db.sqlite")
}

#[cfg(target_os = "macos")]
fn database_path(root: &Path) -> PathBuf {
    root.join("home/Library/Application Support/trees/db.sqlite")
}

#[cfg(target_os = "windows")]
fn database_path(root: &Path) -> PathBuf {
    root.join("local-app-data/trees/db.sqlite")
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn database_path(root: &Path) -> PathBuf {
    root.join("home/.local/state/trees/db.sqlite")
}

fn path(value: impl AsRef<Path>) -> CanonicalPath {
    CanonicalPath::from_absolute(value).unwrap()
}

fn add_workspace(
    connection: &mut diesel::sqlite::SqliteConnection,
    directory: &Path,
    origin_id: trees::domain::OriginRepositoryId,
) -> (WorkspaceId, RepoWorktreeId) {
    let workspace_id = WorkspaceId::new();
    let worktree_id = RepoWorktreeId::new();
    let now = Timestamp::now();
    insert_managed_workspace(
        connection,
        &NewManagedWorkspace {
            id: workspace_id,
            canonical_path: path(directory),
            state: WorkspaceState::Ready,
            created_at: now.clone(),
            updated_at: now.clone(),
            last_reconciled_at: Some(now.clone()),
            management_mode: WorkspaceManagementMode::Manual,
            pool_id: None,
            last_released_at: None,
            removed_at: None,
        },
    )
    .unwrap();
    insert_repo_worktree(
        connection,
        &NewRepoWorktree {
            id: worktree_id,
            workspace_id,
            origin_repository_id: origin_id,
            worktree_path: path(directory.join("repo")),
            state: RepoWorktreeState::Attached,
            last_head: None,
            last_observed_at: now,
        },
    )
    .unwrap();
    (workspace_id, worktree_id)
}

#[test]
fn lifecycle_refresh_updates_workspace_worktree_and_origin_caches() {
    let root = root();
    let origin_dir = root.join("origin");
    let first_dir = root.join("workspaces/first");
    let second_dir = root.join("workspaces/second");
    for directory in [
        &origin_dir,
        &first_dir.join("repo"),
        &second_dir.join("repo"),
    ] {
        fs::create_dir_all(directory).unwrap();
        fs::write(directory.join("data"), vec![1_u8; 4096]).unwrap();
    }
    let database_path = database_path(&root);
    fs::create_dir_all(database_path.parent().unwrap()).unwrap();
    let mut connection = database::connect(&database_path).unwrap();
    let origin = ensure_origin_repository(
        &mut connection,
        &path(origin_dir.join(".git")),
        &path(&origin_dir),
    )
    .unwrap();
    let (first, first_worktree) = add_workspace(&mut connection, &first_dir, origin.id);
    let (second, _) = add_workspace(&mut connection, &second_dir, origin.id);
    assert_eq!(
        trees::size_refresh::refresh_workspace(&mut connection, first)
            .unwrap()
            .complete,
        3
    );
    assert_eq!(
        disk_usage_cache::workspaces(&mut connection).unwrap()[&first]
            .as_ref()
            .unwrap()
            .status,
        Completeness::Complete
    );
    assert_eq!(
        disk_usage_cache::worktrees(&mut connection).unwrap()[&first_worktree]
            .as_ref()
            .unwrap()
            .status,
        Completeness::Complete
    );
    assert!(
        disk_usage_cache::origins(&mut connection).unwrap()[&origin.id]
            .as_ref()
            .unwrap()
            .allocated_bytes
            .unwrap()
            > 0
    );
    assert!(disk_usage_cache::workspaces(&mut connection).unwrap()[&second].is_none());
    assert_eq!(
        trees::size_refresh::refresh_workspace(&mut connection, second)
            .unwrap()
            .complete,
        3
    );
    drop(connection);

    let repo_status = command(&root)
        .current_dir(&root)
        .args(["status", "--view", "repos", "--json"])
        .output()
        .unwrap();
    assert!(repo_status.status.success());
    let repo_status: serde_json::Value = serde_json::from_slice(&repo_status.stdout).unwrap();
    assert_eq!(repo_status["repos"][0]["disk_usage"]["status"], "complete");
    let repo_human = command(&root)
        .current_dir(&root)
        .args(["status", "--view", "repos"])
        .output()
        .unwrap();
    assert!(repo_human.status.success());
    let repo_human = String::from_utf8(repo_human.stdout).unwrap();
    assert!(repo_human.lines().next().unwrap().contains("SIZE"));

    let workspace_status = command(&root)
        .current_dir(&root)
        .args([
            "status",
            &first.to_string(),
            "--view",
            "workspaces",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(workspace_status.status.success());
    let workspace_status: serde_json::Value =
        serde_json::from_slice(&workspace_status.stdout).unwrap();
    assert_eq!(
        workspace_status["target_workspace"]["repo_worktrees"][0]["disk_usage"]["status"],
        "complete"
    );
    let measured_at = workspace_status["target_disk_usage"]["observed_at"].clone();
    let opened = command(&root)
        .current_dir(&root)
        .args([
            "open",
            "--workspace-id",
            &first.to_string(),
            "--program=/usr/bin/true",
        ])
        .output()
        .unwrap();
    assert!(
        opened.status.success(),
        "{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    let after_open = command(&root)
        .current_dir(&root)
        .args(["status", &first.to_string(), "--json"])
        .output()
        .unwrap();
    assert!(after_open.status.success());
    let after_open: serde_json::Value = serde_json::from_slice(&after_open.stdout).unwrap();
    assert_eq!(after_open["target_disk_usage"]["observed_at"], measured_at);

    fs::remove_dir_all(&origin_dir).unwrap();
    let stale_status = command(&root)
        .current_dir(&root)
        .args(["status", "--view", "repos", "--json"])
        .output()
        .unwrap();
    assert!(stale_status.status.success());
    let stale_status: serde_json::Value = serde_json::from_slice(&stale_status.stdout).unwrap();
    assert_eq!(stale_status["repos"][0]["disk_usage"]["status"], "complete");
    let mut connection = database::connect(&database_path).unwrap();
    let mut expired_origin = disk_usage_cache::origins(&mut connection).unwrap()[&origin.id]
        .clone()
        .unwrap();
    expired_origin.observed_at = Timestamp::before_seconds(10);
    disk_usage_cache::save_if_path_unchanged(
        &mut connection,
        disk_usage_cache::EntityId::Origin(origin.id),
        &path(&origin_dir),
        Some(&expired_origin),
    )
    .unwrap();
    let summary = trees::size_refresh::refresh_workspace(&mut connection, first).unwrap();
    assert_eq!(summary.unavailable, 1);
    assert_eq!(
        disk_usage_cache::origins(&mut connection).unwrap()[&origin.id]
            .as_ref()
            .unwrap()
            .status,
        Completeness::Unavailable
    );
    assert!(trees::size_refresh::refresh_workspace(&mut connection, WorkspaceId::new()).is_err());
    drop(connection);

    let command_help = command(&root).arg("--help").output().unwrap();
    assert!(command_help.status.success());
    let command_help = String::from_utf8(command_help.stdout).unwrap();
    assert!(!command_help
        .lines()
        .any(|line| line.trim_start().starts_with("size ")));
    assert!(!command(&root)
        .args(["size", "refresh"])
        .output()
        .unwrap()
        .status
        .success());
    fs::remove_dir_all(root).unwrap();
}
