#[cfg(unix)]
mod unix {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use trees::database;
    use trees::domain::{
        CanonicalPath, ClaimId, JsonDocument, PoolId, Timestamp, WorkspaceId,
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
                reclaimed_at: (state == WorkspaceState::Reclaimed).then_some(now),
            },
        )
        .expect("workspace should be inserted");
        (id, path)
    }

    #[test]
    fn opens_manual_and_claimed_automatic_workspaces_by_id() {
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

        fs::remove_dir_all(root).expect("test root should be removable");
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
        let (unclaimed_id, _) = insert_workspace(
            &mut connection,
            &root,
            "unclaimed",
            WorkspaceState::Ready,
            WorkspaceManagementMode::Automatic,
            Some(pool_id),
        );
        let (reclaimed_id, _) = insert_workspace(
            &mut connection,
            &root,
            "reclaimed",
            WorkspaceState::Reclaimed,
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
                "test",
                Timestamp::after_seconds(300),
                "test",
                JsonDocument::parse("{}").unwrap(),
            ),
        )
        .expect("operation should be inserted");
        drop(connection);

        for (workspace_id, message) in [
            (unclaimed_id, "automatic workspace is unclaimed"),
            (reclaimed_id, "workspace has been reclaimed"),
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

        fs::remove_dir_all(root).expect("test root should be removable");
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
