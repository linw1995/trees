use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use trees::database;
use trees::domain::{CanonicalPath, WorkspaceId, WorkspaceState};
use trees::storage::find_workspace_by_path;

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!("trees-remove-cli-{}", WorkspaceId::new()))
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

fn repository(path: &Path) {
    fs::create_dir_all(path).expect("repository should be created");
    run_git(path, &["init", "-q"]);
    run_git(path, &["config", "user.email", "trees@example.invalid"]);
    run_git(path, &["config", "user.name", "trees tests"]);
    fs::write(path.join("README"), "test\n").expect("repository file should be written");
    run_git(path, &["add", "README"]);
    run_git(path, &["commit", "-qm", "initial"]);
}

#[test]
fn dry_runs_confirms_and_removes_a_workspace_by_id() {
    let root = test_root();
    let source = root.join("source");
    let workspace_path = root.join("workspace");
    repository(&source);

    let created = command(&root)
        .args([
            "create",
            workspace_path.to_str().unwrap(),
            "--repo",
            source.to_str().unwrap(),
        ])
        .output()
        .expect("create should run");
    assert!(created.status.success());

    let canonical_workspace =
        CanonicalPath::resolve(&workspace_path).expect("workspace should resolve");
    let mut connection =
        database::connect(&database_path(&root)).expect("database should open for lookup");
    let workspace = find_workspace_by_path(&mut connection, &canonical_workspace)
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    drop(connection);

    let unconfirmed = command(&root)
        .args(["remove", &workspace.id.to_string()])
        .output()
        .expect("unconfirmed remove should run");
    assert!(!unconfirmed.status.success());
    assert!(workspace_path.exists());
    assert!(String::from_utf8_lossy(&unconfirmed.stderr)
        .contains("interactive confirmation is unavailable"));

    let dry_run = command(&root)
        .args(["remove", &workspace.id.to_string(), "--dry-run"])
        .output()
        .expect("dry-run remove should run");
    assert!(dry_run.status.success());
    assert!(workspace_path.exists());
    assert!(String::from_utf8_lossy(&dry_run.stdout).contains("reason=eligible"));

    let removed = command(&root)
        .args(["remove", &workspace.id.to_string(), "--yes"])
        .output()
        .expect("confirmed remove should run");
    assert!(removed.status.success());
    assert!(!workspace_path.exists());
    assert!(String::from_utf8_lossy(&removed.stdout).contains("removed=true"));

    let mut connection = database::connect(&database_path(&root)).expect("database should reopen");
    assert_eq!(
        find_workspace_by_path(&mut connection, &canonical_workspace)
            .expect("workspace lookup should succeed")
            .expect("workspace tombstone should remain")
            .state,
        WorkspaceState::Reclaimed
    );
    drop(connection);

    fs::remove_dir_all(root).expect("test root should be removable");
}
