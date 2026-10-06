#[cfg(unix)]
mod unix {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use trees::database;
    use trees::domain::{
        CanonicalPath, ClaimId, JsonDocument, OperationState, PoolId, Timestamp, WorkspaceId,
        WorkspaceManagementMode, WorkspaceState,
    };
    use trees::storage::{
        insert_managed_workspace, insert_workspace_claim, insert_workspace_pool,
        persist_operation_intent, NewManagedWorkspace, NewWorkspaceClaim, NewWorkspacePool,
        OperationIntent,
    };

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-open-cli-{}", WorkspaceId::new()))
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

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn database_path(root: &Path) -> PathBuf {
        root.join("home")
            .join(".local")
            .join("state")
            .join("trees")
            .join("db.sqlite")
    }

    fn insert_workspace(
        connection: &mut diesel::sqlite::SqliteConnection,
        root: &Path,
        name: &str,
        state: WorkspaceState,
        mode: WorkspaceManagementMode,
        pool_id: Option<PoolId>,
    ) -> (WorkspaceId, CanonicalPath) {
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("workspace directory should be created");
        let path = CanonicalPath::resolve(&directory).expect("workspace path should resolve");
        let id = WorkspaceId::new();
        let now = Timestamp::now();
        insert_managed_workspace(
            connection,
            &NewManagedWorkspace {
                id,
                canonical_path: path.clone(),
                state,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_reconciled_at: Some(now.clone()),
                management_mode: mode,
                pool_id,
                last_released_at: None,
                removed_at: (state == WorkspaceState::Removed).then_some(now),
            },
        )
        .expect("workspace should be inserted");
        (id, path)
    }

    #[test]
    fn opens_manual_and_automatic_workspaces_by_id_regardless_of_claim() {
        let root = test_root();
        let database_path = database_path(&root);
        fs::create_dir_all(database_path.parent().unwrap()).expect("state directory should exist");
        let mut connection = database::connect(&database_path).expect("database should open");
        let (manual_id, manual_path) = insert_workspace(
            &mut connection,
            &root,
            "manual",
            WorkspaceState::Ready,
            WorkspaceManagementMode::Manual,
            None,
        );
        let pool_id = PoolId::new();
        insert_workspace_pool(
            &mut connection,
            &NewWorkspacePool {
                id: pool_id,
                hash_key: "open-pool".to_owned(),
                repository_ids: "[]".to_owned(),
            },
        )
        .expect("pool should be inserted");
        let (automatic_id, automatic_path) = insert_workspace(
            &mut connection,
            &root,
            "automatic",
            WorkspaceState::Ready,
            WorkspaceManagementMode::Automatic,
            Some(pool_id),
        );
        let (unclaimed_id, unclaimed_path) = insert_workspace(
            &mut connection,
            &root,
            "unclaimed",
            WorkspaceState::Ready,
            WorkspaceManagementMode::Automatic,
            Some(pool_id),
        );
        insert_workspace_claim(
            &mut connection,
            &NewWorkspaceClaim {
                id: ClaimId::new(),
                workspace_id: automatic_id,
                claimed_at: Timestamp::now(),
            },
        )
        .expect("claim should be inserted");
        drop(connection);

        let manual = command(&root)
            .args(["open", &manual_id.to_string(), "--program=pwd"])
            .output()
            .expect("manual workspace should open");
        assert!(manual.status.success());
        assert_eq!(trim(&manual.stdout), manual_path.to_string());

        let automatic = command(&root)
            .args(["open", &automatic_id.to_string()])
            .env("SHELL", "pwd")
            .output()
            .expect("automatic workspace should open");
        assert!(automatic.status.success());
        assert_eq!(trim(&automatic.stdout), automatic_path.to_string());

        let unclaimed = command(&root)
            .args(["open", &unclaimed_id.to_string(), "--program=pwd"])
            .output()
            .expect("unclaimed workspace should open");
        assert!(unclaimed.status.success());
        assert_eq!(trim(&unclaimed.stdout), unclaimed_path.to_string());

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn opens_origins_and_rejects_ambiguous_ids() {
        let root = test_root();
        let database_path = database_path(&root);
        fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut connection = database::connect(&database_path).unwrap();
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        let source = CanonicalPath::resolve(&source).unwrap();
        let identity = CanonicalPath::from_absolute(source.as_path().join(".git")).unwrap();
        let origin =
            trees::storage::ensure_origin_repository(&mut connection, &identity, &source).unwrap();

        for explicit_program in [false, true] {
            let mut cmd = command(&root);
            cmd.args(["open", &origin.id.to_string()])
                .env("SHELL", "pwd");
            if explicit_program {
                cmd.arg("--program=pwd");
            }
            let output = cmd.output().unwrap();
            assert!(output.status.success(), "{}", trim(&output.stderr));
            assert_eq!(trim(&output.stdout), source.to_string());
        }
        let output = command(&root)
            .args([
                "open",
                "--workspace-id",
                &origin.id.to_string(),
                "--program=pwd",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(trim(&output.stderr).contains("workspace not found"));

        let (workspace_id, workspace_path) = insert_workspace(
            &mut connection,
            &root,
            "workspace",
            WorkspaceState::Ready,
            WorkspaceManagementMode::Manual,
            None,
        );
        trees::storage::insert_origin_repository(
            &mut connection,
            &trees::storage::NewOriginRepository {
                id: workspace_id.to_string().parse().unwrap(),
                repository_identity: workspace_path.clone(),
                source_path: source.clone(),
            },
        )
        .unwrap();
        let output = command(&root)
            .args(["open", &workspace_id.to_string(), "--program=pwd"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(trim(&output.stderr).contains("ID matches both"));
        let output = command(&root)
            .args([
                "open",
                "--workspace-id",
                &workspace_id.to_string(),
                "--program=pwd",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(trim(&output.stdout), workspace_path.to_string());

        fs::remove_dir_all(source.as_path()).unwrap();
        let output = command(&root)
            .args(["open", &origin.id.to_string(), "--program=pwd"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(trees::storage::origin::find(&mut connection, origin.id)
            .unwrap()
            .is_some());
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_ineligible_workspace_ids_without_mutation() {
        let root = test_root();
        let database_path = database_path(&root);
        fs::create_dir_all(database_path.parent().unwrap()).expect("state directory should exist");
        let mut connection = database::connect(&database_path).expect("database should open");
        let pool_id = PoolId::new();
        insert_workspace_pool(
            &mut connection,
            &NewWorkspacePool {
                id: pool_id,
                hash_key: "rejected-open-pool".to_owned(),
                repository_ids: "[]".to_owned(),
            },
        )
        .expect("pool should be inserted");
        let (removed_id, _) = insert_workspace(
            &mut connection,
            &root,
            "removed",
            WorkspaceState::Removed,
            WorkspaceManagementMode::Automatic,
            Some(pool_id),
        );
        let (active_id, _) = insert_workspace(
            &mut connection,
            &root,
            "active",
            WorkspaceState::Ready,
            WorkspaceManagementMode::Manual,
            None,
        );
        persist_operation_intent(
            &mut connection,
            &OperationIntent::new(
                active_id,
                "release",
                Timestamp::after_seconds(300),
                "test",
                JsonDocument::parse("{}").unwrap(),
            ),
        )
        .expect("operation should be inserted");
        pending_operation(&mut connection, removed_id, "release");
        let leases = [removed_id, active_id].map(|id| {
            trees::storage::find_running_operation(&mut connection, &id)
                .unwrap()
                .unwrap()
        });
        drop(connection);

        for (workspace_id, message) in [
            (removed_id, "workspace has been removed"),
            (active_id, "workspace has an active operation"),
            (WorkspaceId::new(), "workspace not found"),
        ] {
            let output = command(&root)
                .args(["open", &workspace_id.to_string(), "--program=pwd"])
                .output()
                .expect("open should run");
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains(message));
        }

        let mut connection = database::connect(&database_path).unwrap();
        for original in leases {
            let current = trees::storage::find_running_operation(
                &mut connection,
                &original.operation.workspace_id,
            )
            .unwrap()
            .unwrap();
            assert_eq!(current.lease.id, original.lease.id);
            assert_eq!(
                current.lease.lease_expires_at,
                original.lease.lease_expires_at
            );
        }
        drop(connection);
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    fn run_git(path: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", trim(&output.stderr));
        trim(&output.stdout)
    }

    fn automatic_workspace(
        root: &Path,
    ) -> (
        diesel::sqlite::SqliteConnection,
        trees::storage::WorkspaceRow,
    ) {
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        run_git(&source, &["init", "-q"]);
        run_git(&source, &["config", "user.email", "trees@example.invalid"]);
        run_git(&source, &["config", "user.name", "trees tests"]);
        fs::write(source.join("README"), "initial\n").unwrap();
        fs::write(source.join(".gitignore"), "ignored\n").unwrap();
        run_git(&source, &["add", "."]);
        run_git(&source, &["commit", "-qm", "initial"]);
        let mut plan =
            trees::workspace::prepare_automatic(&trees::workspace::AutomaticCreateRequest {
                repositories: vec![source],
                offline: true,
            })
            .unwrap();
        plan.workspace_root = CanonicalPath::from_absolute(root.join("managed")).unwrap();
        let database_path = database_path(root);
        fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut db = database::connect(&database_path).unwrap();
        let workspace = trees::workspace::provision_automatic(&mut db, &plan).unwrap();
        let row = trees::storage::find_workspace_by_path(&mut db, &workspace.workspace_path)
            .unwrap()
            .unwrap();
        (db, row)
    }

    fn pending_operation(
        db: &mut diesel::sqlite::SqliteConnection,
        workspace_id: WorkspaceId,
        kind: &str,
    ) -> OperationIntent {
        let intent = OperationIntent::new(
            workspace_id,
            kind,
            Timestamp::parse("2020-01-01T00:00:00Z").unwrap(),
            "interrupted operation",
            JsonDocument::parse("{}").unwrap(),
        );
        persist_operation_intent(db, &intent).unwrap();
        intent
    }

    #[test]
    fn open_recovers_expired_operations_and_preserves_local_work_and_claims() {
        let root = test_root();
        let (mut db, workspace) = automatic_workspace(&root);
        let path = workspace.canonical_path.as_path();
        run_git(path, &["checkout", "-qb", "local-work"]);
        fs::write(path.join("README"), "staged\n").unwrap();
        run_git(path, &["add", "README"]);
        fs::write(path.join("README"), "unstaged\n").unwrap();
        fs::write(path.join("untracked"), "untracked\n").unwrap();
        fs::write(path.join("ignored"), "ignored\n").unwrap();
        let head = run_git(path, &["rev-parse", "HEAD"]);
        let status = run_git(path, &["status", "--porcelain", "--ignored"]);
        let claim = trees::storage::find_workspace_claim(&mut db, &workspace.id)
            .unwrap()
            .unwrap();

        for kind in ["release", "acquire", "claim", "gc", "remove"] {
            let intent = pending_operation(&mut db, workspace.id, kind);
            let output = command(&root)
                .args(["open", &workspace.id.to_string(), "--program=pwd"])
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", trim(&output.stderr));
            assert_eq!(trim(&output.stdout), workspace.canonical_path.to_string());
            assert!(
                trees::storage::find_running_operation(&mut db, &workspace.id)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                trees::storage::operation_state(&mut db, &intent.id).unwrap(),
                Some(OperationState::Failed)
            );
            assert!(
                trees::storage::list_events_for_operation(&mut db, &intent.id)
                    .unwrap()
                    .iter()
                    .any(|event| event.event_type == "operation_recovered")
            );

            assert_eq!(run_git(path, &["rev-parse", "HEAD"]), head);
            assert_eq!(run_git(path, &["branch", "--show-current"]), "local-work");
            assert_eq!(
                run_git(path, &["status", "--porcelain", "--ignored"]),
                status
            );
            for (name, contents) in [
                ("README", "unstaged\n"),
                ("untracked", "untracked\n"),
                ("ignored", "ignored\n"),
            ] {
                assert_eq!(fs::read_to_string(path.join(name)).unwrap(), contents);
            }
            let current_claim = trees::storage::find_workspace_claim(&mut db, &workspace.id)
                .unwrap()
                .unwrap();
            assert_eq!(current_claim.id, claim.id);
            assert_eq!(current_claim.claimed_at, claim.claimed_at);
        }
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    fn recovery_workspace(
        root: &Path,
    ) -> (diesel::sqlite::SqliteConnection, WorkspaceId, CanonicalPath) {
        let database_path = database_path(root);
        fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut db = database::connect(&database_path).unwrap();
        let (id, path) = insert_workspace(
            &mut db,
            root,
            "workspace",
            WorkspaceState::Ready,
            WorkspaceManagementMode::Manual,
            None,
        );
        (db, id, path)
    }

    #[test]
    fn open_preserves_expired_structural_and_unknown_operations() {
        for kind in ["create", "add", "unknown"] {
            let root = test_root();
            let (mut db, workspace_id, path) = recovery_workspace(&root);
            fs::write(path.as_path().join("sentinel"), "preserve\n").unwrap();
            let intent = pending_operation(&mut db, workspace_id, kind);
            let events = trees::storage::list_events_for_operation(&mut db, &intent.id)
                .unwrap()
                .len();
            let output = command(&root)
                .args(["open", &workspace_id.to_string(), "--program=pwd"])
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            let stderr = trim(&output.stderr);
            assert!(stderr.contains(&format!("expired {kind} operation {}", intent.id)));
            assert!(stderr.contains("recover it through its lifecycle command"));
            let running = trees::storage::find_running_operation(&mut db, &workspace_id)
                .unwrap()
                .unwrap();
            assert_eq!(running.lease.id, intent.lease_id);
            assert!(running.lease.lease_expires_at.has_expired());
            assert_eq!(
                trees::storage::list_events_for_operation(&mut db, &intent.id)
                    .unwrap()
                    .len(),
                events
            );
            assert_eq!(
                fs::read_to_string(path.as_path().join("sentinel")).unwrap(),
                "preserve\n"
            );
            drop(db);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn open_rechecks_operation_admission_after_recovery() {
        use diesel::connection::SimpleConnection;

        let root = test_root();
        let (mut db, workspace_id, _) = recovery_workspace(&root);
        let intent = pending_operation(&mut db, workspace_id, "release");
        let competing = OperationIntent::new(
            workspace_id,
            "release",
            Timestamp::after_seconds(300),
            "competing release",
            JsonDocument::parse("{}").unwrap(),
        );
        db.batch_execute(&format!(
            "CREATE TRIGGER competing_release AFTER DELETE ON operation_leases
             WHEN OLD.operation_id = '{}'
             BEGIN
               INSERT INTO operations (id, workspace_id, kind, started_at, intent_json)
               VALUES ('{}', '{}', 'release', '{}', '{{}}');
               INSERT INTO operation_leases (id, operation_id, workspace_id, lease_expires_at)
               VALUES ('{}', '{}', '{}', '{}');
             END;",
            intent.id,
            competing.id,
            workspace_id,
            Timestamp::now(),
            competing.lease_id,
            competing.id,
            workspace_id,
            competing.lease_expires_at,
        ))
        .unwrap();
        let output = command(&root)
            .args(["open", &workspace_id.to_string(), "--program=pwd"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(trim(&output.stderr).contains("workspace has an active operation"));
        assert_eq!(
            trees::storage::operation_state(&mut db, &intent.id).unwrap(),
            Some(OperationState::Failed)
        );
        assert_eq!(
            trees::storage::find_running_operation(&mut db, &workspace_id)
                .unwrap()
                .unwrap()
                .operation
                .id,
            competing.id
        );
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn open_does_not_launch_when_recovery_cannot_persist_its_terminal_event() {
        use diesel::connection::SimpleConnection;

        let root = test_root();
        let (mut db, workspace_id, _) = recovery_workspace(&root);
        let intent = pending_operation(&mut db, workspace_id, "release");
        db.batch_execute(
            "CREATE TRIGGER reject_recovery BEFORE INSERT ON lifecycle_events
             WHEN NEW.event_type = 'operation_recovered'
             BEGIN SELECT RAISE(FAIL, 'recovery persistence failed'); END;",
        )
        .unwrap();
        let output = command(&root)
            .args(["open", &workspace_id.to_string(), "--program=pwd"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(trim(&output.stderr).contains("recovery persistence failed"));
        let running = trees::storage::find_running_operation(&mut db, &workspace_id)
            .unwrap()
            .unwrap();
        assert_eq!(running.operation.id, intent.id);
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unavailable_or_empty_programs_before_database_access() {
        let root = test_root();
        let workspace_id = WorkspaceId::new();

        let missing_shell = command(&root)
            .args(["open", &workspace_id.to_string()])
            .env_remove("SHELL")
            .output()
            .expect("open should run");
        assert!(!missing_shell.status.success());
        assert!(String::from_utf8_lossy(&missing_shell.stderr)
            .contains("$SHELL is unset or empty; use --program=<PROGRAM>"));
        assert!(!database_path(&root).exists());

        let empty_program = command(&root)
            .args(["open", &workspace_id.to_string(), "--program="])
            .output()
            .expect("open should run");
        assert!(!empty_program.status.success());
        assert!(String::from_utf8_lossy(&empty_program.stderr)
            .contains("--program program must not be empty"));
        assert!(!database_path(&root).exists());

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    fn trim(output: &[u8]) -> String {
        String::from_utf8(output.to_vec())
            .expect("output should be UTF-8")
            .trim()
            .to_owned()
    }
}
