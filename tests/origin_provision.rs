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
