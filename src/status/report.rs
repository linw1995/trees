use diesel::prelude::*;
use serde::Serialize;
use snafu::ResultExt;

use super::combined::{self, Snapshot, SnapshotError};
use super::processes::{self, Boundary, Observation};
use super::target::TargetSelector;
use super::{StatusView, WorkspaceStatus};
use crate::storage::repository;

#[derive(Debug, Serialize)]
pub struct Report {
    #[serde(flatten)]
    pub snapshot: Snapshot,
    pub target_processes: Option<Observation>,
}

pub fn load(
    connection: Option<SqliteConnection>,
    selector: &TargetSelector,
    view: StatusView,
    include_removed: bool,
) -> Result<Report, SnapshotError> {
    load_with_observer(
        connection,
        selector,
        view,
        include_removed,
        |target, boundaries| processes::observe(target.workspace_id, boundaries),
    )
}

fn load_with_observer(
    mut connection: Option<SqliteConnection>,
    selector: &TargetSelector,
    view: StatusView,
    include_removed: bool,
    observer: impl FnOnce(&WorkspaceStatus, &[Boundary]) -> Observation,
) -> Result<Report, SnapshotError> {
    let (snapshot, boundaries) =
        load_persisted(connection.as_mut(), selector, view, include_removed, || {})?;
    drop(connection);
    let target_processes = snapshot
        .target()
        .map(|target| observer(target, &boundaries));
    Ok(Report {
        snapshot,
        target_processes,
    })
}

fn load_persisted(
    connection: Option<&mut SqliteConnection>,
    selector: &TargetSelector,
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
    use std::cell::Cell;

    use super::*;
    use crate::database;
    use crate::domain::*;
    use crate::status::processes::{Completeness, IssueCode};
    use crate::status::tests::insert_workspace;

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
                &TargetSelector::Id(target),
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
    fn skips_observation_without_a_target_and_on_load_errors() {
        let outside = TargetSelector::Directory(CanonicalPath::from_absolute("/outside").unwrap());
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
                &TargetSelector::Id(WorkspaceId::new()),
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
            &TargetSelector::Id(id),
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
            &TargetSelector::Id(id),
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
            &TargetSelector::Id(target),
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
