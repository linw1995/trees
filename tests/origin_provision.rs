use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use trees::domain::{OriginRepositoryId, RepositoryManagementMode};

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("trees-origin-{}", OriginRepositoryId::new()));
        fs::create_dir_all(root.join("remote")).unwrap();
        git(&root.join("remote"), &["init", "-b", "main"]);
        git(
            &root.join("remote"),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        Self(root)
    }
    fn url(&self) -> String {
        format!("file://{}", self.0.join("remote").display())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn clones_and_reuses_origins_across_configuration_changes() {
    let fixture = Fixture::new();
    let mut connection = trees::database::connect(Path::new(":memory:")).unwrap();
    let locks = fixture.0.join("locks");
    let first = trees::origin::provision::provision(
        &mut connection,
        &fixture.url(),
        &fixture.0.join("origins"),
        &locks,
    )
    .unwrap();
    assert_eq!(first.management_mode, RepositoryManagementMode::Automatic);
    assert!(first.source_path.as_path().join(".git").is_dir());
    let repeated = trees::origin::provision::provision(
        &mut connection,
        &fixture.url(),
        &fixture.0.join("other"),
        &locks,
    )
    .unwrap();
    assert_eq!(first.id, repeated.id);
    assert!(!fixture.0.join("other").exists());
    fs::rename(first.source_path.as_path(), fixture.0.join("moved")).unwrap();
    assert!(trees::origin::provision::provision(
        &mut connection,
        &fixture.url(),
        &fixture.0.join("other"),
        &locks
    )
    .is_err());
}

#[test]
fn failed_clone_does_not_publish_an_origin() {
    let fixture = Fixture::new();
    let mut connection = trees::database::connect(Path::new(":memory:")).unwrap();
    let url = format!("file://{}", fixture.0.join("missing").display());
    assert!(trees::origin::provision::provision(
        &mut connection,
        &url,
        &fixture.0.join("origins"),
        &fixture.0.join("locks")
    )
    .is_err());
    assert!(trees::storage::origin::find_by_url(&mut connection, &url)
        .unwrap()
        .is_none());
    assert!(
        trees::storage::origin::pending_by_url(&mut connection, &url)
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read_dir(fixture.0.join("origins")).unwrap().count(), 0);
}

#[test]
fn recovers_interrupted_owned_clone_and_preserves_unproven_content() {
    use trees::origin::reservation::{reserve, Reservation};
    let fixture = Fixture::new();
    let mut connection = trees::database::connect(Path::new(":memory:")).unwrap();
    let root = fixture.0.join("origins");
    let locks = fixture.0.join("locks");
    let Reservation::Pending(reservation) =
        reserve(&mut connection, &fixture.url(), &root, &locks).unwrap()
    else {
        panic!("expected pending")
    };
    let container = reservation
        .pending
        .source_path
        .as_path()
        .parent()
        .unwrap()
        .to_owned();
    fs::create_dir(&container).unwrap();
    fs::write(
        container.join(trees::origin::provision::OWNER_FILE),
        &reservation.pending.ownership_token,
    )
    .unwrap();
    fs::write(container.join("partial"), "partial").unwrap();
    let id = reservation.pending.id;
    drop(reservation);
    let row = trees::origin::provision::provision(&mut connection, &fixture.url(), &root, &locks)
        .unwrap();
    assert_eq!(row.id, id);
    assert!(!container.join("partial").exists());
    assert!(
        trees::storage::origin::pending_by_url(&mut connection, &fixture.url())
            .unwrap()
            .is_none()
    );

    let other_url = format!("file://{}/other", fixture.0.display());
    let Reservation::Pending(reservation) =
        reserve(&mut connection, &other_url, &root, &locks).unwrap()
    else {
        panic!("expected pending")
    };
    let container = reservation
        .pending
        .source_path
        .as_path()
        .parent()
        .unwrap()
        .to_owned();
    fs::create_dir(&container).unwrap();
    fs::write(container.join("valuable"), "keep").unwrap();
    drop(reservation);
    assert!(
        trees::origin::provision::provision(&mut connection, &other_url, &root, &locks).is_err()
    );
    assert_eq!(
        fs::read_to_string(container.join("valuable")).unwrap(),
        "keep"
    );
    assert!(
        trees::storage::origin::pending_by_url(&mut connection, &other_url)
            .unwrap()
            .is_some()
    );
}

fn trees(fixture: &Fixture) -> Command {
    let home = fixture.0.join("home");
    fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_trees"));
    command
        .current_dir(&fixture.0)
        .env("HOME", home)
        .env("XDG_STATE_HOME", fixture.0.join("state"))
        .env("XDG_DATA_HOME", fixture.0.join("data"))
        .env("XDG_CONFIG_HOME", fixture.0.join("config"));
    command
}

fn success(output: std::process::Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn creates_from_url_then_reuses_directory_name() {
    let fixture = Fixture::new();
    success(
        trees(&fixture)
            .args(["create", "one", "--repo", &fixture.url(), "--json"])
            .output()
            .unwrap(),
    );
    let second = success(
        trees(&fixture)
            .current_dir(fixture.0.join("home"))
            .args(["create", "two", "--repo", "remote", "--offline", "--json"])
            .output()
            .unwrap(),
    );
    assert!(Path::new(second["workspace_path"].as_str().unwrap()).exists());
    let unknown = trees(&fixture)
        .args(["create", "three", "--repo", "unknown-repository"])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no registered repository"));
}

#[test]
fn repeated_url_reuses_pool_and_offline_requires_a_known_clone() {
    let fixture = Fixture::new();
    let unknown = trees(&fixture)
        .args(["create", "--repo", &fixture.url(), "--offline", "--json"])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("offline creation cannot clone"));
    let first = success(
        trees(&fixture)
            .args(["create", "--repo", &fixture.url(), "--json"])
            .output()
            .unwrap(),
    );
    let release = trees(&fixture)
        .args(["release", "--claim-id", first["claim_id"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        release.status.success(),
        "{}",
        String::from_utf8_lossy(&release.stderr)
    );
    let second = success(
        trees(&fixture)
            .args(["create", "--repo", &fixture.url(), "--offline", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(first["pool_id"], second["pool_id"]);
    assert_eq!(first["workspace_path"], second["workspace_path"]);
    assert_ne!(first["claim_id"], second["claim_id"]);
}

#[test]
fn mixed_manual_and_automatic_origins_support_both_workspace_modes() {
    let fixture = Fixture::new();
    let local = fixture.0.join("web");
    fs::create_dir(&local).unwrap();
    git(&local, &["init", "-b", "main"]);
    git(
        &local,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    let local = local.to_str().unwrap();
    let manual = success(
        trees(&fixture)
            .args([
                "create",
                "manual",
                "--repo",
                local,
                "--repo",
                &fixture.url(),
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let automatic = success(
        trees(&fixture)
            .args([
                "create",
                "--repo",
                local,
                "--repo",
                &fixture.url(),
                "--offline",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    for result in [manual, automatic] {
        let root = Path::new(result["workspace_path"].as_str().unwrap());
        assert!(root.join("web/.git").is_file());
        assert!(root.join("remote/.git").is_file());
    }
}

#[test]
fn preflight_prevents_clone_and_later_failures_preserve_published_sources() {
    let fixture = Fixture::new();
    let root = fixture.0.join("sources");
    let configured = trees(&fixture)
        .args(["config", "set", "origins-dir", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(configured.status.success());
    let invalid = trees(&fixture)
        .args([
            "create",
            "one",
            "--repo",
            &fixture.url(),
            "--repo",
            "./missing-local",
        ])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(!root.exists());
    let existing = trees(&fixture)
        .args(["create", "remote", "--repo", &fixture.url()])
        .output()
        .unwrap();
    assert!(!existing.status.success());
    assert!(!root.exists());
    let missing_url = format!("file://{}", fixture.0.join("missing-remote").display());
    let partial = trees(&fixture)
        .args([
            "create",
            "one",
            "--repo",
            &fixture.url(),
            "--repo",
            &missing_url,
        ])
        .output()
        .unwrap();
    assert!(!partial.status.success());
    assert!(String::from_utf8_lossy(&partial.stderr).contains("Origin available for reuse:"));
    assert!(!fixture.0.join("one").exists());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    success(
        trees(&fixture)
            .args([
                "create",
                "one",
                "--repo",
                &fixture.url(),
                "--offline",
                "--json",
            ])
            .output()
            .unwrap(),
    );
}

#[test]
fn repos_json_is_versioned_and_reports_missing_sources_without_mutation() {
    let fixture = Fixture::new();
    let empty = success(
        trees(&fixture)
            .args(["status", "--view", "repos", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(empty["schema_version"], 1);
    assert_eq!(empty["view"], "repos");
    assert_eq!(empty["repos"], serde_json::json!([]));
    assert!(!fixture.0.join("state").exists());
    success(
        trees(&fixture)
            .args(["create", "one", "--repo", &fixture.url(), "--json"])
            .output()
            .unwrap(),
    );
    let before = success(
        trees(&fixture)
            .args(["status", "--view", "repos", "--json"])
            .output()
            .unwrap(),
    );
    let repo = &before["repos"][0];
    assert_eq!(repo["management_mode"], "automatic");
    assert_eq!(repo["remote_url"], fixture.url());
    assert_eq!(repo["label"], "remote");
    assert_eq!(repo["registered"], true);
    assert!(repo["origin_repository_id"]
        .as_str()
        .unwrap()
        .parse::<OriginRepositoryId>()
        .is_ok());
    assert!(before["snapshot_at"].is_string());
    let source = Path::new(repo["source_path"].as_str().unwrap());
    assert!(source.starts_with(repo["managed_root"].as_str().unwrap()));
    fs::rename(source, fixture.0.join("moved-source")).unwrap();
    let after = success(
        trees(&fixture)
            .args(["status", "--view", "repos", "--all", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(before["repos"], after["repos"]);
    assert!(!source.exists());
}

#[test]
fn unregister_preserves_sources_claims_and_identity_for_both_modes() {
    for automatic in [false, true] {
        let fixture = Fixture::new();
        let input = if automatic {
            fixture.url()
        } else {
            fixture.0.join("remote").to_string_lossy().into_owned()
        };
        let created = success(
            trees(&fixture)
                .args(["create", "--repo", &input, "--json"])
                .output()
                .unwrap(),
        );
        let status = success(
            trees(&fixture)
                .args(["status", "--view", "repos", "--json"])
                .output()
                .unwrap(),
        );
        let row = &status["repos"][0];
        let id = row["origin_repository_id"].as_str().unwrap();
        let preview = trees(&fixture)
            .args(["remove", id, "--dry-run"])
            .output()
            .unwrap();
        assert!(preview.status.success());
        assert!(String::from_utf8_lossy(&preview.stdout).contains("action=unregister_repository"));
        assert_eq!(
            success(
                trees(&fixture)
                    .args(["status", "--view", "repos", "--json"])
                    .output()
                    .unwrap()
            )["repos"],
            status["repos"]
        );
        let removed = trees(&fixture)
            .args(["remove", id, "--force"])
            .output()
            .unwrap();
        assert!(
            removed.status.success(),
            "{}",
            String::from_utf8_lossy(&removed.stderr)
        );
        assert!(Path::new(row["source_path"].as_str().unwrap()).exists());
        assert!(success(
            trees(&fixture)
                .args(["status", "--view", "repos", "--json"])
                .output()
                .unwrap()
        )["repos"]
            .as_array()
            .unwrap()
            .is_empty());
        let all = success(
            trees(&fixture)
                .args(["status", "--view", "repos", "--all", "--json"])
                .output()
                .unwrap(),
        );
        assert_eq!(all["repos"][0]["registered"], false);
        assert!(trees(&fixture)
            .args(["remove", id, "--yes"])
            .output()
            .unwrap()
            .status
            .success());
        let release = trees(&fixture)
            .args([
                "release",
                "--claim-id",
                created["claim_id"].as_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            release.status.success(),
            "{}",
            String::from_utf8_lossy(&release.stderr)
        );
        let reused = success(
            trees(&fixture)
                .args(["create", "--repo", &input, "--offline", "--json"])
                .output()
                .unwrap(),
        );
        assert_eq!(created["pool_id"], reused["pool_id"]);
        let current = success(
            trees(&fixture)
                .args(["status", "--view", "repos", "--json"])
                .output()
                .unwrap(),
        );
        assert_eq!(current["repos"][0]["origin_repository_id"], id);
    }
}

#[test]
fn publication_failure_rolls_back_and_cleans_only_its_clone() {
    use diesel::connection::SimpleConnection;
    let fixture = Fixture::new();
    let mut connection = trees::database::connect(Path::new(":memory:")).unwrap();
    connection.batch_execute("CREATE TRIGGER reject_origin BEFORE INSERT ON origin_repositories BEGIN SELECT RAISE(ABORT, 'injected publication failure'); END;").unwrap();
    let root = fixture.0.join("origins");
    let error = trees::origin::provision::provision(
        &mut connection,
        &fixture.url(),
        &root,
        &fixture.0.join("locks"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("injected publication failure"));
    assert!(trees::storage::origin::list(&mut connection, true)
        .unwrap()
        .is_empty());
    assert!(
        trees::storage::origin::pending_by_url(&mut connection, &fixture.url())
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    assert!(fixture.0.join("remote/.git").exists());
}

#[test]
fn cleanup_never_removes_a_published_origin() {
    use trees::domain::CanonicalPath;
    use trees::origin::reservation::{reserve, Reservation};
    let fixture = Fixture::new();
    let mut connection = trees::database::connect(Path::new(":memory:")).unwrap();
    let Reservation::Pending(reservation) = reserve(
        &mut connection,
        &fixture.url(),
        &fixture.0.join("origins"),
        &fixture.0.join("locks"),
    )
    .unwrap() else {
        panic!("expected pending")
    };
    let path = reservation.pending.source_path.as_path();
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("valuable"), "committed").unwrap();
    trees::storage::origin::publish_clone(
        &mut connection,
        &reservation.pending,
        &CanonicalPath::from_absolute(path.join(".git")).unwrap(),
    )
    .unwrap();
    trees::origin::recovery::clean_partial(&mut connection, &reservation).unwrap();
    assert_eq!(
        fs::read_to_string(path.join("valuable")).unwrap(),
        "committed"
    );
}

#[test]
fn unusable_head_is_not_published_and_its_clone_is_cleaned() {
    let fixture = Fixture::new();
    let empty = fixture.0.join("empty");
    fs::create_dir(&empty).unwrap();
    git(&empty, &["init", "-b", "main"]);
    let mut connection = trees::database::connect(Path::new(":memory:")).unwrap();
    let url = format!("file://{}", empty.display());
    let root = fixture.0.join("origins");
    assert!(trees::origin::provision::provision(
        &mut connection,
        &url,
        &root,
        &fixture.0.join("locks")
    )
    .is_err());
    assert!(trees::storage::origin::list(&mut connection, true)
        .unwrap()
        .is_empty());
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn manual_paths_inside_the_managed_root_remain_manual_and_names_can_be_ambiguous() {
    let fixture = Fixture::new();
    let configured = trees(&fixture)
        .args(["config", "set", "origins-dir", fixture.0.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(configured.status.success());
    success(
        trees(&fixture)
            .args(["create", "one", "--repo", "./remote", "--offline", "--json"])
            .output()
            .unwrap(),
    );
    let second = fixture.0.join("other/remote");
    fs::create_dir_all(&second).unwrap();
    git(&second, &["init", "-b", "main"]);
    git(
        &second,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    success(
        trees(&fixture)
            .args([
                "create",
                "two",
                "--repo",
                second.to_str().unwrap(),
                "--offline",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    let snapshot = success(
        trees(&fixture)
            .args(["status", "--view", "repos", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(snapshot["repos"].as_array().unwrap().len(), 2);
    for row in snapshot["repos"].as_array().unwrap() {
        assert_eq!(row["management_mode"], "manual");
        assert!(row["managed_root"].is_null());
    }
    let ambiguous = trees(&fixture)
        .current_dir(fixture.0.join("home"))
        .args(["create", "three", "--repo", "remote", "--offline"])
        .output()
        .unwrap();
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("ambiguous"));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn old_schema_status_fails_without_migrating_or_emitting_partial_json() {
    use diesel_migrations::MigrationHarness;
    let fixture = Fixture::new();
    success(
        trees(&fixture)
            .args(["create", "one", "--repo", "./remote", "--offline", "--json"])
            .output()
            .unwrap(),
    );
    #[cfg(target_os = "macos")]
    let path = fixture
        .0
        .join("home/Library/Application Support/trees/db.sqlite");
    #[cfg(target_os = "linux")]
    let path = fixture.0.join("state/trees/db.sqlite");
    let mut connection = trees::database::connect(&path).unwrap();
    connection
        .revert_last_migration(trees::database::MIGRATIONS)
        .unwrap();
    drop(connection);
    let before = fs::read(&path).unwrap();
    let status = trees(&fixture)
        .args(["status", "--view", "repos", "--json"])
        .output()
        .unwrap();
    assert!(!status.status.success());
    assert!(status.stdout.is_empty());
    assert!(String::from_utf8_lossy(&status.stderr).contains("upgrade"));
    assert_eq!(before, fs::read(path).unwrap());
}

#[test]
fn rejects_duplicate_inputs_and_changed_identity_before_workspace_mutation() {
    let fixture = Fixture::new();
    success(
        trees(&fixture)
            .args(["create", "one", "--repo", &fixture.url(), "--json"])
            .output()
            .unwrap(),
    );
    let snapshot = success(
        trees(&fixture)
            .args(["status", "--view", "repos", "--json"])
            .output()
            .unwrap(),
    );
    let path = snapshot["repos"][0]["source_path"].as_str().unwrap();
    for duplicate in [&fixture.url(), path] {
        let output = trees(&fixture)
            .args([
                "create",
                "duplicate",
                "--repo",
                &fixture.url(),
                "--repo",
                duplicate,
                "--offline",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!fixture.0.join("duplicate").exists());
    }
    let source = trees::domain::CanonicalPath::resolve(path).unwrap();
    let mut expected = trees::git::inspect_repository(&source).unwrap();
    expected.common_dir =
        trees::domain::CanonicalPath::from_absolute(fixture.0.join("different/.git")).unwrap();
    let result = trees::workspace::prepare_create_resolved(
        &trees::workspace::CreateRequest {
            workspace_path: fixture.0.join("changed"),
            repositories: vec![source.as_path().to_owned()],
            offline: false,
        },
        &[expected],
    );
    assert!(matches!(
        result,
        Err(trees::workspace::WorkspaceError::SourceChanged { .. })
    ));
    assert!(!fixture.0.join("changed").exists());
}
