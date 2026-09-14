use super::trees_library as trees;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use trees::add::AddRequest;
use trees::domain::{CanonicalPath, WorkspaceId};
use trees::storage;
use trees::workspace_locator::WorkspaceSelector;

pub(super) fn git(path: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) db: diesel::SqliteConnection,
    pub(super) workspace: WorkspaceId,
    pub(super) path: CanonicalPath,
    pub(super) api: PathBuf,
    pub(super) web: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!("trees-add-{}", WorkspaceId::new()));
        fs::create_dir(&root).unwrap();
        let api = Self::repository(&root, "api");
        let web = Self::repository(&root, "web");
        let mut db = trees::database::connect(&root.join("db.sqlite")).unwrap();
        let result = trees::workspace::create_with_connection(
            &mut db,
            trees::workspace::prepare_create(&trees::workspace::CreateRequest {
                workspace_path: root.join("workspace"),
                repositories: vec![api.clone()],
                offline: true,
            })
            .unwrap(),
        )
        .unwrap();
        let workspace = storage::find_workspace_by_path(&mut db, &result.workspace_path)
            .unwrap()
            .unwrap()
            .id;
        Self {
            root,
            db,
            workspace,
            path: result.workspace_path,
            api,
            web,
        }
    }

    pub(super) fn repository(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q"]);
        git(&path, &["config", "user.email", "trees@example.invalid"]);
        git(&path, &["config", "user.name", "trees tests"]);
        fs::write(path.join("README"), "initial\n").unwrap();
        git(&path, &["add", "README"]);
        git(&path, &["commit", "-qm", "initial"]);
        path
    }

    pub(super) fn request(&self, repositories: Vec<PathBuf>) -> AddRequest {
        AddRequest {
            selector: WorkspaceSelector::Id(self.workspace),
            repositories,
            offline: true,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    pub(super) fn automatic() -> Self {
        let mut fixture = Self::new();
        let mut plan =
            trees::workspace::prepare_automatic(&trees::workspace::AutomaticCreateRequest {
                repositories: vec![fixture.api.clone()],
                offline: true,
            })
            .unwrap();
        plan.workspace_root = CanonicalPath::from_absolute(fixture.root.join("managed")).unwrap();
        let allocated =
            trees::workspace::allocate_automatic_workspace(&mut fixture.db, &plan).unwrap();
        fixture.path = allocated.workspace_path;
        fixture.workspace = storage::find_workspace_by_path(&mut fixture.db, &fixture.path)
            .unwrap()
            .unwrap()
            .id;
        fixture
    }
}

pub(super) fn install_event_failure(db: &mut diesel::SqliteConnection, name: &str, event: &str) {
    use diesel::connection::SimpleConnection;
    db.batch_execute(&format!("CREATE TRIGGER {name} BEFORE INSERT ON lifecycle_events WHEN NEW.event_type = '{event}' BEGIN SELECT RAISE(ABORT, 'injected event failure'); END;")).unwrap();
}

pub(super) fn clear_event_failure(db: &mut diesel::SqliteConnection, name: &str) {
    use diesel::connection::SimpleConnection;
    db.batch_execute(&format!("DROP TRIGGER {name};")).unwrap();
}
