use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use diesel::prelude::*;

use trees::domain::{
    CanonicalPath, OperationId, RepoWorktreeState, WorkspaceManagementMode, WorkspaceState,
};
use trees::git;
use trees::reconciliation::reconcile_workspace;
use trees::storage::{
    find_workspace_by_path, list_events_for_operation, list_repo_worktrees, operation_state,
};
use trees::workspace::{
    create_with_connection, execute_creation, initialize_creation, prepare_create, CreateRequest,
    WorkspaceError,
};

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!("trees-create-{}", uuid::Uuid::now_v7()))
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

fn repository(path: &Path, name: &str) -> PathBuf {
    let path = path.join(name);
    fs::create_dir_all(&path).expect("repository should be created");
    run_git(&path, &["init", "-q"]);
    run_git(&path, &["config", "user.email", "trees@example.invalid"]);
    run_git(&path, &["config", "user.name", "trees tests"]);
    fs::write(path.join("README"), format!("{name}\n")).expect("test file should be written");
    run_git(&path, &["add", "README"]);
    run_git(&path, &["commit", "-qm", "initial"]);
    path
}

#[test]
fn creates_direct_child_worktrees_and_tracks_events() {
    let root = test_root();
    let first = repository(&root, "alpha");
    let second = repository(&root, "beta");
    let workspace_path = root.join("workspace");
    let plan = prepare_create(&CreateRequest {
        workspace_path: workspace_path.clone(),
        repositories: vec![first.clone(), second.clone()],
    })
    .expect("creation plan should be prepared");
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");

    let result =
        create_with_connection(&mut connection, plan).expect("workspace should be created");

    assert_eq!(result.worktree_paths.len(), 2);
    assert!(result
        .worktree_paths
        .iter()
        .all(|path| path.parent() == Some(result.workspace_path.as_path())));
    assert!(result
        .worktree_paths
        .iter()
        .all(|path| path.join("README").exists()));
    assert!(first.join("README").exists());
    assert!(second.join("README").exists());
    for repository in [&first, &second] {
        assert_eq!(
            git::list_worktrees(&CanonicalPath::resolve(repository).unwrap())
                .unwrap()
                .len(),
            2
        );
    }

    let workspace = find_workspace_by_path(
        &mut connection,
        &CanonicalPath::resolve(&workspace_path).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(workspace.state, WorkspaceState::Ready);
    assert_eq!(workspace.management_mode, WorkspaceManagementMode::Manual);
    assert_eq!(
        list_repo_worktrees(&mut connection, &workspace.id)
            .unwrap()
            .len(),
        2
    );
    assert!(list_repo_worktrees(&mut connection, &workspace.id)
        .unwrap()
        .iter()
        .all(|repository| repository.state == RepoWorktreeState::Attached));
    let operation_id = trees::schema::operations::table
        .filter(trees::schema::operations::workspace_id.eq(&workspace.id))
        .select(trees::schema::operations::id)
        .first::<OperationId>(&mut connection)
        .unwrap();
    assert_eq!(
        operation_state(&mut connection, &operation_id).unwrap(),
        Some(trees::domain::OperationState::Succeeded)
    );
    let events = list_events_for_operation(&mut connection, &operation_id).unwrap();
    assert_eq!(events.len(), 13);
    assert!(events
        .windows(2)
        .all(|pair| pair[0].occurred_at <= pair[1].occurred_at));

    let removed = result.worktree_paths[0].clone();
    git::remove_worktree(&CanonicalPath::resolve(&first).unwrap(), &removed)
        .expect("external worktree removal should succeed");
    let summary = reconcile_workspace(&mut connection, &workspace.id, &operation_id)
        .expect("reconciliation should succeed");
    assert_eq!(summary.workspace_state, WorkspaceState::Degraded);
    assert_eq!(summary.changed_worktrees, 1);

    git::remove_worktree(
        &CanonicalPath::resolve(&second).unwrap(),
        &result.worktree_paths[1],
    )
    .expect("remaining worktree should be removable");
    drop(connection);
    fs::remove_file(database_path).expect("state database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn creates_a_single_repository_at_the_workspace_root() {
    let root = test_root();
    let source = repository(&root, "alpha");
    let workspace_path = root.join("workspace");
    let plan = prepare_create(&CreateRequest {
        workspace_path: workspace_path.clone(),
        repositories: vec![source.clone()],
    })
    .expect("creation plan should be prepared");
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");

    let result =
        create_with_connection(&mut connection, plan).expect("workspace should be created");

    assert_eq!(
        result.worktree_paths,
        vec![result.workspace_path.as_path().to_owned()]
    );
    assert!(workspace_path.join("README").exists());
    assert!(!workspace_path.join("alpha").exists());
    assert_eq!(
        git::list_worktrees(&CanonicalPath::resolve(&source).unwrap())
            .unwrap()
            .len(),
        2
    );

    git::remove_worktree(&CanonicalPath::resolve(&source).unwrap(), &workspace_path)
        .expect("worktree should be removable");
    drop(connection);
    fs::remove_file(database_path).expect("state database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn rejects_a_workspace_repo_that_does_not_match_upstream_head() {
    let root = test_root();
    let upstream = repository(&root, "alpha");
    let upstream = CanonicalPath::resolve(&upstream).expect("upstream should resolve");
    let upstream_head = git::inspect_repository(&upstream)
        .expect("upstream should be inspectable")
        .head;
    let existing_workspace = root.join("existing-workspace");
    git::add_detached_worktree(&upstream, &existing_workspace)
        .expect("existing workspace should be created");
    fs::write(existing_workspace.join("README"), "workspace\n")
        .expect("existing workspace should be updated");
    run_git(&existing_workspace, &["commit", "-qam", "workspace"]);
    let workspace_head = git::inspect_repository(
        &CanonicalPath::resolve(&existing_workspace).expect("workspace repo should resolve"),
    )
    .expect("workspace repo should be inspectable")
    .head;
    assert_ne!(workspace_head, upstream_head);

    let target = root.join("new-workspace");
    let error = prepare_create(&CreateRequest {
        workspace_path: target.clone(),
        repositories: vec![existing_workspace.clone()],
    })
    .expect_err("divergent workspace repo should be rejected");
    assert!(matches!(
        error,
        WorkspaceError::Git(git::GitError::UpstreamHeadMismatch { .. })
    ));
    assert!(!target.exists());
    assert_eq!(
        git::inspect_repository(&upstream)
            .expect("upstream should remain inspectable")
            .head,
        upstream_head
    );
    assert_eq!(
        git::inspect_repository(
            &CanonicalPath::resolve(&existing_workspace)
                .expect("workspace repo should still resolve"),
        )
        .expect("workspace repo should remain inspectable")
        .head,
        workspace_head
    );

    git::remove_worktree(&upstream, &existing_workspace)
        .expect("existing worktree should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn failed_creation_leaves_no_partial_workspace() {
    let root = test_root();
    let first = repository(&root, "alpha");
    let second = repository(&root, "beta");
    let second_git = second.join(".git");
    let workspace_path = root.join("workspace");
    let plan = prepare_create(&CreateRequest {
        workspace_path: workspace_path.clone(),
        repositories: vec![first.clone(), second.clone()],
    })
    .expect("creation plan should be prepared");
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");
    let context = initialize_creation(&mut connection, plan).expect("creation should initialize");
    fs::remove_dir_all(second_git).expect("second repository metadata should be removed");

    assert!(execute_creation(&mut connection, &context).is_err());
    assert!(!workspace_path.exists());
    assert_eq!(
        git::list_worktrees(&CanonicalPath::resolve(&first).unwrap())
            .unwrap()
            .len(),
        1
    );
    let workspace = find_workspace_by_path(&mut connection, &context.plan.workspace_path)
        .unwrap()
        .unwrap();
    assert_eq!(workspace.state, WorkspaceState::Failed);

    drop(connection);
    fs::remove_file(database_path).expect("state database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn detects_an_external_worktree_branch_change() {
    let root = test_root();
    let source = repository(&root, "alpha");
    let workspace_path = root.join("workspace");
    let plan = prepare_create(&CreateRequest {
        workspace_path,
        repositories: vec![source.clone()],
    })
    .expect("creation plan should be prepared");
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");
    let result =
        create_with_connection(&mut connection, plan).expect("workspace should be created");
    let workspace = find_workspace_by_path(
        &mut connection,
        &CanonicalPath::resolve(&result.workspace_path).unwrap(),
    )
    .unwrap()
    .unwrap();
    let operation_id = trees::schema::operations::table
        .filter(trees::schema::operations::workspace_id.eq(&workspace.id))
        .select(trees::schema::operations::id)
        .first::<OperationId>(&mut connection)
        .unwrap();

    let worktree_path = &result.worktree_paths[0];
    run_git(worktree_path, &["checkout", "-q", "-b", "external"]);
    let summary = reconcile_workspace(&mut connection, &workspace.id, &operation_id)
        .expect("reconciliation should succeed");
    assert_eq!(summary.workspace_state, WorkspaceState::Degraded);
    assert_eq!(summary.changed_worktrees, 1);
    assert_eq!(
        list_repo_worktrees(&mut connection, &workspace.id).unwrap()[0].state,
        RepoWorktreeState::Diverged
    );

    git::remove_worktree(&CanonicalPath::resolve(&source).unwrap(), worktree_path)
        .expect("changed worktree should be removable");
    drop(connection);
    fs::remove_file(database_path).expect("state database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[cfg(unix)]
fn trees_command(root: &Path) -> Command {
    let home = root.join("home");
    fs::create_dir_all(&home).expect("test home should be created");
    let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
    command
        .env("HOME", home)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"));
    command
}

#[cfg(unix)]
#[test]
fn manual_create_opens_an_explicit_program_in_the_workspace() {
    let root = test_root();
    let source = repository(&root, "source");
    let workspace_path = root.join("workspace");

    let output = trees_command(&root)
        .args([
            "create",
            workspace_path
                .to_str()
                .expect("workspace path should be UTF-8"),
            "--repo",
            source.to_str().expect("repository path should be UTF-8"),
            "--open=pwd",
        ])
        .output()
        .expect("trees create should run");

    assert!(
        output.status.success(),
        "trees create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("program output should be UTF-8")
            .trim(),
        CanonicalPath::resolve(&workspace_path)
            .expect("workspace should resolve")
            .to_string()
    );

    git::remove_worktree(&CanonicalPath::resolve(&source).unwrap(), &workspace_path)
        .expect("created worktree should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[cfg(unix)]
#[test]
fn automatic_create_opens_the_shell_program_in_the_workspace() {
    let root = test_root();
    let source = repository(&root, "source");

    let output = trees_command(&root)
        .args([
            "create",
            "--repo",
            source.to_str().expect("repository path should be UTF-8"),
            "--open",
        ])
        .env("SHELL", "pwd")
        .output()
        .expect("trees create should run");

    assert!(
        output.status.success(),
        "trees create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let workspace_path = PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("program output should be UTF-8")
            .trim(),
    );
    let canonical_workspace =
        CanonicalPath::resolve(&workspace_path).expect("workspace should resolve");
    assert_eq!(workspace_path, canonical_workspace.as_path());

    git::remove_worktree(&CanonicalPath::resolve(&source).unwrap(), &workspace_path)
        .expect("created worktree should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}
