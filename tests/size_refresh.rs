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
fn gc_previews_cached_sizes_and_excludes_removed_workspaces() {
    use diesel::prelude::*;
    use trees::status::disk_usage::{Issue, IssueCode, Observation};

    let root = root();
    let database_path = database_path(&root);
    fs::create_dir_all(database_path.parent().unwrap()).unwrap();
    let mut connection = database::connect(&database_path).unwrap();
    let origin = ensure_origin_repository(
        &mut connection,
        &path(root.join("api/.git")),
        &path(root.join("api")),
    )
    .unwrap();
    let pool = trees::storage::ensure_workspace_pool(
        &mut connection,
        &trees::pool::RepositorySetKey::from_repository_ids(&[origin.id]),
    )
    .unwrap();
    let workspace_path = root.join("workspace");
    fs::create_dir_all(&workspace_path).unwrap();
    fs::write(workspace_path.join("keep"), "preserved").unwrap();
    let (active, _) = add_workspace(&mut connection, &workspace_path, origin.id);
    let (removed, _) = add_workspace(&mut connection, &root.join("removed"), origin.id);
    diesel::update(trees::schema::workspaces::table)
        .set((
            trees::schema::workspaces::management_mode.eq(WorkspaceManagementMode::Automatic),
            trees::schema::workspaces::pool_id.eq(Some(pool.id)),
        ))
        .execute(&mut connection)
        .unwrap();
    diesel::update(trees::schema::workspaces::table.find(removed))
        .set((
            trees::schema::workspaces::state.eq(WorkspaceState::Removed),
            trees::schema::workspaces::disk_usage_json.eq(Some("{}")),
        ))
        .execute(&mut connection)
        .unwrap();
    let before_events = trees::schema::lifecycle_events::table
        .count()
        .get_result::<i64>(&mut connection)
        .unwrap();

    for (status, expected) in [
        (Some(Completeness::Complete), "1.5 KiB"),
        (Some(Completeness::Partial), "1.5 KiB (partial)"),
        (Some(Completeness::Unavailable), "unavailable"),
        (None, "unknown"),
    ] {
        let observation = status.map(|status| Observation {
            observed_at: Timestamp::now(),
            status,
            allocated_bytes: (status != Completeness::Unavailable).then_some(1536),
            issues: (status != Completeness::Complete)
                .then_some(Issue {
                    code: IssueCode::EntryUnreadable,
                    affected_count: Some(1),
                })
                .into_iter()
                .collect(),
        });
        disk_usage_cache::save_if_path_unchanged(
            &mut connection,
            disk_usage_cache::EntityId::Workspace(active),
            &path(&workspace_path),
            observation.as_ref(),
        )
        .unwrap();
        let output = command(&root)
            .args(["gc", "--older-than", "30d", "--dry-run"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let output = String::from_utf8(output.stdout).unwrap();
        let row = output
            .lines()
            .find(|line| line.contains(&active.to_string()))
            .unwrap();
        assert!(row.starts_with("young"));
        assert!(row.contains("1/1 api"));
        assert!(row.contains(expected));
        assert!(output.contains("SIZE"));
        assert!(!output.contains("PATH"));
        assert!(!output.contains(root.to_str().unwrap()));
        assert!(!output.contains(&removed.to_string()));
        assert_eq!(
            trees::schema::lifecycle_events::table
                .count()
                .get_result::<i64>(&mut connection)
                .unwrap(),
            before_events
        );
        assert_eq!(
            disk_usage_cache::workspaces_for_ids(&mut connection, &[active].into()).unwrap()
                [&active],
            observation
        );
    }
    let skipped = command(&root)
        .args(["gc", "--older-than", "30d", "--yes"])
        .output()
        .unwrap();
    assert!(skipped.status.success(), "{skipped:?}");
    let skipped = String::from_utf8(skipped.stdout).unwrap();
    assert_eq!(
        skipped
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["1", "0", "0", "1", "0"]
    );
    assert_eq!(
        fs::read_to_string(workspace_path.join("keep")).unwrap(),
        "preserved"
    );
    diesel::update(trees::schema::workspaces::table.find(active))
        .set(trees::schema::workspaces::state.eq(WorkspaceState::Removed))
        .execute(&mut connection)
        .unwrap();
    let empty = command(&root)
        .args(["gc", "--older-than", "30d", "--yes"])
        .output()
        .unwrap();
    assert!(empty.status.success(), "{empty:?}");
    let empty = String::from_utf8(empty.stdout).unwrap();
    assert!(empty.contains("No automatic workspaces."));
    assert_eq!(
        empty
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["0", "0", "0", "0", "0"]
    );
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lifecycle_refresh_updates_workspace_worktree_and_origin_caches() {
    let root = root();
    let origin_dir = root.join("origin");
    let first_dir = root.join("workspaces/first");
    for directory in [&origin_dir, &first_dir.join("repo")] {
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
    assert_eq!(
        trees::size_refresh::refresh_workspace(&mut connection, first).unwrap(),
        trees::size_refresh::RefreshOutcome::Complete
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
    assert_eq!(summary, trees::size_refresh::RefreshOutcome::Incomplete);
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
