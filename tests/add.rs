use std::fs;
use std::path::Path;
use std::process::Command;

use trees as trees_library;
use trees::add;
use trees::domain::{CanonicalPath, WorkspaceId};
use trees::storage;

#[path = "support/add.rs"]
mod support;
use support::{clear_event_failure, git, install_event_failure, Fixture};

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

#[test]
fn rejected_paths_and_locked_moves_preserve_original_content() {
    for scenario in ["collision", "lock", "missing", "nested"] {
        let mut fixture = Fixture::new();
        let original = fs::read(fixture.path.as_path().join("README")).unwrap();
        let input = match scenario {
            "collision" => Fixture::repository(&fixture.root.join("other"), "api"),
            "lock" => {
                git(
                    &fixture.api,
                    &["worktree", "lock", fixture.path.as_path().to_str().unwrap()],
                );
                fixture.web.clone()
            }
            "missing" => {
                fs::remove_file(fixture.path.as_path().join(".git")).unwrap();
                fixture.web.clone()
            }
            "nested" => {
                storage::insert_workspace(
                    &mut fixture.db,
                    &storage::NewWorkspace {
                        id: WorkspaceId::new(),
                        canonical_path: CanonicalPath::from_absolute(
                            fixture.path.as_path().join("inner"),
                        )
                        .unwrap(),
                        state: trees::domain::WorkspaceState::Removed,
                        created_at: trees::domain::Timestamp::now(),
                        updated_at: trees::domain::Timestamp::now(),
                        last_reconciled_at: None,
                    },
                )
                .unwrap();
                fixture.web.clone()
            }
            _ => unreachable!(),
        };
        let request = fixture.request(vec![input]);
        assert!(
            add::execute(&mut fixture.db, request).is_err(),
            "{scenario}"
        );
        assert_eq!(
            fs::read(fixture.path.as_path().join("README")).unwrap(),
            original,
            "{scenario}"
        );
        assert_eq!(
            storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace)
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn existing_child_paths_and_ignored_files_survive_expansion() {
    let mut fixture = Fixture::new();
    fs::write(fixture.path.as_path().join(".gitignore"), "cache\n").unwrap();
    fs::write(fixture.path.as_path().join("cache"), "ignored content").unwrap();
    git(fixture.path.as_path(), &["switch", "-c", "feature"]);
    git(fixture.path.as_path(), &["add", ".gitignore"]);
    git(
        fixture.path.as_path(),
        &[
            "-c",
            "user.email=trees@example.invalid",
            "-c",
            "user.name=trees tests",
            "commit",
            "-qm",
            "ignore cache",
        ],
    );
    let head = git(fixture.path.as_path(), &["rev-parse", "HEAD"]);
    let request = fixture.request(vec![fixture.web.clone()]);
    add::execute(&mut fixture.db, request).unwrap();
    let api = fixture.path.as_path().join("api");
    assert_eq!(git(&api, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&api, &["branch", "--show-current"]), "feature");
    assert_eq!(
        fs::read_to_string(api.join("cache")).unwrap(),
        "ignored content"
    );
    let shared = Fixture::repository(&fixture.root, "shared");
    let request = fixture.request(vec![shared]);
    let result = add::execute(&mut fixture.db, request).unwrap();
    assert!(result.relocated.is_empty());
    assert!(api.join("README").exists());
    assert!(fixture.path.as_path().join("web/README").exists());
    assert!(fixture.path.as_path().join("shared/README").exists());
}

#[test]
fn faulted_journal_boundaries_never_publish_partial_membership() {
    let events = [
        "operation_started",
        "workspace_add_planned",
        "operation_step_started",
        "worktree_relocated",
        "workspace_container_created",
        "worktree_add_intended",
        "worktree_directory_created",
        "worktree_add_completed",
        "worktree_added",
        "worktree_observed",
        "workspace_repositories_added",
        "operation_succeeded",
    ];
    for event in events {
        let mut fixture = Fixture::new();
        install_event_failure(&mut fixture.db, "fail_add_event", event);
        let request = fixture.request(vec![fixture.web.clone()]);
        assert!(add::execute(&mut fixture.db, request).is_err(), "{event}");
        clear_event_failure(&mut fixture.db, "fail_add_event");
        let rows = storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace).unwrap();
        assert!(
            rows.iter().all(
                |repo| repo.origin_repository_id == rows[0].origin_repository_id
                    || repo.state == trees::domain::RepoWorktreeState::Failed
            ),
            "{event}: {rows:?}"
        );
        // A failed ownership-journal write deliberately requires repair of the empty container.
        if event == "workspace_container_created" {
            assert!(fs::read_dir(fixture.path.as_path())
                .unwrap()
                .next()
                .is_none());
            fs::remove_dir(fixture.path.as_path()).unwrap();
        }
        if event == "worktree_directory_created" {
            let path = fixture.path.as_path().join("web");
            assert!(fs::read_dir(&path).unwrap().next().is_none());
            fs::remove_dir(path).unwrap();
        }
        let request = fixture.request(vec![fixture.web.clone()]);
        let result = add::execute(&mut fixture.db, request)
            .unwrap_or_else(|error| panic!("{event}: {error:?}"));
        assert_eq!(result.workspace_id, fixture.workspace);
        assert_eq!(
            fs::read_to_string(fixture.path.as_path().join("api/README")).unwrap(),
            "initial\n"
        );
        assert_eq!(
            storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace)
                .unwrap()
                .len(),
            2
        );
    }
}

#[cfg(unix)]
#[test]
fn symlink_destinations_are_not_overwritten() {
    let mut fixture = Fixture::new();
    let request = fixture.request(vec![fixture.web.clone()]);
    add::execute(&mut fixture.db, request).unwrap();
    let shared = Fixture::repository(&fixture.root, "shared");
    let target = fixture.root.join("user-data");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), "keep").unwrap();
    std::os::unix::fs::symlink(&target, fixture.path.as_path().join("shared")).unwrap();
    let request = fixture.request(vec![shared]);
    assert!(add::execute(&mut fixture.db, request).is_err());
    assert_eq!(fs::read_to_string(target.join("keep")).unwrap(), "keep");
}

#[test]
fn url_inputs_preserve_order_and_known_sources_work_offline() {
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
    let url = format!("file://{}", fixture.web.display());
    let rejected = cli(&fixture.root)
        .args(["add", path, "--repo", &url, "--offline", "--json"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    let added = json_output(cli(&fixture.root).args([
        "add", path, "--repo", &url, "--repo", "api", "--repo", &url, "--json",
    ]));
    let repositories = added["repositories"].as_array().unwrap();
    assert_eq!(repositories.len(), 2);
    assert_eq!(repositories[0]["result"], "added");
    assert_eq!(repositories[1]["result"], "already_present");
    let repeated =
        json_output(cli(&fixture.root).args(["add", path, "--repo", &url, "--offline", "--json"]));
    assert_eq!(repeated["repositories"][0]["result"], "already_present");
    let conflict = cli(&fixture.root)
        .args(["add", path, "--workspace-dir", path, "--repo", "web"])
        .output()
        .unwrap();
    assert!(!conflict.status.success());
    let missing = cli(&fixture.root)
        .current_dir(path)
        .args([
            "add",
            "--workspace-id",
            &WorkspaceId::new().to_string(),
            "--repo",
            "web",
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
}

#[test]
fn existing_destination_pool_is_reused_without_changing_other_claims() {
    let mut fixture = Fixture::automatic();
    let mut pair = trees::workspace::prepare_automatic(&trees::workspace::AutomaticCreateRequest {
        repositories: vec![fixture.api.clone(), fixture.web.clone()],
        offline: true,
    })
    .unwrap();
    pair.workspace_root = CanonicalPath::from_absolute(fixture.root.join("managed")).unwrap();
    let other = trees::workspace::allocate_automatic_workspace(&mut fixture.db, &pair).unwrap();
    let request = fixture.request(vec![fixture.web.clone()]);
    let result = add::execute(&mut fixture.db, request).unwrap();
    assert_eq!(result.pool_id, Some(other.pool_id));
    assert_ne!(result.claim_id, Some(other.claim_id));
    let other_workspace = storage::find_workspace_by_path(&mut fixture.db, &other.workspace_path)
        .unwrap()
        .unwrap();
    assert_eq!(
        storage::find_workspace_claim(&mut fixture.db, &other_workspace.id)
            .unwrap()
            .unwrap()
            .id,
        other.claim_id
    );
    trees::workspace::release_automatic_workspace(
        &mut fixture.db,
        &fixture.path,
        result.claim_id.unwrap(),
    )
    .unwrap();
    let reused = trees::workspace::allocate_automatic_workspace(&mut fixture.db, &pair).unwrap();
    assert_eq!(reused.workspace_path, fixture.path);
    assert_ne!(reused.claim_id, result.claim_id.unwrap());
}

#[test]
fn final_transaction_failure_keeps_original_pool_and_claim() {
    use diesel::connection::SimpleConnection;
    let mut fixture = Fixture::automatic();
    let before = storage::find_workspace(&mut fixture.db, &fixture.workspace).unwrap();
    let claim = storage::find_workspace_claim(&mut fixture.db, &fixture.workspace)
        .unwrap()
        .unwrap();
    fixture.db.batch_execute("CREATE TRIGGER fail_publication BEFORE INSERT ON lifecycle_events WHEN NEW.event_type = 'workspace_repositories_added' BEGIN SELECT RAISE(ABORT, 'injected publication failure'); END;").unwrap();
    let request = fixture.request(vec![fixture.web.clone()]);
    assert!(add::execute(&mut fixture.db, request).is_err());
    let after = storage::find_workspace(&mut fixture.db, &fixture.workspace).unwrap();
    assert_eq!(after.pool_id, before.pool_id);
    assert_eq!(after.last_released_at, before.last_released_at);
    assert_eq!(
        storage::find_workspace_claim(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .unwrap()
            .id,
        claim.id
    );
    assert_eq!(
        storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .len(),
        1
    );
    assert!(fixture.path.as_path().join(".git").is_file());
}

#[test]
fn later_batch_failure_restores_the_original_single_worktree() {
    use diesel::connection::SimpleConnection;
    let mut fixture = Fixture::new();
    let shared = Fixture::repository(&fixture.root, "shared");
    fixture.db.batch_execute("CREATE TRIGGER fail_second_add BEFORE INSERT ON lifecycle_events WHEN NEW.event_type = 'worktree_add_intended' AND NEW.details_json LIKE '%/shared%' BEGIN SELECT RAISE(ABORT, 'injected second repository failure'); END;").unwrap();
    let request = fixture.request(vec![fixture.web.clone(), shared]);
    assert!(add::execute(&mut fixture.db, request).is_err());
    assert!(fixture.path.as_path().join(".git").is_file());
    assert!(!fixture.path.as_path().join("web").exists());
    assert_eq!(
        storage::list_repo_worktrees(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .len(),
        1
    );
    assert!(
        storage::find_running_operation(&mut fixture.db, &fixture.workspace)
            .unwrap()
            .is_none()
    );
}

#[test]
fn failed_second_move_preserves_original_content_and_can_resume() {
    use diesel::connection::SimpleConnection;
    for event in ["operation_step_started", "worktree_relocated"] {
        let mut fixture = Fixture::new();
        fixture.db.batch_execute(&format!("CREATE TRIGGER fail_second_move BEFORE INSERT ON lifecycle_events WHEN NEW.event_type = '{event}' AND NEW.details_json LIKE '%/workspace/api%' BEGIN SELECT RAISE(ABORT, 'injected second move failure'); END;")).unwrap();
        let request = fixture.request(vec![fixture.web.clone()]);
        assert!(add::execute(&mut fixture.db, request).is_err());
        fixture
            .db
            .batch_execute("DROP TRIGGER fail_second_move;")
            .unwrap();
        let request = fixture.request(vec![fixture.web.clone()]);
        add::execute(&mut fixture.db, request).unwrap();
        assert_eq!(
            fs::read_to_string(fixture.path.as_path().join("api/README")).unwrap(),
            "initial\n"
        );
    }
}

#[test]
fn ambiguous_registered_names_fail_without_changing_the_workspace() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("home")).unwrap();
    Fixture::repository(&fixture.root.join("one"), "shared");
    Fixture::repository(&fixture.root.join("two"), "shared");
    json_output(cli(&fixture.root).args([
        "create",
        "first",
        "--repo",
        "one/shared",
        "--offline",
        "--json",
    ]));
    json_output(cli(&fixture.root).args([
        "create",
        "second",
        "--repo",
        "two/shared",
        "--offline",
        "--json",
    ]));
    let created = json_output(cli(&fixture.root).args([
        "create",
        "target",
        "--repo",
        "api",
        "--offline",
        "--json",
    ]));
    let path = created["workspace_path"].as_str().unwrap();
    let rejected = cli(&fixture.root)
        .args(["add", path, "--repo", "shared", "--offline", "--json"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert!(Path::new(path).join(".git").is_file());
}

#[test]
fn a_failed_move_without_mutation_has_failed_terminal_state() {
    let mut fixture = Fixture::new();
    git(
        &fixture.api,
        &["worktree", "lock", fixture.path.as_path().to_str().unwrap()],
    );
    let request = fixture.request(vec![fixture.web.clone()]);
    assert!(add::execute(&mut fixture.db, request).is_err());
    use diesel::prelude::*;
    let operation = trees::schema::operations::table
        .filter(trees::schema::operations::kind.eq("add"))
        .select(trees::schema::operations::id)
        .first::<trees::domain::OperationId>(&mut fixture.db)
        .unwrap();
    assert_eq!(
        storage::operation_state(&mut fixture.db, &operation).unwrap(),
        Some(trees::domain::OperationState::Failed)
    );
}

#[test]
fn failed_source_provisioning_retains_published_origin_audit_details() {
    use diesel::prelude::*;
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
    let valid = format!("file://{}", fixture.web.display());
    let missing = format!("file://{}/missing", fixture.root.display());
    let output = cli(&fixture.root)
        .args(["add", path, "--repo", &valid, "--repo", &missing, "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    #[cfg(target_os = "macos")]
    let state = fixture
        .root
        .join("home/Library/Application Support/trees/db.sqlite");
    #[cfg(target_os = "linux")]
    let state = fixture.root.join("state/trees/db.sqlite");
    #[cfg(target_os = "windows")]
    let state = fixture.root.join("local-app-data/trees/db.sqlite");
    let mut db = trees::database::connect(&state).unwrap();
    let operation = trees::schema::operations::table
        .filter(trees::schema::operations::kind.eq("add"))
        .select(trees::schema::operations::id)
        .first::<trees::domain::OperationId>(&mut db)
        .unwrap();
    let events = storage::list_events_for_operation(&mut db, &operation).unwrap();
    let published = events
        .iter()
        .find(|event| event.event_type == "workspace_add_origin_available")
        .unwrap();
    assert!(published
        .details_json
        .as_ref()
        .unwrap()
        .to_string()
        .contains("origin_repository_id"));
    assert!(published
        .details_json
        .as_ref()
        .unwrap()
        .to_string()
        .contains("/web"));
    assert_eq!(
        storage::operation_state(&mut db, &operation).unwrap(),
        Some(trees::domain::OperationState::Failed)
    );
}
