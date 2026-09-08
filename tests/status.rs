use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use diesel::prelude::*;

use trees::database;
use trees::domain::{
    CanonicalPath, ClaimId, JsonDocument, PoolId, RepoWorktreeId, RepoWorktreeState, Timestamp,
    WorkspaceId, WorkspaceManagementMode, WorkspaceState,
};
use trees::storage::{
    ensure_origin_repository, find_workspace, insert_managed_workspace, insert_repo_worktree,
    insert_workspace_claim, insert_workspace_pool, insert_workspace_pool_repositories,
    persist_operation_intent, NewManagedWorkspace, NewRepoWorktree, NewWorkspaceClaim,
    NewWorkspacePool, NewWorkspacePoolRepository, OperationIntent,
};

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!("trees-status-cli-{}", WorkspaceId::new()))
}

fn command(root: &Path) -> Command {
    let home = root.join("home");
    fs::create_dir_all(&home).expect("test home should be created");
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
    root.join("state").join("trees").join("db.sqlite")
}

#[cfg(target_os = "macos")]
fn database_path(root: &Path) -> PathBuf {
    root.join("home")
        .join("Library")
        .join("Application Support")
        .join("trees")
        .join("db.sqlite")
}

#[cfg(target_os = "windows")]
fn database_path(root: &Path) -> PathBuf {
    root.join("local-app-data").join("trees").join("db.sqlite")
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn database_path(root: &Path) -> PathBuf {
    root.join("home")
        .join(".local")
        .join("state")
        .join("trees")
        .join("db.sqlite")
}

fn path(value: impl AsRef<Path>) -> CanonicalPath {
    CanonicalPath::from_absolute(value).expect("test path should be absolute")
}

fn insert_workspace(
    connection: &mut diesel::sqlite::SqliteConnection,
    workspace_path: CanonicalPath,
    state: WorkspaceState,
    management_mode: WorkspaceManagementMode,
    pool_id: Option<PoolId>,
) -> WorkspaceId {
    let workspace_id = WorkspaceId::new();
    let now = Timestamp::now();
    insert_managed_workspace(
        connection,
        &NewManagedWorkspace {
            id: workspace_id,
            canonical_path: workspace_path,
            state,
            created_at: now.clone(),
            updated_at: now.clone(),
            last_reconciled_at: Some(now.clone()),
            management_mode,
            pool_id,
            last_released_at: None,
            reclaimed_at: (state == WorkspaceState::Reclaimed).then_some(now),
        },
    )
    .expect("workspace should be inserted");
    workspace_id
}

#[test]
fn missing_database_is_a_successful_empty_result_without_side_effects() {
    let root = test_root();

    let human = command(&root)
        .arg("status")
        .output()
        .expect("status should run");
    assert!(human.status.success());
    assert_eq!(human.stdout, b"No workspace pools.\n");
    assert!(human.stderr.is_empty());
    assert!(!database_path(&root).exists());

    let json = command(&root)
        .args(["status", "--json"])
        .output()
        .expect("JSON status should run");
    assert!(json.status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("stdout should be valid JSON");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["view"], "pools");
    assert!(value["snapshot_at"].is_string());
    assert_eq!(value["pools"], serde_json::json!([]));
    assert!(json.stderr.is_empty());
    assert!(!database_path(&root).exists());

    let workspaces = command(&root)
        .args(["status", "--view", "workspaces"])
        .output()
        .expect("workspace status should run");
    assert!(workspaces.status.success());
    assert_eq!(workspaces.stdout, b"No workspaces.\n");

    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn status_renders_ordered_persisted_state_and_preserves_storage() {
    let root = test_root();
    let database_path = database_path(&root);
    fs::create_dir_all(database_path.parent().unwrap()).expect("state directory should exist");
    let mut connection = database::connect(&database_path).expect("database should open");
    let pool_id = PoolId::new();
    insert_workspace_pool(
        &mut connection,
        &NewWorkspacePool {
            id: pool_id,
            hash_key: "status-pool".to_owned(),
            repository_ids: "[]".to_owned(),
        },
    )
    .expect("pool should be inserted");

    let automatic_path = path(root.join("workspaces").join("zeta"));
    let automatic_id = insert_workspace(
        &mut connection,
        automatic_path.clone(),
        WorkspaceState::Ready,
        WorkspaceManagementMode::Automatic,
        Some(pool_id),
    );
    let claim_id = ClaimId::new();
    insert_workspace_claim(
        &mut connection,
        &NewWorkspaceClaim {
            id: claim_id,
            workspace_id: automatic_id,
            claimed_at: Timestamp::now(),
        },
    )
    .expect("claim should be inserted");
    let operation = OperationIntent::new(
        automatic_id,
        "release",
        Timestamp::parse("2000-01-01T00:00:00Z").unwrap(),
        "release workspace claim",
        JsonDocument::parse("{}").unwrap(),
    );
    persist_operation_intent(&mut connection, &operation).expect("operation should be inserted");

    let manual_path = path(root.join("manual").join("alpha"));
    let manual_id = insert_workspace(
        &mut connection,
        manual_path.clone(),
        WorkspaceState::Degraded,
        WorkspaceManagementMode::Manual,
        None,
    );
    let origin = ensure_origin_repository(
        &mut connection,
        &path(root.join("origins").join("example.git")),
        &path(root.join("origins").join("example")),
    )
    .expect("origin should be inserted");
    insert_workspace_pool_repositories(
        &mut connection,
        &[NewWorkspacePoolRepository {
            pool_id,
            repository_id: origin.id,
        }],
    )
    .expect("pool repository should be inserted");
    insert_repo_worktree(
        &mut connection,
        &NewRepoWorktree {
            id: RepoWorktreeId::new(),
            workspace_id: manual_id,
            origin_repository_id: origin.id,
            worktree_path: path(root.join("missing-worktrees").join("example")),
            state: RepoWorktreeState::Dirty,
            last_head: Some("0123456789abcdef".to_owned()),
            last_observed_at: Timestamp::now(),
        },
    )
    .expect("repo worktree should be inserted");

    let reclaimed_path = path(root.join("workspaces").join("reclaimed"));
    let reclaimed_id = insert_workspace(
        &mut connection,
        reclaimed_path.clone(),
        WorkspaceState::Reclaimed,
        WorkspaceManagementMode::Automatic,
        Some(pool_id),
    );
    let before_events = trees::schema::lifecycle_events::table
        .count()
        .get_result::<i64>(&mut connection)
        .expect("event count should be readable");
    drop(connection);

    let human = command(&root)
        .arg("status")
        .env("PATH", "")
        .output()
        .expect("status should run");
    assert!(
        human.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human = String::from_utf8(human.stdout).expect("human output should be UTF-8");
    assert!(human.starts_with("REPOS"));
    assert!(human.contains("example"));
    assert!(human.contains("0/1/0"));
    assert!(!human.contains('\u{1b}'));
    assert!(!human.contains("degraded"));
    assert!(!human.contains(manual_path.as_path().to_str().unwrap()));
    assert!(!human.contains(automatic_path.as_path().to_str().unwrap()));
    assert!(!human.contains(reclaimed_path.as_path().to_str().unwrap()));

    let details = command(&root)
        .args(["status", "--view", "workspaces"])
        .env("PATH", "")
        .output()
        .expect("workspace status should run");
    assert!(details.status.success());
    let details = String::from_utf8(details.stdout).expect("details should be UTF-8");
    assert!(details.starts_with("STATE"));
    assert!(details.contains("degraded"));
    assert!(details.contains("0/1 example"));
    assert!(details.contains("🤖"));
    assert!(details.contains("👤"));
    assert!(details.contains(manual_path.as_path().to_str().unwrap()));
    assert!(details.contains(automatic_path.as_path().to_str().unwrap()));
    assert!(!details.contains(reclaimed_path.as_path().to_str().unwrap()));
    assert!(
        details
            .find(manual_path.as_path().to_str().unwrap())
            .unwrap()
            < details
                .find(automatic_path.as_path().to_str().unwrap())
                .unwrap()
    );

    let pools_json = command(&root)
        .args(["status", "--json"])
        .output()
        .expect("pool JSON status should run");
    assert!(pools_json.status.success());
    let pools_value: serde_json::Value =
        serde_json::from_slice(&pools_json.stdout).expect("pool JSON should be valid");
    assert_eq!(pools_value["view"], "pools");
    assert!(pools_value["pools"][0].get("allocated").is_none());
    assert_eq!(pools_value["pools"][0]["available"], 0);
    assert_eq!(pools_value["pools"][0]["capacity"], 1);
    assert_eq!(pools_value["pools"][0]["abnormal"], 0);

    let json = command(&root)
        .args(["status", "--view", "workspaces", "--all", "--json"])
        .env("PATH", "")
        .output()
        .expect("JSON status should run");
    assert!(
        json.status.success(),
        "JSON status failed: {}",
        String::from_utf8_lossy(&json.stderr)
    );
    assert_eq!(json.stdout.iter().filter(|byte| **byte == b'\n').count(), 1);
    let value: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("stdout should be one JSON document");
    assert_eq!(value["view"], "workspaces");
    let workspaces = value["workspaces"].as_array().unwrap();
    assert_eq!(workspaces.len(), 3);
    assert_eq!(workspaces[0]["workspace_id"], manual_id.to_string());
    assert_eq!(workspaces[0]["repo_worktrees"][0]["state"], "dirty");
    assert_eq!(workspaces[1]["workspace_id"], reclaimed_id.to_string());
    assert_eq!(workspaces[1]["state"], "reclaimed");
    assert_eq!(workspaces[2]["workspace_id"], automatic_id.to_string());
    assert_eq!(workspaces[2]["claim"]["claim_id"], claim_id.to_string());
    assert_eq!(
        workspaces[2]["current_operation"]["lease_status"],
        "expired"
    );
    assert!(json.stderr.is_empty());

    let mut connection =
        database::connect_read_only(&database_path).expect("read-only database should reopen");
    let after_events = trees::schema::lifecycle_events::table
        .count()
        .get_result::<i64>(&mut connection)
        .expect("event count should remain readable");
    assert_eq!(after_events, before_events);
    assert_eq!(
        find_workspace(&mut connection, &automatic_id)
            .expect("automatic workspace should remain readable")
            .state,
        WorkspaceState::Ready
    );
    drop(connection);

    fs::remove_file(database_path).expect("database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn all_requires_the_workspace_view_without_opening_storage() {
    let root = test_root();

    for arguments in [
        vec!["status", "--all"],
        vec!["status", "--view", "pools", "--all"],
    ] {
        let output = command(&root)
            .args(arguments)
            .output()
            .expect("status should run");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "Error: --all requires --view workspaces\n"
        );
        assert!(!database_path(&root).exists());
    }

    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn corrupt_database_fails_without_partial_json() {
    let root = test_root();
    let database_path = database_path(&root);
    fs::create_dir_all(database_path.parent().unwrap()).expect("state directory should exist");
    fs::write(&database_path, "not a sqlite database").expect("corrupt database should be written");

    let output = command(&root)
        .args(["status", "--json"])
        .output()
        .expect("status should run");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("Error: "));

    fs::remove_dir_all(root).expect("test root should be removable");
}
