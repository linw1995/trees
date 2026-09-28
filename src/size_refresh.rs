use std::path::Path;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use snafu::{OptionExt, Snafu};

use crate::domain::{CanonicalPath, WorkspaceId};
use crate::status::disk_usage::{self, Completeness, Observation};
use crate::storage::disk_usage_cache::{self, CacheError, Candidate};
use crate::workspace_locator::{locate, LocateError, WorkspaceSelector};

#[derive(Debug, Clone)]
enum Target {
    Path(CanonicalPath),
    Workspace(WorkspaceId),
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct RefreshSummary {
    pub complete: usize,
    pub partial: usize,
    pub unavailable: usize,
    pub skipped: usize,
}

#[derive(Debug, Snafu)]
pub enum RefreshError {
    #[snafu(transparent)]
    Cache { source: CacheError },
    #[snafu(transparent)]
    Locate { source: LocateError },
    #[snafu(
        context(false),
        display("failed to select disk usage targets: {source}")
    )]
    Transaction { source: diesel::result::Error },
    #[snafu(display("no registered workspace contains {path}"))]
    CurrentWorkspaceUnknown { path: CanonicalPath },
    #[snafu(display("unknown workspace: {id}"))]
    WorkspaceUnknown { id: WorkspaceId },
}

fn candidates(
    connection: &mut SqliteConnection,
    target: &Target,
) -> Result<Vec<Candidate>, RefreshError> {
    match target {
        Target::Path(path) => {
            let located = locate(
                connection,
                &WorkspaceSelector::ContainingDirectory(path.clone()),
            )?
            .context(CurrentWorkspaceUnknownSnafu { path: path.clone() })?;
            disk_usage_cache::workspace_candidates(connection, located.workspace.id)?.context(
                WorkspaceUnknownSnafu {
                    id: located.workspace.id,
                },
            )
        }
        Target::Workspace(id) => disk_usage_cache::workspace_candidates(connection, *id)?
            .context(WorkspaceUnknownSnafu { id: *id }),
    }
}

pub fn refresh_workspace(
    connection: &mut SqliteConnection,
    id: WorkspaceId,
) -> Result<RefreshSummary, RefreshError> {
    refresh_with_observer(connection, Target::Workspace(id), disk_usage::observe)
}

pub fn refresh_path(
    connection: &mut SqliteConnection,
    path: &CanonicalPath,
) -> Result<RefreshSummary, RefreshError> {
    refresh_with_observer(connection, Target::Path(path.clone()), disk_usage::observe)
}

fn refresh_with_observer(
    connection: &mut SqliteConnection,
    target: Target,
    mut observe: impl FnMut(&Path) -> Observation,
) -> Result<RefreshSummary, RefreshError> {
    let selected = connection
        .transaction::<_, RefreshError, _>(|connection| candidates(connection, &target))?;
    let mut summary = RefreshSummary::default();
    for candidate in selected {
        let observation = observe(candidate.path.as_path());
        if !disk_usage_cache::save_if_path_unchanged(
            connection,
            candidate.entity,
            &candidate.path,
            Some(&observation),
        )? {
            summary.skipped += 1;
            continue;
        }
        match observation.status {
            Completeness::Complete => summary.complete += 1,
            Completeness::Partial => summary.partial += 1,
            Completeness::Unavailable => summary.unavailable += 1,
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::domain::{
        OriginRepositoryId, RepoWorktreeId, RepoWorktreeState, Timestamp, WorkspaceManagementMode,
        WorkspaceState,
    };
    use crate::status::disk_usage::{Issue, IssueCode};
    use crate::storage::{
        ensure_origin_repository, insert_managed_workspace, insert_repo_worktree,
        NewManagedWorkspace, NewRepoWorktree,
    };

    fn path(value: &str) -> CanonicalPath {
        CanonicalPath::from_absolute(value).unwrap()
    }

    fn fixture(
        connection: &mut SqliteConnection,
    ) -> (WorkspaceId, WorkspaceId, OriginRepositoryId) {
        let origin =
            ensure_origin_repository(connection, &path("/origin/.git"), &path("/origin")).unwrap();
        let now = Timestamp::now();
        let mut ids = Vec::new();
        for name in ["a", "b"] {
            let id = WorkspaceId::new();
            ids.push(id);
            let root = path(&format!("/work/{name}"));
            insert_managed_workspace(
                connection,
                &NewManagedWorkspace {
                    id,
                    canonical_path: root.clone(),
                    state: WorkspaceState::Ready,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    last_reconciled_at: Some(now.clone()),
                    management_mode: WorkspaceManagementMode::Manual,
                    pool_id: None,
                    last_released_at: None,
                    removed_at: None,
                },
            )
            .unwrap();
            insert_repo_worktree(
                connection,
                &NewRepoWorktree {
                    id: RepoWorktreeId::new(),
                    workspace_id: id,
                    origin_repository_id: origin.id,
                    worktree_path: path(&format!("/work/{name}/repo")),
                    state: RepoWorktreeState::Attached,
                    last_head: None,
                    last_observed_at: now.clone(),
                },
            )
            .unwrap();
        }
        (ids[0], ids[1], origin.id)
    }

    fn observation(status: Completeness) -> Observation {
        Observation {
            observed_at: Timestamp::now(),
            status,
            allocated_bytes: (status != Completeness::Unavailable).then_some(512),
            issues: (status != Completeness::Complete)
                .then_some(Issue {
                    code: IssueCode::EntryUnreadable,
                    affected_count: Some(1),
                })
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn workspace_refresh_includes_each_worktree_and_shared_origin_once() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let (first, second, origin) = fixture(&mut connection);
        let calls = RefCell::new(Vec::new());
        let summary = refresh_with_observer(&mut connection, Target::Workspace(first), |path| {
            calls.borrow_mut().push(path.to_path_buf());
            observation(Completeness::Complete)
        })
        .unwrap();
        assert_eq!(summary.complete, 3);
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls
                .iter()
                .filter(|path| path.as_path() == Path::new("/origin"))
                .count(),
            1
        );
        assert_eq!(
            disk_usage_cache::workspaces(&mut connection).unwrap().len(),
            2
        );

        assert!(disk_usage_cache::origins(&mut connection).unwrap()[&origin].is_some());
        assert!(disk_usage_cache::workspaces(&mut connection).unwrap()[&second].is_none());

        let summary = refresh_with_observer(&mut connection, Target::Workspace(first), |_| {
            observation(Completeness::Partial)
        })
        .unwrap();
        assert_eq!(summary.partial, 3);
        let summary = refresh_with_observer(
            &mut connection,
            Target::Path(path("/work/b/repo/nested")),
            |_| observation(Completeness::Unavailable),
        )
        .unwrap();
        assert_eq!(summary.unavailable, 3);
        assert!(disk_usage_cache::workspaces(&mut connection)
            .unwrap()
            .contains_key(&second));
    }

    #[test]
    fn missing_explicit_targets_fail_before_observation() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        assert!(matches!(
            refresh_with_observer(
                &mut connection,
                Target::Workspace(WorkspaceId::new()),
                |_| { panic!("unknown target must not be scanned") }
            ),
            Err(RefreshError::WorkspaceUnknown { .. })
        ));
        assert!(matches!(
            refresh_with_observer(&mut connection, Target::Path(path("/outside")), |_| {
                panic!("unknown path must not be scanned")
            }),
            Err(RefreshError::CurrentWorkspaceUnknown { .. })
        ));
    }

    #[test]
    fn changed_path_is_skipped_after_scan_without_a_write_transaction() {
        let root = std::env::temp_dir().join(format!("trees-size-refresh-{}", WorkspaceId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let database_path = root.join("state.sqlite");
        let mut connection = crate::database::connect(&database_path).unwrap();
        let (workspace, _, origin) = fixture(&mut connection);
        let summary = refresh_with_observer(
            &mut connection,
            Target::Workspace(workspace),
            |observed_path| {
                if observed_path == Path::new("/origin") {
                    let mut writer = crate::database::connect(&database_path).unwrap();
                    diesel::update(crate::schema::origin_repositories::table.find(origin))
                        .set(
                            crate::schema::origin_repositories::source_path.eq(path("/new-origin")),
                        )
                        .execute(&mut writer)
                        .unwrap();
                }
                observation(Completeness::Complete)
            },
        )
        .unwrap();
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.complete, 2);
        assert_eq!(
            disk_usage_cache::origins(&mut connection).unwrap()[&origin],
            None
        );
        drop(connection);
        std::fs::remove_dir_all(root).unwrap();
    }
}
