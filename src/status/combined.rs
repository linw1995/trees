use diesel::prelude::*;
use serde::Serialize;
use snafu::{ResultExt, Snafu};

use super::target::{TargetError, TargetSelector};
use super::{repos, PoolStatusSnapshot, StatusSnapshot, StatusView, WorkspaceStatus};
use crate::domain::Timestamp;
use crate::storage::{repository, WorkspaceRow};

#[derive(Debug, Snafu)]
pub enum SnapshotError {
    #[snafu(transparent)]
    Target { source: TargetError },
    #[snafu(context(false), display("failed to load status snapshot: {source}"))]
    Transaction { source: diesel::result::Error },
    #[snafu(
        visibility(pub(super)),
        display("failed to load {view} status: {source}")
    )]
    Inventory {
        view: &'static str,
        source: diesel::result::Error,
    },
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Snapshot {
    Pools(PoolStatusSnapshot),
    Workspaces(StatusSnapshot),
    Repos(repos::RepoSnapshot),
}

impl Snapshot {
    pub fn target(&self) -> Option<&WorkspaceStatus> {
        match self {
            Self::Pools(snapshot) => snapshot.target_workspace.as_ref(),
            Self::Workspaces(snapshot) => snapshot.target_workspace.as_ref(),
            Self::Repos(snapshot) => snapshot.target_workspace.as_ref(),
        }
    }
}

pub fn load(
    connection: Option<&mut SqliteConnection>,
    selector: &TargetSelector,
    view: StatusView,
    include_removed: bool,
) -> Result<Snapshot, SnapshotError> {
    let Some(connection) = connection else {
        selector.select(None)?;
        return Ok(match view {
            StatusView::Pools => Snapshot::Pools(PoolStatusSnapshot::empty()),
            StatusView::Workspaces => Snapshot::Workspaces(StatusSnapshot::empty()),
            StatusView::Repos => Snapshot::Repos(repos::RepoSnapshot::empty()),
        });
    };
    load_with_observer(connection, selector, view, include_removed, || {})
}

fn load_with_observer(
    connection: &mut SqliteConnection,
    selector: &TargetSelector,
    view: StatusView,
    include_removed: bool,
    after_selection: impl FnOnce(),
) -> Result<Snapshot, SnapshotError> {
    connection.transaction(|connection| {
        let snapshot_at = Timestamp::now();
        let target = selector.select(Some(connection))?;
        after_selection();
        match view {
            StatusView::Pools => {
                let mut snapshot =
                    super::load_pools_in_transaction(connection, snapshot_at.clone()).context(
                        InventorySnafu {
                            view: "workspace pool",
                        },
                    )?;
                snapshot.target_workspace = load_target(connection, target, snapshot_at)?;
                Ok(Snapshot::Pools(snapshot))
            }
            StatusView::Workspaces => {
                let mut snapshot = super::load_workspaces_in_transaction(
                    connection,
                    include_removed,
                    snapshot_at.clone(),
                )
                .context(InventorySnafu { view: "workspace" })?;
                snapshot.target_workspace = match target {
                    Some(row) => match snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id == row.id)
                    {
                        Some(workspace) => Some(workspace.clone()),
                        None => load_target(connection, Some(row), snapshot_at)?,
                    },
                    None => None,
                };
                Ok(Snapshot::Workspaces(snapshot))
            }
            StatusView::Repos => {
                let mut snapshot = repos::load_in_transaction(connection, snapshot_at.clone())
                    .context(InventorySnafu { view: "repository" })?;
                snapshot.target_workspace = load_target(connection, target, snapshot_at)?;
                Ok(Snapshot::Repos(snapshot))
            }
        }
    })
}

fn load_target(
    connection: &mut SqliteConnection,
    target: Option<WorkspaceRow>,
    snapshot_at: Timestamp,
) -> QueryResult<Option<WorkspaceStatus>> {
    let Some(row) = target else { return Ok(None) };
    let id = Some(row.id);
    let claims = repository::list_status_workspace_claims_for_target(connection, true, id)?;
    let operations = repository::list_status_leased_operations_for_target(connection, true, id)?;
    let events = repository::list_status_operation_events_for_target(connection, true, id)?;
    let repos = repository::list_status_repo_worktrees_for_target(connection, true, id)?;
    Ok(
        super::assemble_snapshot(snapshot_at, vec![row], claims, operations, events, repos)?
            .workspaces
            .pop(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::domain::*;
    use crate::status::tests::{insert_automatic_workspace, insert_workspace};
    use crate::storage::*;

    #[test]
    fn concurrent_claim_changes_cannot_split_target_and_inventory() {
        let path =
            std::env::temp_dir().join(format!("trees-snapshot-{}.sqlite", WorkspaceId::new()));
        let mut writer = database::connect(&path).unwrap();
        let pool_id = PoolId::new();
        insert_workspace_pool(
            &mut writer,
            &NewWorkspacePool {
                id: pool_id,
                hash_key: "snapshot".into(),
                repository_ids: "[]".into(),
            },
        )
        .unwrap();
        let id = insert_automatic_workspace(
            &mut writer,
            "/snapshot/target",
            WorkspaceState::Ready,
            pool_id,
        );
        let mut reader = database::connect_read_only(&path).unwrap();
        let selector = TargetSelector::Id(id);
        let snapshot = load_with_observer(&mut reader, &selector, StatusView::Pools, false, || {
            insert_workspace_claim(
                &mut writer,
                &NewWorkspaceClaim {
                    id: ClaimId::new(),
                    workspace_id: id,
                    claimed_at: Timestamp::now(),
                },
            )
            .unwrap();
        })
        .unwrap();
        let Snapshot::Pools(before) = snapshot else {
            panic!("expected pools")
        };
        assert_eq!(before.pools[0].available, 1);
        assert!(before.target_workspace.unwrap().claim.is_none());
        let Snapshot::Pools(after) =
            load(Some(&mut reader), &selector, StatusView::Pools, false).unwrap()
        else {
            panic!("expected pools")
        };
        assert_eq!(after.pools[0].available, 0);
        assert!(after.target_workspace.unwrap().claim.is_some());
        drop(reader);
        drop(writer);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn every_view_preserves_complete_removed_targets_and_global_inventory() {
        let mut db = database::connect(std::path::Path::new(":memory:")).unwrap();
        let id = insert_workspace(&mut db, "/missing/removed", WorkspaceState::Removed);
        let other = insert_workspace(&mut db, "/missing/current", WorkspaceState::Ready);
        let origin_path = CanonicalPath::from_absolute("/missing/origin").unwrap();
        let origin = ensure_origin_repository(&mut db, &origin_path, &origin_path).unwrap();
        insert_repo_worktree(
            &mut db,
            &NewRepoWorktree {
                id: RepoWorktreeId::new(),
                workspace_id: id,
                origin_repository_id: origin.id,
                worktree_path: CanonicalPath::from_absolute("/missing/removed/repo").unwrap(),
                state: RepoWorktreeState::Removed,
                last_head: Some("abc".into()),
                last_observed_at: Timestamp::now(),
            },
        )
        .unwrap();
        insert_workspace_claim(
            &mut db,
            &NewWorkspaceClaim {
                id: ClaimId::new(),
                workspace_id: id,
                claimed_at: Timestamp::now(),
            },
        )
        .unwrap();
        persist_operation_intent(
            &mut db,
            &OperationIntent::new(
                id,
                "release",
                Timestamp::parse("2000-01-01T00:00:00Z").unwrap(),
                "release",
                JsonDocument::parse("{}").unwrap(),
            ),
        )
        .unwrap();
        let expected = super::super::load_snapshot(&mut db, true)
            .unwrap()
            .workspaces
            .into_iter()
            .find(|row| row.workspace_id == id)
            .unwrap();
        for (view, key) in [
            (StatusView::Pools, "pools"),
            (StatusView::Workspaces, "workspaces"),
            (StatusView::Repos, "repos"),
        ] {
            let snapshot = load(Some(&mut db), &TargetSelector::Id(id), view, false).unwrap();
            assert_eq!(snapshot.target(), Some(&expected));
            let json = serde_json::to_value(&snapshot).unwrap();
            assert_eq!(json["schema_version"], 2);
            assert_eq!(json["view"], key);
            assert!(json["snapshot_at"].is_string());
            assert!(json[key].is_array());
            assert_eq!(
                json["target_workspace"],
                serde_json::to_value(&expected).unwrap()
            );
            if view == StatusView::Workspaces {
                assert_eq!(json[key].as_array().unwrap().len(), 1);
                assert_eq!(json[key][0]["workspace_id"], other.to_string());
            }
            let outside =
                TargetSelector::Directory(CanonicalPath::from_absolute("/outside").unwrap());
            let json =
                serde_json::to_value(load(Some(&mut db), &outside, view, false).unwrap()).unwrap();
            assert_eq!(json["target_workspace"], serde_json::Value::Null);
            assert!(json.get("target_workspace").is_some());
        }
        let snapshot = load(
            Some(&mut db),
            &TargetSelector::Id(other),
            StatusView::Workspaces,
            false,
        )
        .unwrap();
        let Snapshot::Workspaces(snapshot) = snapshot else {
            panic!("expected workspaces")
        };
        assert_eq!(
            snapshot.target_workspace.as_ref(),
            snapshot.workspaces.first()
        );
    }
}
