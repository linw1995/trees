#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use diesel::prelude::*;
use serde_json::Value;
use trees::domain::{JsonDocument, OperationState, Timestamp, WorkspaceId, WorkspaceState};
use trees::storage::{begin_operation, find_operation_lease, operation_state, OperationIntent};

struct Fixture {
    root: PathBuf,
    workspace: PathBuf,
    id: WorkspaceId,
}

impl Fixture {
    fn new(automatic: bool) -> Self {
        let root = std::env::temp_dir().join(format!("trees-reconcile-{}", WorkspaceId::new()));
        fs::create_dir_all(root.join("home")).unwrap();
        let mut fixture = Self {
            workspace: root.join("workspace"),
            root,
            id: WorkspaceId::new(),
        };
        for name in ["alpha", "beta"] {
            let source = fixture.root.join(name);
            fs::create_dir(&source).unwrap();
            for args in [
                vec!["init", "-q"],
                vec!["config", "user.email", "trees@example.invalid"],
                vec!["config", "user.name", "trees tests"],
                vec!["commit", "--allow-empty", "-qm", "initial"],
            ] {
                success(
                    Command::new("git")
                        .arg("-C")
                        .arg(&source)
                        .args(args)
                        .output()
                        .unwrap(),
                );
            }
        }
        let mut command = fixture.command();
        command.args([
            "create",
            "--offline",
            "--json",
            "--repo",
            "alpha",
            "--repo",
            "beta",
        ]);
        if !automatic {
            command.arg(&fixture.workspace);
        }
        let output = success(command.output().unwrap());
        let created: Value = serde_json::from_slice(&output.stdout).unwrap();
        fixture.workspace = created["workspace_path"].as_str().unwrap().into();
        fixture.id = fixture.status()["target_workspace"]["workspace_id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        fixture
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
        command
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CONFIG_HOME", self.root.join("config"));
        command
    }

    fn connection(&self) -> SqliteConnection {
        #[cfg(target_os = "macos")]
        let base = self.root.join("home/Library/Application Support");
        #[cfg(target_os = "linux")]
        let base = self.root.join("state");
        trees::database::connect(&base.join("trees/db.sqlite")).unwrap()
    }

    fn status(&self) -> Value {
        let output = success(
            self.command()
                .args([
                    "status",
                    "--json",
                    "--no-hooks",
                    "--view",
                    "workspaces",
                    "--workspace-dir",
                ])
                .arg(&self.workspace)
                .output()
                .unwrap(),
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn reconcile(&self) -> Value {
        let output = success(
            self.command()
                .args([
                    "reconcile",
                    "--json",
                    "--workspace-id",
                    &self.id.to_string(),
                ])
                .output()
                .unwrap(),
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn operation(&self, kind: &str, expired: bool) -> OperationIntent {
        let intent = OperationIntent::new(
            self.id,
            kind,
            if expired {
                Timestamp::now()
            } else {
                Timestamp::after_seconds(300)
            },
            "test operation",
            JsonDocument::parse("{}").unwrap(),
        );
        begin_operation(&mut self.connection(), &intent).unwrap();
        intent
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn refreshes_dirty_state_and_restores_ready_state() {
    let fixture = Fixture::new(false);
    let unsaved = fixture.workspace.join("alpha/unsaved");
    fs::write(&unsaved, "keep this work").unwrap();
    assert_eq!(fixture.status()["target_workspace"]["state"], "ready");
    let result = fixture.reconcile();
    assert_eq!(result["workspace_id"], fixture.id.to_string());
    assert_eq!(result["workspace_state"], "degraded");
    assert_eq!(result["changed_worktrees"], 1);
    assert_eq!(
        fixture.status()["workspaces"][0]["repo_worktrees"][0]["state"],
        "dirty"
    );
    assert_eq!(fs::read_to_string(&unsaved).unwrap(), "keep this work");
    fs::remove_file(unsaved).unwrap();
    let result = fixture.reconcile();
    assert_eq!(result["workspace_state"], "ready");
    assert_eq!(result["changed_worktrees"], 1);
    assert!(fixture.status()["target_workspace"]["last_reconciled_at"].is_string());
}

#[test]
fn checks_the_current_workspace_and_preserves_its_claim() {
    let fixture = Fixture::new(true);
    let claim = fixture.status()["target_workspace"]["claim"]["claim_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let shared = fixture.workspace.join("notes");
    fs::create_dir(&shared).unwrap();
    let output = success(
        fixture
            .command()
            .current_dir(&shared)
            .args(["reconcile", "--json"])
            .output()
            .unwrap(),
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["workspace_id"], fixture.id.to_string());
    assert_eq!(result["workspace_state"], "degraded");
    assert_eq!(result["changed_worktrees"], 0);
    assert_eq!(
        fixture.status()["target_workspace"]["claim"]["claim_id"],
        claim
    );
    fs::remove_dir(shared).unwrap();
    assert_eq!(fixture.reconcile()["workspace_state"], "ready");
    let output = success(
        fixture
            .command()
            .args(["reconcile", &fixture.id.to_string()])
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("workspace_state=ready"));
}

#[test]
fn rejects_active_operations_without_observing_workspace() {
    let fixture = Fixture::new(false);
    let intent = fixture.operation("release", false);
    let before = fixture.status();
    let output = fixture
        .command()
        .args(["reconcile", &fixture.id.to_string()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("active operation"));
    assert_eq!(
        fixture.status()["target_workspace"],
        before["target_workspace"]
    );
    assert_eq!(
        operation_state(&mut fixture.connection(), &intent.id).unwrap(),
        Some(OperationState::Running)
    );
}

#[test]
fn recovers_expired_observation_operations_and_preserves_files() {
    let fixture = Fixture::new(true);
    let intent = fixture.operation("reconcile", true);
    let claim = fixture.status()["target_workspace"]["claim"].clone();
    fs::write(fixture.workspace.join("alpha/unsaved"), "keep this work").unwrap();
    assert_eq!(fixture.reconcile()["workspace_state"], "degraded");
    assert_eq!(
        operation_state(&mut fixture.connection(), &intent.id).unwrap(),
        Some(OperationState::Failed)
    );
    assert!(find_operation_lease(&mut fixture.connection(), &intent.id)
        .unwrap()
        .is_none());
    assert_eq!(fixture.status()["target_workspace"]["claim"], claim);
    assert_eq!(
        fs::read_to_string(fixture.workspace.join("alpha/unsaved")).unwrap(),
        "keep this work"
    );
    assert!(
        trees::storage::find_running_operation(&mut fixture.connection(), &fixture.id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn finalizes_failed_checks_and_preserves_the_primary_error_chain() {
    use diesel::connection::SimpleConnection;
    use std::error::Error;
    use trees::workspace_locator::WorkspaceSelector;

    for (fail_observation, fail_finalization) in [(true, false), (true, true), (false, true)] {
        let fixture = Fixture::new(false);
        let mut connection = fixture.connection();
        fs::write(fixture.workspace.join("alpha/unsaved"), "keep this work").unwrap();
        if fail_observation {
            connection
                .batch_execute(
                    "CREATE TRIGGER reject_observation BEFORE INSERT ON lifecycle_events
                     WHEN NEW.event_type = 'external_worktree_changed'
                     BEGIN SELECT RAISE(FAIL, 'observation persistence failed'); END;",
                )
                .unwrap();
        }
        if fail_finalization {
            connection
                .batch_execute(
                    "CREATE TRIGGER reject_finalization BEFORE DELETE ON operation_leases
                 BEGIN SELECT RAISE(FAIL, 'finalization persistence failed'); END;",
                )
                .unwrap();
        }
        let error =
            trees::reconciliation::execute(&mut connection, &WorkspaceSelector::Id(fixture.id))
                .unwrap_err();
        let expected_error = if fail_observation {
            "observation persistence failed"
        } else {
            "finalization persistence failed"
        };
        assert!(error.to_string().contains(expected_error));
        assert!(error.source().unwrap().to_string().contains(expected_error));
        let running = trees::storage::find_running_operation(&mut connection, &fixture.id).unwrap();
        assert_eq!(running.is_some(), fail_finalization);
        if fail_finalization {
            assert!(error
                .to_string()
                .contains("finalization persistence failed"));
        } else {
            let operation = trees::schema::operations::table
                .filter(trees::schema::operations::kind.eq("reconcile"))
                .select(trees::schema::operations::id)
                .first::<trees::domain::OperationId>(&mut connection)
                .unwrap();
            assert_eq!(
                operation_state(&mut connection, &operation).unwrap(),
                Some(OperationState::Failed)
            );
        }
        assert_eq!(
            fs::read_to_string(fixture.workspace.join("alpha/unsaved")).unwrap(),
            "keep this work"
        );
    }
}

#[test]
fn rejects_a_changed_current_workspace_after_recovery() {
    use diesel::connection::SimpleConnection;

    let fixture = Fixture::new(false);
    let nested = fixture.workspace.join("alpha/nested");
    fs::create_dir(&nested).unwrap();
    let nested_id = WorkspaceId::new();
    let intent = fixture.operation("reconcile", true);
    let mut connection = fixture.connection();
    connection
        .batch_execute(&format!(
            "CREATE TRIGGER register_nested_workspace AFTER DELETE ON operation_leases
         WHEN OLD.operation_id = '{}'
         BEGIN
           INSERT INTO workspaces (id, canonical_path, state, created_at, updated_at)
           VALUES ('{}', '{}', 'ready', '{}', '{}');
         END;",
            intent.id,
            nested_id,
            fs::canonicalize(&nested).unwrap().display(),
            Timestamp::now(),
            Timestamp::now(),
        ))
        .unwrap();
    let output = fixture
        .command()
        .current_dir(&nested)
        .arg("reconcile")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("workspace selection changed"));
    assert_eq!(
        operation_state(&mut connection, &intent.id).unwrap(),
        Some(OperationState::Failed)
    );
    assert!(
        trees::storage::find_running_operation(&mut connection, &fixture.id)
            .unwrap()
            .is_none()
    );
    assert!(
        trees::storage::find_running_operation(&mut connection, &nested_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn rejects_expired_structural_operations_missing_targets_and_removed_workspaces() {
    let fixture = Fixture::new(false);
    let intent = fixture.operation("create", true);
    let output = fixture
        .command()
        .args(["reconcile", &fixture.id.to_string()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("lifecycle command"));
    assert_eq!(
        operation_state(&mut fixture.connection(), &intent.id).unwrap(),
        Some(OperationState::Running)
    );
    for args in [
        vec!["reconcile".to_owned()],
        vec!["reconcile".to_owned(), WorkspaceId::new().to_string()],
    ] {
        let output = fixture.command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("workspace not found"));
    }
    diesel::update(trees::schema::workspaces::table.find(fixture.id))
        .set(trees::schema::workspaces::state.eq(WorkspaceState::Removed))
        .execute(&mut fixture.connection())
        .unwrap();
    let output = fixture
        .command()
        .args(["reconcile", &fixture.id.to_string()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("has been removed"));
}
