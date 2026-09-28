use diesel::prelude::*;
use serde::Serialize;
use snafu::ResultExt;

use super::combined::{self, Snapshot, SnapshotError};
use super::disk_usage;
use super::processes::{self, Boundary, Observation};
use super::{session_hook, StatusView, WorkspaceStatus};
use crate::domain::WorkspaceId;
use crate::storage::repository;
use crate::workspace_locator::WorkspaceSelector;

#[derive(Debug, Serialize)]
pub struct Report {
    #[serde(flatten)]
    pub snapshot: Snapshot,
    pub target_processes: Option<Observation>,
    pub target_disk_usage: Option<disk_usage::Observation>,
    pub workspace_disk_usage: Vec<WorkspaceDiskUsage>,
    pub workspace_sessions: Option<session_hook::Observation>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceDiskUsage {
    pub workspace_id: WorkspaceId,
    #[serde(flatten)]
    pub observation: disk_usage::Observation,
}

pub fn load(
    connection: Option<SqliteConnection>,
    selector: &WorkspaceSelector,
    view: StatusView,
    include_removed: bool,
    no_hooks: bool,
) -> Result<Report, SnapshotError> {
    load_with_all_observers(
        connection,
        selector,
        view,
        include_removed,
        |target, boundaries| processes::observe(target.workspace_id, boundaries),
        |workspaces| session_hook::observe(workspaces, !no_hooks),
        |workspace| disk_usage::observe(workspace.path.as_path()),
    )
}

#[cfg(test)]
fn load_with_observers(
    connection: Option<SqliteConnection>,
    selector: &WorkspaceSelector,
    view: StatusView,
    include_removed: bool,
    observer: impl FnOnce(&WorkspaceStatus, &[Boundary]) -> Observation,
    sessions: impl FnOnce(&[WorkspaceStatus]) -> Option<session_hook::Observation>,
) -> Result<Report, SnapshotError> {
    load_with_all_observers(
        connection,
        selector,
        view,
        include_removed,
        observer,
        sessions,
        |workspace| disk_usage::observe(workspace.path.as_path()),
    )
}

fn load_with_all_observers(
    mut connection: Option<SqliteConnection>,
    selector: &WorkspaceSelector,
    view: StatusView,
    include_removed: bool,
    observer: impl FnOnce(&WorkspaceStatus, &[Boundary]) -> Observation,
    sessions: impl FnOnce(&[WorkspaceStatus]) -> Option<session_hook::Observation>,
    mut disk_observer: impl FnMut(&WorkspaceStatus) -> disk_usage::Observation,
) -> Result<Report, SnapshotError> {
    let (snapshot, boundaries) =
        load_persisted(connection.as_mut(), selector, view, include_removed, || {})?;
    drop(connection);
    let workspace_sessions = match &snapshot {
        Snapshot::Workspaces(snapshot) if !snapshot.workspaces.is_empty() => {
            sessions(&snapshot.workspaces)
        }
        _ => None,
    };
    let target_processes = snapshot
        .target()
        .map(|target| observer(target, &boundaries));
    let target_disk_usage = snapshot.target().map(&mut disk_observer);
    let workspace_disk_usage = match &snapshot {
        Snapshot::Workspaces(snapshot) => snapshot
            .workspaces
            .iter()
            .map(|workspace| WorkspaceDiskUsage {
                workspace_id: workspace.workspace_id,
                observation: if snapshot
                    .target_workspace
                    .as_ref()
                    .is_some_and(|target| target.workspace_id == workspace.workspace_id)
                {
                    target_disk_usage
                        .as_ref()
                        .expect("target observation exists")
                        .clone()
                } else {
                    disk_observer(workspace)
                },
            })
            .collect(),
        _ => Vec::new(),
    };
    Ok(Report {
        snapshot,
        target_processes,
        target_disk_usage,
        workspace_disk_usage,
        workspace_sessions,
    })
}

#[cfg(test)]
fn load_with_observer(
    connection: Option<SqliteConnection>,
    selector: &WorkspaceSelector,
    view: StatusView,
    include_removed: bool,
    observer: impl FnOnce(&WorkspaceStatus, &[Boundary]) -> Observation,
) -> Result<Report, SnapshotError> {
    load_with_observers(
        connection,
        selector,
        view,
        include_removed,
        observer,
        |_| None,
    )
}

fn load_persisted(
    connection: Option<&mut SqliteConnection>,
    selector: &WorkspaceSelector,
    view: StatusView,
    include_removed: bool,
    after_snapshot: impl FnOnce(),
) -> Result<(Snapshot, Vec<Boundary>), SnapshotError> {
    match connection {
        Some(connection) => connection.transaction::<_, SnapshotError, _>(|connection| {
            // The existing loader uses a nested transaction (savepoint); this outer
            // transaction keeps its snapshot alive until the boundary query finishes.
            let snapshot = combined::load(Some(connection), selector, view, include_removed)?;
            after_snapshot();
            let boundaries = if snapshot.target().is_some() {
                repository::list_workspace_boundaries(connection)
                    .context(super::combined::InventorySnafu {
                        view: "workspace boundary",
                    })?
                    .into_iter()
                    .map(|(workspace_id, path)| Boundary { workspace_id, path })
                    .collect()
            } else {
                Vec::new()
            };
            Ok((snapshot, boundaries))
        }),
        None => Ok((
            combined::load(None, selector, view, include_removed)?,
            Vec::new(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::database;
    use crate::domain::*;
    use crate::status::processes::{Completeness, IssueCode};
    use crate::status::tests::insert_workspace;

    fn disk_observation(bytes: u64) -> disk_usage::Observation {
        disk_usage::Observation {
            observed_at: Timestamp::parse("2000-01-01T00:00:00Z").unwrap(),
            status: disk_usage::Completeness::Complete,
            allocated_bytes: Some(bytes),
            issues: Vec::new(),
        }
    }

    #[test]
    fn disk_usage_covers_rows_and_reuses_the_target_observation() {
        for view in [StatusView::Pools, StatusView::Workspaces, StatusView::Repos] {
            let mut connection = database::connect(std::path::Path::new(":memory:")).unwrap();
            let target = insert_workspace(&mut connection, "/work/api", WorkspaceState::Ready);
            let other = insert_workspace(&mut connection, "/work/web", WorkspaceState::Ready);
            let calls = RefCell::new(Vec::new());
            let report = load_with_all_observers(
                Some(connection),
                &WorkspaceSelector::Id(target),
                view,
                false,
                |_, _| Observation::unavailable(Timestamp::now(), IssueCode::EnumerationFailed),
                |_| None,
                |workspace| {
                    calls.borrow_mut().push(workspace.workspace_id);
                    disk_observation(4096)
                },
            )
            .unwrap();
            let calls = calls.into_inner();
            assert_eq!(calls.iter().filter(|&&id| id == target).count(), 1);
            let json = serde_json::to_value(report).unwrap();
            assert_eq!(json["target_disk_usage"]["allocated_bytes"], 4096);
            assert_eq!(json["target_workspace"]["workspace_id"], target.to_string());
            if view == StatusView::Workspaces {
                assert_eq!(calls.len(), 2);
                assert!(calls.contains(&other));
                assert_eq!(json["workspace_disk_usage"].as_array().unwrap().len(), 2);
                for (workspace, usage) in json["workspaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(json["workspace_disk_usage"].as_array().unwrap())
                {
                    assert_eq!(usage["workspace_id"], workspace["workspace_id"]);
                    if workspace["workspace_id"] == target.to_string() {
                        assert_eq!(
                            usage["observed_at"],
                            json["target_disk_usage"]["observed_at"]
                        );
                    }
                }
            } else {
                assert_eq!(calls, vec![target]);
                assert_eq!(json["workspace_disk_usage"], serde_json::json!([]));
            }
        }
    }

    #[test]
    fn disk_usage_scans_workspace_rows_without_a_target() {
        let mut connection = database::connect(std::path::Path::new(":memory:")).unwrap();
        let first = insert_workspace(&mut connection, "/work/api", WorkspaceState::Ready);
        let second = insert_workspace(&mut connection, "/work/web", WorkspaceState::Ready);
        let calls = RefCell::new(Vec::new());
        let selector = WorkspaceSelector::ContainingDirectory(
            CanonicalPath::from_absolute("/outside").unwrap(),
        );
        let report = load_with_all_observers(
            Some(connection),
            &selector,
            StatusView::Workspaces,
            false,
            |_, _| panic!("no target must not trigger process observation"),
            |_| None,
            |workspace| {
                calls.borrow_mut().push(workspace.workspace_id);
                disk_observation(1024)
            },
        )
        .unwrap();
        let json = serde_json::to_value(report).unwrap();
        assert!(json["target_disk_usage"].is_null());
        assert_eq!(json["workspace_disk_usage"].as_array().unwrap().len(), 2);
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 2);
        assert!(calls.contains(&first));
        assert!(calls.contains(&second));
    }

    #[test]
    fn disk_usage_observes_removed_target_outside_the_inventory() {
        let mut connection = database::connect(std::path::Path::new(":memory:")).unwrap();
        let active = insert_workspace(&mut connection, "/work/api", WorkspaceState::Ready);
        let removed = insert_workspace(&mut connection, "/work/old", WorkspaceState::Removed);
        let calls = RefCell::new(Vec::new());
        let report = load_with_all_observers(
            Some(connection),
            &WorkspaceSelector::Id(removed),
            StatusView::Workspaces,
            false,
            |_, _| Observation::unavailable(Timestamp::now(), IssueCode::EnumerationFailed),
            |_| None,
            |workspace| {
                calls.borrow_mut().push(workspace.workspace_id);
                disk_usage::Observation {
                    observed_at: Timestamp::now(),
                    status: disk_usage::Completeness::Unavailable,
                    allocated_bytes: None,
                    issues: vec![disk_usage::Issue {
                        code: disk_usage::IssueCode::RootMissing,
                        affected_count: None,
                    }],
                }
            },
        )
        .unwrap();
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 2);
        assert!(calls.contains(&active));
        assert!(calls.contains(&removed));
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["target_disk_usage"]["status"], "unavailable");
        assert_eq!(
            json["workspace_disk_usage"][0]["workspace_id"],
            active.to_string()
        );
    }

    #[test]
    fn disk_usage_runs_after_storage_closes_and_skips_failed_loads() {
        let root = std::env::temp_dir().join(format!("trees-disk-report-{}", WorkspaceId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.sqlite");
        let mut writer = database::connect(&path).unwrap();
        let target = insert_workspace(&mut writer, "/work/api", WorkspaceState::Ready);
        drop(writer);
        let report = load_with_all_observers(
            Some(database::connect_read_only(&path).unwrap()),
            &WorkspaceSelector::Id(target),
            StatusView::Workspaces,
            false,
            |_, _| Observation::unavailable(Timestamp::now(), IssueCode::EnumerationFailed),
            |_| None,
            |_| {
                let mut writer = database::connect(&path).unwrap();
                writer
                    .exclusive_transaction::<_, diesel::result::Error, _>(|_| Ok(()))
                    .unwrap();
                disk_observation(512)
            },
        )
        .unwrap();
        assert_eq!(report.workspace_disk_usage.len(), 1);
        assert_eq!(report.target_disk_usage.unwrap().allocated_bytes, Some(512));
        assert!(load_with_all_observers(
            None,
            &WorkspaceSelector::Id(WorkspaceId::new()),
            StatusView::Workspaces,
            false,
            |_, _| panic!("failed load must not observe processes"),
            |_| panic!("failed load must not observe sessions"),
            |_| panic!("failed load must not observe disk usage"),
        )
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_views_observe_once_after_closing_the_database() {
        let root = std::env::temp_dir().join(format!("trees-report-{}", WorkspaceId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.sqlite");
        let mut writer = database::connect(&path).unwrap();
        let target = insert_workspace(&mut writer, "/work/api", WorkspaceState::Ready);
        let inner = insert_workspace(&mut writer, "/work/api/nested", WorkspaceState::Removed);
        drop(writer);
        for view in [StatusView::Pools, StatusView::Workspaces, StatusView::Repos] {
            let calls = Cell::new(0);
            let observation_time = Timestamp::parse("2000-01-01T00:00:00Z").unwrap();
            let report = load_with_observer(
                Some(database::connect_read_only(&path).unwrap()),
                &WorkspaceSelector::Id(target),
                view,
                false,
                |workspace, boundaries| {
                    calls.set(calls.get() + 1);
                    assert_eq!(workspace.workspace_id, target);
                    assert!(boundaries
                        .iter()
                        .any(|boundary| boundary.workspace_id == inner));
                    // Observer writes are independent from the already captured status snapshot.
                    let mut writer = database::connect(&path).unwrap();
                    writer
                        .exclusive_transaction::<_, diesel::result::Error, _>(|_| Ok(()))
                        .unwrap();
                    Observation::unavailable(observation_time.clone(), IssueCode::EnumerationFailed)
                },
            )
            .unwrap();
            assert_eq!(calls.get(), 1);
            let json = serde_json::to_value(report).unwrap();
            assert_eq!(json["schema_version"], 2);
            assert_eq!(json["target_processes"]["status"], "unavailable");
            assert_eq!(
                json["target_processes"]["observed_at"],
                observation_time.to_string()
            );
            assert_ne!(json["snapshot_at"], json["target_processes"]["observed_at"]);
            if view == StatusView::Workspaces {
                assert_eq!(json["target_workspace"], json["workspaces"][0]);
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sessions_run_after_storage_closes_and_before_processes() {
        let root = std::env::temp_dir().join(format!("trees-session-order-{}", WorkspaceId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.sqlite");
        let mut writer = database::connect(&path).unwrap();
        let id = insert_workspace(&mut writer, "/work/api", WorkspaceState::Ready);
        drop(writer);
        let calls = Cell::new(0);
        let report = load_with_observers(
            Some(database::connect_read_only(&path).unwrap()),
            &WorkspaceSelector::Id(id),
            StatusView::Workspaces,
            false,
            |_, _| {
                assert_eq!(calls.get(), 1);
                Observation::unavailable(Timestamp::now(), IssueCode::EnumerationFailed)
            },
            |workspaces| {
                calls.set(calls.get() + 1);
                let mut writer = database::connect(&path).unwrap();
                writer
                    .exclusive_transaction::<_, diesel::result::Error, _>(|_| Ok(()))
                    .unwrap();
                let request = session_hook::Request::new(workspaces);
                Some(session_hook::Observation::unavailable(
                    &request,
                    Timestamp::parse("2000-01-01T00:00:00Z").unwrap(),
                    session_hook::Issue::new(session_hook::IssueCode::TimedOut),
                ))
            },
        )
        .unwrap();
        let json = serde_json::to_value(report).unwrap();
        assert_ne!(
            json["snapshot_at"],
            json["workspace_sessions"]["observed_at"]
        );
        assert_eq!(json["workspaces"][0], json["target_workspace"]);
        assert_eq!(calls.get(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skips_observation_without_a_target_and_on_load_errors() {
        let outside = WorkspaceSelector::ContainingDirectory(
            CanonicalPath::from_absolute("/outside").unwrap(),
        );
        for view in [StatusView::Pools, StatusView::Workspaces, StatusView::Repos] {
            for connection in [
                None,
                Some(database::connect(std::path::Path::new(":memory:")).unwrap()),
            ] {
                let report = load_with_observer(connection, &outside, view, false, |_, _| {
                    panic!("no target must not trigger observation")
                })
                .unwrap();
                let json = serde_json::to_value(report).unwrap();
                assert!(json["target_workspace"].is_null());
                assert!(json.get("target_processes").unwrap().is_null());
            }
            assert!(load_with_observer(
                None,
                &WorkspaceSelector::Id(WorkspaceId::new()),
                view,
                false,
                |_, _| panic!("unknown target must not trigger observation")
            )
            .is_err());
        }
        let connection = SqliteConnection::establish(":memory:").unwrap();
        assert!(load_with_observer(
            Some(connection),
            &outside,
            StatusView::Pools,
            false,
            |_, _| panic!("missing schema must not trigger observation")
        )
        .is_err());
    }

    #[test]
    fn concurrent_registration_cannot_change_attribution_boundaries() {
        let root = std::env::temp_dir().join(format!("trees-boundary-{}", WorkspaceId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.sqlite");
        let mut writer = database::connect(&path).unwrap();
        let id = insert_workspace(&mut writer, "/work/api", WorkspaceState::Ready);
        let mut reader = database::connect_read_only(&path).unwrap();
        let (snapshot, boundaries) = load_persisted(
            Some(&mut reader),
            &WorkspaceSelector::Id(id),
            StatusView::Workspaces,
            false,
            || {
                insert_workspace(&mut writer, "/work/api/nested", WorkspaceState::Removed);
            },
        )
        .unwrap();
        assert_eq!(snapshot.target().unwrap().workspace_id, id);
        assert_eq!(boundaries.len(), 1);
        let (_, boundaries) = load_persisted(
            Some(&mut reader),
            &WorkspaceSelector::Id(id),
            StatusView::Workspaces,
            false,
            || {},
        )
        .unwrap();
        assert_eq!(boundaries.len(), 2);
        drop(reader);
        drop(writer);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_successful_empty_observation_remains_distinct_from_no_target() {
        let mut connection = database::connect(std::path::Path::new(":memory:")).unwrap();
        let target = insert_workspace(&mut connection, "/missing", WorkspaceState::Removed);
        let report = load_with_observer(
            Some(connection),
            &WorkspaceSelector::Id(target),
            StatusView::Workspaces,
            false,
            |_, _| Observation {
                observed_at: Timestamp::now(),
                status: Completeness::Complete,
                count: Some(0),
                processes: Vec::new(),
                issues: Vec::new(),
            },
        )
        .unwrap();
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["target_workspace"]["state"], "removed");
        assert_eq!(json["workspaces"], serde_json::json!([]));
        assert_eq!(json["target_processes"]["count"], 0);
    }
}
