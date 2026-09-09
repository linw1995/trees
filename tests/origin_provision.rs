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
}
