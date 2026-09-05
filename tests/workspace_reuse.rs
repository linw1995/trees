use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use diesel::prelude::*;

use trees::domain::{
    CanonicalPath, RepoWorktreeState, Timestamp, WorkspaceManagementMode, WorkspaceState,
};
use trees::gc;
use trees::git;
use trees::storage::{
    find_origin_repository_by_identity, find_workspace, find_workspace_by_path,
    find_workspace_pool_by_id, list_repo_worktrees, list_workspace_pool_repositories,
};
use trees::workspace::{
    allocate_automatic_workspace, create_with_connection, prepare_automatic, prepare_create,
    provision_automatic, release_automatic_workspace, AutomaticCreateRequest, CreateRequest,
};

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!("trees-reuse-integration-{}", uuid::Uuid::now_v7()))
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

struct AutomaticFixture {
    root: PathBuf,
    database_path: PathBuf,
    connection: diesel::sqlite::SqliteConnection,
    plan: trees::workspace::AutomaticAllocationPlan,
    workspace: trees::storage::WorkspaceRow,
    source: CanonicalPath,
    worktree_path: PathBuf,
    workspace_root: CanonicalPath,
}

fn automatic_fixture() -> AutomaticFixture {
    let root = test_root();
    let source_path = root.join("source");
    repository(&source_path);
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");
    let mut plan = prepare_automatic(&AutomaticCreateRequest {
        repositories: vec![source_path],
        claim_id: None,
    })
    .expect("automatic plan should be prepared");
    plan.workspace_root = CanonicalPath::from_absolute(root.join("managed"))
        .expect("workspace root should be absolute");
    let checkout = provision_automatic(&mut connection, &plan)
        .expect("automatic workspace should be provisioned");
    release_automatic_workspace(&mut connection, &checkout.workspace_path, checkout.claim_id)
        .expect("automatic workspace should be checked in");
    let mut workspace = find_workspace_by_path(&mut connection, &checkout.workspace_path)
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let old = Timestamp::parse("2020-01-01T00:00:00Z").expect("timestamp should parse");
    diesel::update(trees::schema::workspaces::table.find(workspace.id))
        .set((
            trees::schema::workspaces::created_at.eq(old.clone()),
            trees::schema::workspaces::last_checked_in_at.eq(Some(old)),
        ))
        .execute(&mut connection)
        .expect("workspace should be aged");
    workspace = find_workspace(&mut connection, &workspace.id).expect("workspace should exist");
    let worktree = list_repo_worktrees(&mut connection, &workspace.id)
        .expect("worktree lookup should succeed")
        .into_iter()
        .next()
        .expect("workspace should have a worktree");
    let pool_id = workspace
        .pool_key
        .expect("automatic workspace should reference a pool");
    let workspace_root = find_workspace_pool_by_id(&mut connection, &pool_id)
        .expect("workspace pool lookup should succeed")
        .workspace_root;
    plan.workspace_root = workspace_root.clone();
    AutomaticFixture {
        root,
        database_path,
        connection,
        plan,
        workspace,
        source: worktree.source_path,
        worktree_path: worktree.worktree_path.into_path_buf(),
        workspace_root,
    }
}

fn cleanup_fixture(fixture: AutomaticFixture) {
    match git::remove_worktree(&fixture.source, &fixture.worktree_path) {
        Ok(()) | Err(git::GitError::WorktreeNotFound(_)) => {}
        Err(error) => panic!("test worktree should be removable: {error}"),
    }
    drop(fixture.connection);
    fs::remove_file(fixture.database_path).expect("database should be removable");
    fs::remove_dir_all(fixture.root).expect("test root should be removable");
}

#[test]
fn reuses_the_same_automatic_slot_across_checkin_cycles() {
    let mut fixture = automatic_fixture();
    let first = allocate_automatic_workspace(&mut fixture.connection, &fixture.plan)
        .expect("first allocation should succeed");
    release_automatic_workspace(
        &mut fixture.connection,
        &first.workspace_path,
        first.claim_id,
    )
    .expect("first allocation should check in");
    let second = allocate_automatic_workspace(&mut fixture.connection, &fixture.plan)
        .expect("second allocation should succeed");

    let workspace = find_workspace_by_path(&mut fixture.connection, &second.workspace_path)
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let repositories = list_repo_worktrees(&mut fixture.connection, &workspace.id)
        .expect("worktree lookup should succeed");
    assert_eq!(workspace.id, fixture.workspace.id);
    assert_ne!(first.claim_id, second.claim_id);
    assert_eq!(repositories.len(), 1);
    assert_eq!(
        repositories[0].worktree_path.as_path(),
        fixture.worktree_path
    );
    assert_eq!(repositories[0].state, RepoWorktreeState::Attached);

    release_automatic_workspace(
        &mut fixture.connection,
        &second.workspace_path,
        second.claim_id,
    )
    .expect("second allocation should check in");
    cleanup_fixture(fixture);
}

#[test]
fn persists_origin_repositories_once_and_links_them_to_pool() {
    let mut fixture = automatic_fixture();
    let pool_id = fixture
        .workspace
        .pool_key
        .expect("automatic workspace should reference a pool");
    let pool = find_workspace_pool_by_id(&mut fixture.connection, &pool_id)
        .expect("workspace pool lookup should succeed");
    assert_eq!(pool.hash_key, fixture.plan.repository_set.hash_key());
    assert_eq!(
        pool.repositories_json,
        fixture.plan.repository_set.repositories_json()
    );

    let origin = find_origin_repository_by_identity(
        &mut fixture.connection,
        &fixture.plan.repositories[0].repository_identity,
    )
    .expect("origin repository lookup should succeed")
    .expect("origin repository should exist");
    assert_eq!(origin.source_path, fixture.plan.repositories[0].source_path);

    let links = list_workspace_pool_repositories(&mut fixture.connection, &pool_id)
        .expect("pool repository links should be readable");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].repository_id, origin.id);

    let worktrees = list_repo_worktrees(&mut fixture.connection, &fixture.workspace.id)
        .expect("worktree lookup should succeed");
    assert_eq!(worktrees.len(), 1);
    assert_eq!(worktrees[0].origin_repository_id, origin.id);

    cleanup_fixture(fixture);
}

#[test]
fn selects_the_oldest_checked_in_slot_for_an_exact_repository_set() {
    let root = test_root();
    let first_source = root.join("first");
    let second_source = root.join("second");
    repository(&first_source);
    repository(&second_source);
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");
    let mut plan = prepare_automatic(&AutomaticCreateRequest {
        repositories: vec![first_source.clone(), second_source.clone()],
        claim_id: None,
    })
    .expect("automatic plan should be prepared");
    plan.workspace_root = CanonicalPath::from_absolute(root.join("managed"))
        .expect("workspace root should be absolute");
    let first = provision_automatic(&mut connection, &plan).expect("first slot should provision");
    release_automatic_workspace(&mut connection, &first.workspace_path, first.claim_id)
        .expect("first slot should check in");
    let first_workspace = find_workspace_by_path(&mut connection, &first.workspace_path)
        .expect("workspace lookup should succeed")
        .expect("first workspace should exist");
    plan.workspace_root = find_workspace_pool_by_id(
        &mut connection,
        &first_workspace
            .pool_key
            .expect("automatic workspace should reference a pool"),
    )
    .expect("workspace pool lookup should succeed")
    .workspace_root;
    let second = provision_automatic(&mut connection, &plan).expect("second slot should provision");
    release_automatic_workspace(&mut connection, &second.workspace_path, second.claim_id)
        .expect("second slot should check in");
    let old = Timestamp::parse("2020-01-01T00:00:00Z").expect("timestamp should parse");
    let newer = Timestamp::parse("2021-01-01T00:00:00Z").expect("timestamp should parse");
    let first_workspace = find_workspace_by_path(&mut connection, &first.workspace_path)
        .expect("workspace lookup should succeed")
        .expect("first workspace should exist");
    let second_workspace = find_workspace_by_path(&mut connection, &second.workspace_path)
        .expect("workspace lookup should succeed")
        .expect("second workspace should exist");
    diesel::update(trees::schema::workspaces::table.find(first_workspace.id))
        .set(trees::schema::workspaces::last_checked_in_at.eq(Some(old)))
        .execute(&mut connection)
        .expect("first workspace should be aged");
    diesel::update(trees::schema::workspaces::table.find(second_workspace.id))
        .set(trees::schema::workspaces::last_checked_in_at.eq(Some(newer)))
        .execute(&mut connection)
        .expect("second workspace should be aged");

    let selected = allocate_automatic_workspace(&mut connection, &plan)
        .expect("pool allocation should succeed");
    assert_eq!(selected.workspace_path, first.workspace_path);
    release_automatic_workspace(&mut connection, &selected.workspace_path, selected.claim_id)
        .expect("selected workspace should check in");

    for checkout in [first, second] {
        let workspace = find_workspace_by_path(&mut connection, &checkout.workspace_path)
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        for worktree in list_repo_worktrees(&mut connection, &workspace.id)
            .expect("worktree lookup should succeed")
        {
            git::remove_worktree(&worktree.source_path, worktree.worktree_path.as_path())
                .expect("test worktree should be removable");
        }
    }
    drop(connection);
    fs::remove_file(database_path).expect("database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn concurrent_allocation_never_returns_the_same_workspace() {
    let fixture = automatic_fixture();
    let AutomaticFixture {
        root,
        database_path,
        connection,
        plan,
        source,
        ..
    } = fixture;
    drop(connection);

    let first_plan = plan.clone();
    let first_database = database_path.clone();
    let first = std::thread::spawn(move || {
        let mut connection =
            trees::database::connect(&first_database).map_err(|error| error.to_string())?;
        let result = allocate_automatic_workspace(&mut connection, &first_plan)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((result.workspace_path, result.claim_id))
    });
    let second_plan = plan;
    let second_database = database_path.clone();
    let second = std::thread::spawn(move || {
        let mut connection =
            trees::database::connect(&second_database).map_err(|error| error.to_string())?;
        let result = allocate_automatic_workspace(&mut connection, &second_plan)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((result.workspace_path, result.claim_id))
    });
    let first = first
        .join()
        .expect("first allocation thread should join")
        .expect("first allocation should succeed");
    let second = second
        .join()
        .expect("second allocation thread should join")
        .expect("second allocation should succeed");
    assert_ne!(first.0, second.0);

    let mut connection = trees::database::connect(&database_path).expect("database should open");
    for (workspace_path, claim_id) in [first, second] {
        release_automatic_workspace(&mut connection, &workspace_path, claim_id)
            .expect("concurrent allocation should check in");
        let workspace = find_workspace_by_path(&mut connection, &workspace_path)
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        for worktree in list_repo_worktrees(&mut connection, &workspace.id)
            .expect("worktree lookup should succeed")
        {
            git::remove_worktree(&source, worktree.worktree_path.as_path())
                .expect("test worktree should be removable");
        }
    }
    drop(connection);
    fs::remove_file(database_path).expect("database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn keeps_manual_workspaces_out_of_automatic_allocation() {
    let root = test_root();
    let source_path = root.join("source");
    repository(&source_path);
    let database_path = root.join("state.sqlite");
    let mut connection = trees::database::connect(&database_path).expect("database should open");
    let manual = create_with_connection(
        &mut connection,
        prepare_create(&CreateRequest {
            workspace_path: root.join("manual"),
            repositories: vec![source_path.clone()],
        })
        .expect("manual plan should be prepared"),
    )
    .expect("manual workspace should be created");
    let manual_workspace = find_workspace_by_path(&mut connection, &manual.workspace_path)
        .expect("workspace lookup should succeed")
        .expect("manual workspace should exist");
    assert_eq!(
        manual_workspace.management_mode,
        WorkspaceManagementMode::Manual
    );

    let mut plan = prepare_automatic(&AutomaticCreateRequest {
        repositories: vec![source_path.clone()],
        claim_id: None,
    })
    .expect("automatic plan should be prepared");
    plan.workspace_root = CanonicalPath::from_absolute(root.join("managed"))
        .expect("workspace root should be absolute");
    let automatic = allocate_automatic_workspace(&mut connection, &plan)
        .expect("automatic allocation should provision a separate slot");
    assert_ne!(automatic.workspace_path, manual.workspace_path);
    let automatic_workspace = find_workspace_by_path(&mut connection, &automatic.workspace_path)
        .expect("workspace lookup should succeed")
        .expect("automatic workspace should exist");
    assert_eq!(
        automatic_workspace.management_mode,
        WorkspaceManagementMode::Automatic
    );
    release_automatic_workspace(
        &mut connection,
        &automatic.workspace_path,
        automatic.claim_id,
    )
    .expect("automatic workspace should check in");

    for workspace in [manual_workspace, automatic_workspace] {
        for worktree in list_repo_worktrees(&mut connection, &workspace.id)
            .expect("worktree lookup should succeed")
        {
            git::remove_worktree(&worktree.source_path, worktree.worktree_path.as_path())
                .expect("test worktree should be removable");
        }
    }
    drop(connection);
    fs::remove_file(database_path).expect("database should be removable");
    fs::remove_dir_all(root).expect("test root should be removable");
}

#[test]
fn dry_run_scan_uses_a_read_only_connection_and_preserves_state() {
    let fixture = automatic_fixture();
    let workspace_id = fixture.workspace.id;
    let database_path = fixture.database_path.clone();
    let workspace_root = fixture.workspace_root.clone();
    let mut connection = fixture.connection;
    let before_events = trees::schema::lifecycle_events::table
        .count()
        .get_result::<i64>(&mut connection)
        .expect("event count should be readable");
    drop(connection);

    let mut read_only =
        trees::database::connect_read_only(&database_path).expect("read-only database should open");
    let before_workspace = find_workspace(&mut read_only, &workspace_id)
        .expect("workspace should exist")
        .state;
    let result = gc::scan(
        &mut read_only,
        &workspace_root,
        "30d".parse().expect("duration should parse"),
    )
    .expect("dry-run scan should succeed");
    let after_events = trees::schema::lifecycle_events::table
        .count()
        .get_result::<i64>(&mut read_only)
        .expect("event count should be readable");
    let after_workspace = find_workspace(&mut read_only, &workspace_id)
        .expect("workspace should exist")
        .state;
    assert_eq!(result.counts.safe_to_reclaim, 1);
    assert_eq!(before_events, after_events);
    assert_eq!(before_workspace, after_workspace);

    drop(read_only);
    match git::remove_worktree(&fixture.source, &fixture.worktree_path) {
        Ok(()) | Err(git::GitError::WorktreeNotFound(_)) => {}
        Err(error) => panic!("test worktree should be removable: {error}"),
    }
    fs::remove_file(fixture.database_path).expect("database should be removable");
    fs::remove_dir_all(fixture.root).expect("test root should be removable");
}

#[test]
fn force_gc_reclaims_diverged_worktrees_and_extra_content() {
    let fixture = automatic_fixture();
    run_git(
        &fixture.worktree_path,
        &["checkout", "-q", "-b", "external"],
    );
    fs::write(
        fixture.workspace.canonical_path.as_path().join("extra"),
        "extra\n",
    )
    .expect("extra content should be written");
    let mut connection = fixture.connection;
    let report = gc::execute(
        &mut connection,
        &fixture.workspace_root,
        "30d".parse().expect("duration should parse"),
        true,
    )
    .expect("forced GC should succeed");
    assert_eq!(
        report.reclaimed.as_slice(),
        std::slice::from_ref(&fixture.workspace.canonical_path)
    );
    assert!(!fixture.workspace.canonical_path.as_path().exists());
    assert_eq!(
        find_workspace(&mut connection, &fixture.workspace.id)
            .expect("workspace should exist")
            .state,
        WorkspaceState::Reclaimed
    );
    assert_eq!(
        git::list_worktrees(&fixture.source)
            .expect("source worktrees should be readable")
            .len(),
        1
    );

    drop(connection);
    fs::remove_file(fixture.database_path).expect("database should be removable");
    fs::remove_dir_all(fixture.root).expect("test root should be removable");
}

#[test]
fn force_gc_keeps_an_unexpired_checkout_lease() {
    let mut fixture = automatic_fixture();
    let checkout = allocate_automatic_workspace(&mut fixture.connection, &fixture.plan)
        .expect("allocation should succeed");

    let report = gc::execute(
        &mut fixture.connection,
        &fixture.workspace_root,
        "30d".parse().expect("duration should parse"),
        true,
    )
    .expect("forced GC should complete with a skip");
    assert!(report.reclaimed.is_empty());
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].reason, gc::GcCandidateReason::CheckedOut);
    assert!(fixture.workspace.canonical_path.as_path().exists());
    assert_eq!(
        trees::storage::find_workspace_claim(&mut fixture.connection, &fixture.workspace.id)
            .expect("lease lookup should succeed")
            .expect("checkout lease should remain active")
            .id,
        checkout.claim_id
    );

    release_automatic_workspace(
        &mut fixture.connection,
        &checkout.workspace_path,
        checkout.claim_id,
    )
    .expect("checkout should be released");
    cleanup_fixture(fixture);
}

#[cfg(unix)]
#[test]
fn records_a_partial_gc_failure_without_reporting_reclamation() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = automatic_fixture();
    let permissions = fs::Permissions::from_mode(0o500);
    fs::set_permissions(&fixture.workspace.canonical_path, permissions)
        .expect("workspace permissions should be restricted");
    let mut connection = fixture.connection;
    let report = gc::execute(
        &mut connection,
        &fixture.workspace_root,
        "30d".parse().expect("duration should parse"),
        false,
    )
    .expect("GC should report the physical failure");
    assert!(report.reclaimed.is_empty());
    assert_eq!(report.failed.len(), 1);
    assert!(fixture.workspace.canonical_path.as_path().exists());
    assert_eq!(
        find_workspace(&mut connection, &fixture.workspace.id)
            .expect("workspace should exist")
            .state,
        WorkspaceState::Failed
    );

    fs::set_permissions(
        &fixture.workspace.canonical_path,
        fs::Permissions::from_mode(0o700),
    )
    .expect("workspace permissions should be restored");
    if git::list_worktrees(&fixture.source)
        .expect("source worktrees should be readable")
        .iter()
        .any(|worktree| worktree.path.as_path() == fixture.worktree_path.as_path())
    {
        git::remove_worktree(&fixture.source, &fixture.worktree_path)
            .expect("test worktree should be removable");
    }
    drop(connection);
    fs::remove_file(fixture.database_path).expect("database should be removable");
    fs::remove_dir_all(fixture.root).expect("test root should be removable");
}
