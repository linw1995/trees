use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use snafu::{OptionExt, Snafu};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domain::{CanonicalPath, WorkspaceId};
use crate::status::disk_usage::{self, Completeness, Observation};
use crate::storage::disk_usage_cache::{self, CacheError, Candidate};
use crate::workspace_locator::{locate, LocateError, WorkspaceSelector};

const DEBOUNCE_SECONDS: i64 = 5;

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
    observe: impl FnMut(&Path) -> Observation,
) -> Result<RefreshSummary, RefreshError> {
    refresh_with_observer_at(connection, target, OffsetDateTime::now_utc(), observe)
}

fn recent(observation: &Observation, now: OffsetDateTime) -> bool {
    let Ok(measured_at) = OffsetDateTime::parse(observation.observed_at.as_str(), &Rfc3339) else {
        return false;
    };
    let age = now - measured_at;
    age >= time::Duration::ZERO && age < time::Duration::seconds(DEBOUNCE_SECONDS)
}

fn status_label(status: Completeness) -> &'static str {
    match status {
        Completeness::Complete => "complete",
        Completeness::Partial => "partial",
        Completeness::Unavailable => "unavailable",
    }
}

fn refresh_with_observer_at(
    connection: &mut SqliteConnection,
    target: Target,
    now: OffsetDateTime,
    mut observe: impl FnMut(&Path) -> Observation,
) -> Result<RefreshSummary, RefreshError> {
    let selected = connection
        .transaction::<_, RefreshError, _>(|connection| candidates(connection, &target))?;
    let mut summary = RefreshSummary::default();
    let mut observed_paths = HashMap::<CanonicalPath, Observation>::new();
    for candidate in &selected {
        let Some(cached) = &candidate.cached else {
            continue;
        };
        if !recent(cached, now) {
            continue;
        }
        let previous = observed_paths.get(&candidate.path);
        if previous.is_none_or(|previous| cached.observed_at > previous.observed_at) {
            observed_paths.insert(candidate.path.clone(), cached.clone());
        }
    }
    let mut scanned_paths = HashSet::new();
    for candidate in selected {
        let reused = observed_paths.contains_key(&candidate.path);
        let observation = observed_paths
            .entry(candidate.path.clone())
            .or_insert_with(|| {
                eprintln!(
                    "[trees] Measuring disk usage: {:?}",
                    candidate.path.as_path()
                );
                let started = Instant::now();
                let observation = observe(candidate.path.as_path());
                eprintln!(
                    "[trees] Measured disk usage: {:?} status={} bytes={} elapsed_ms={:.1}",
                    candidate.path.as_path(),
                    status_label(observation.status),
                    observation
                        .allocated_bytes
                        .map_or_else(|| "unknown".to_owned(), |bytes| bytes.to_string()),
                    started.elapsed().as_secs_f64() * 1000.0,
                );
                scanned_paths.insert(candidate.path.clone());
                observation
            });
        if reused {
            let source = if scanned_paths.contains(&candidate.path) {
                "current operation"
            } else {
                "debounce window"
            };
            eprintln!(
                "[trees] Reusing disk usage ({source}): {:?} measured_at={}",
                candidate.path.as_path(),
                observation.observed_at
            );
        }
        if !disk_usage_cache::save_if_path_unchanged(
            connection,
            candidate.entity,
            &candidate.path,
            Some(observation),
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
    use std::cell::{Cell, RefCell};

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

        let after_window = OffsetDateTime::now_utc() + time::Duration::seconds(6);
        let summary = refresh_with_observer_at(
            &mut connection,
            Target::Workspace(first),
            after_window,
            |_| observation(Completeness::Partial),
        )
        .unwrap();
        assert_eq!(summary.partial, 3);
        let summary = refresh_with_observer_at(
            &mut connection,
            Target::Path(path("/work/b/repo/nested")),
            after_window,
            |_| observation(Completeness::Unavailable),
        )
        .unwrap();
        assert_eq!(summary.unavailable, 3);
        assert!(disk_usage_cache::workspaces(&mut connection)
            .unwrap()
            .contains_key(&second));
    }

    #[test]
    fn identical_workspace_and_worktree_paths_share_one_scan() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let workspace_id = WorkspaceId::new();
        let worktree_id = RepoWorktreeId::new();
        let workspace_path = path("/work/single");
        let now = Timestamp::now();
        let origin =
            ensure_origin_repository(&mut connection, &path("/origin/.git"), &path("/origin"))
                .unwrap();
        insert_managed_workspace(
            &mut connection,
            &NewManagedWorkspace {
                id: workspace_id,
                canonical_path: workspace_path.clone(),
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
            &mut connection,
            &NewRepoWorktree {
                id: worktree_id,
                workspace_id,
                origin_repository_id: origin.id,
                worktree_path: workspace_path,
                state: RepoWorktreeState::Attached,
                last_head: None,
                last_observed_at: now,
            },
        )
        .unwrap();
        let calls = RefCell::new(Vec::new());
        let summary =
            refresh_with_observer(&mut connection, Target::Workspace(workspace_id), |path| {
                calls.borrow_mut().push(path.to_path_buf());
                observation(Completeness::Complete)
            })
            .unwrap();
        assert_eq!(summary.complete, 3);
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls
                .iter()
                .filter(|path| path.as_path() == Path::new("/work/single"))
                .count(),
            1
        );
        assert_eq!(
            disk_usage_cache::workspaces(&mut connection).unwrap()[&workspace_id],
            disk_usage_cache::worktrees(&mut connection).unwrap()[&worktree_id]
        );

        let workspace_observation = disk_usage_cache::workspaces(&mut connection).unwrap()
            [&workspace_id]
            .clone()
            .unwrap();
        disk_usage_cache::save_if_path_unchanged(
            &mut connection,
            disk_usage_cache::EntityId::Worktree(worktree_id),
            &path("/work/single"),
            None,
        )
        .unwrap();
        let now = OffsetDateTime::parse(workspace_observation.observed_at.as_str(), &Rfc3339)
            .unwrap()
            + time::Duration::seconds(1);
        let summary = refresh_with_observer_at(
            &mut connection,
            Target::Workspace(workspace_id),
            now,
            |_| panic!("a matching recent path must reuse its observation"),
        )
        .unwrap();
        assert_eq!(summary.complete, 3);
        assert_eq!(
            disk_usage_cache::worktrees(&mut connection).unwrap()[&worktree_id],
            Some(workspace_observation)
        );
    }

    #[test]
    fn recent_measurements_debounce_across_database_connections() {
        let root = std::env::temp_dir().join(format!("trees-size-debounce-{}", WorkspaceId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let database_path = root.join("state.sqlite");
        let mut connection = crate::database::connect(&database_path).unwrap();
        let (workspace, _, origin) = fixture(&mut connection);
        let original_time = Timestamp::parse("2026-09-28T00:00:00Z").unwrap();
        let mut cached = observation(Completeness::Complete);
        cached.observed_at = original_time.clone();
        for candidate in disk_usage_cache::workspace_candidates(&mut connection, workspace)
            .unwrap()
            .unwrap()
        {
            disk_usage_cache::save_if_path_unchanged(
                &mut connection,
                candidate.entity,
                &candidate.path,
                Some(&cached),
            )
            .unwrap();
        }
        drop(connection);

        let mut connection = crate::database::connect(&database_path).unwrap();
        let inside_window = OffsetDateTime::parse("2026-09-28T00:00:03Z", &Rfc3339).unwrap();
        let summary = refresh_with_observer_at(
            &mut connection,
            Target::Workspace(workspace),
            inside_window,
            |_| panic!("a recent persisted measurement must not be scanned"),
        )
        .unwrap();
        assert_eq!(summary.complete, 3);
        assert_eq!(
            disk_usage_cache::workspaces(&mut connection).unwrap()[&workspace]
                .as_ref()
                .unwrap()
                .observed_at,
            original_time
        );

        let outside_window = OffsetDateTime::parse("2026-09-28T00:00:05Z", &Rfc3339).unwrap();
        let calls = Cell::new(0);
        let summary = refresh_with_observer_at(
            &mut connection,
            Target::Workspace(workspace),
            outside_window,
            |_| {
                calls.set(calls.get() + 1);
                let mut measured = observation(Completeness::Complete);
                measured.observed_at = Timestamp::parse("2026-09-28T00:00:05Z").unwrap();
                measured
            },
        )
        .unwrap();
        assert_eq!(calls.get(), 3);
        assert_eq!(summary.complete, 3);

        let mut future = observation(Completeness::Complete);
        future.observed_at = Timestamp::parse("2026-09-28T00:00:10Z").unwrap();
        disk_usage_cache::save_if_path_unchanged(
            &mut connection,
            disk_usage_cache::EntityId::Origin(origin),
            &path("/origin"),
            Some(&future),
        )
        .unwrap();
        let before_future = OffsetDateTime::parse("2026-09-28T00:00:06Z", &Rfc3339).unwrap();
        calls.set(0);
        refresh_with_observer_at(
            &mut connection,
            Target::Workspace(workspace),
            before_future,
            |_| {
                calls.set(calls.get() + 1);
                let mut measured = observation(Completeness::Complete);
                measured.observed_at = Timestamp::parse("2026-09-28T00:00:06Z").unwrap();
                measured
            },
        )
        .unwrap();
        assert_eq!(calls.get(), 1);
        drop(connection);
        std::fs::remove_dir_all(root).unwrap();
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
