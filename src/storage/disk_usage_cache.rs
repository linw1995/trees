use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use snafu::{ensure, ResultExt, Snafu};

use crate::domain::{CanonicalPath, OriginRepositoryId, RepoWorktreeId, WorkspaceId};
use crate::domain::{RepoWorktreeState, WorkspaceState};
use crate::schema::{origin_repositories, repo_worktrees, workspaces};
use crate::status::disk_usage::{Completeness, Observation};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum EntityId {
    Workspace(WorkspaceId),
    Worktree(RepoWorktreeId),
    Origin(OriginRepositoryId),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Candidate {
    pub entity: EntityId,
    pub path: CanonicalPath,
}

impl Display for EntityId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workspace(id) => write!(formatter, "workspace {id}"),
            Self::Worktree(id) => write!(formatter, "worktree {id}"),
            Self::Origin(id) => write!(formatter, "origin {id}"),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum CacheError {
    #[snafu(display("failed to query {kind} disk usage cache: {source}"))]
    Query {
        kind: &'static str,
        source: diesel::result::Error,
    },
    #[snafu(display("failed to serialize disk usage observation: {source}"))]
    Serialize { source: serde_json::Error },
    #[snafu(display("failed to decode disk usage for {entity}: {source}"))]
    Decode {
        entity: String,
        source: serde_json::Error,
    },
    #[snafu(display("invalid disk usage observation for {entity}"))]
    Invalid { entity: String },
}

fn decode<I: Copy + Eq + Hash + Display>(
    rows: Vec<(I, Option<String>)>,
) -> Result<HashMap<I, Option<Observation>>, CacheError> {
    rows.into_iter()
        .map(|(id, json)| {
            let observation = json
                .map(|json| {
                    let observation: Observation =
                        serde_json::from_str(&json).context(DecodeSnafu {
                            entity: id.to_string(),
                        })?;
                    let valid = match observation.status {
                        Completeness::Complete => {
                            observation.allocated_bytes.is_some() && observation.issues.is_empty()
                        }
                        Completeness::Partial => {
                            observation.allocated_bytes.is_some() && !observation.issues.is_empty()
                        }
                        Completeness::Unavailable => {
                            observation.allocated_bytes.is_none() && !observation.issues.is_empty()
                        }
                    };
                    ensure!(
                        valid,
                        InvalidSnafu {
                            entity: id.to_string()
                        }
                    );
                    Ok(observation)
                })
                .transpose()?;
            Ok((id, observation))
        })
        .collect()
}

pub fn workspaces(
    connection: &mut SqliteConnection,
) -> Result<HashMap<WorkspaceId, Option<Observation>>, CacheError> {
    let rows = workspaces::table
        .select((workspaces::id, workspaces::disk_usage_json))
        .load(connection)
        .context(QuerySnafu { kind: "workspace" })?;
    decode(rows)
}

pub fn worktrees(
    connection: &mut SqliteConnection,
) -> Result<HashMap<RepoWorktreeId, Option<Observation>>, CacheError> {
    let rows = repo_worktrees::table
        .select((repo_worktrees::id, repo_worktrees::disk_usage_json))
        .load(connection)
        .context(QuerySnafu { kind: "worktree" })?;
    decode(rows)
}

pub fn origins(
    connection: &mut SqliteConnection,
) -> Result<HashMap<OriginRepositoryId, Option<Observation>>, CacheError> {
    let rows = origin_repositories::table
        .select((
            origin_repositories::id,
            origin_repositories::disk_usage_json,
        ))
        .load(connection)
        .context(QuerySnafu { kind: "origin" })?;
    decode(rows)
}

pub fn workspace_candidates(
    connection: &mut SqliteConnection,
    id: WorkspaceId,
) -> Result<Option<Vec<Candidate>>, CacheError> {
    let workspace = workspaces::table
        .find(id)
        .select((workspaces::id, workspaces::canonical_path))
        .first::<(WorkspaceId, CanonicalPath)>(connection)
        .optional()
        .context(QuerySnafu { kind: "workspace" })?;
    let Some((workspace_id, workspace_path)) = workspace else {
        return Ok(None);
    };
    let worktrees = repo_worktrees::table
        .filter(repo_worktrees::workspace_id.eq(id))
        .filter(repo_worktrees::state.ne(RepoWorktreeState::Removed))
        .order(repo_worktrees::worktree_path.asc())
        .select((
            repo_worktrees::id,
            repo_worktrees::origin_repository_id,
            repo_worktrees::worktree_path,
        ))
        .load::<(RepoWorktreeId, OriginRepositoryId, CanonicalPath)>(connection)
        .context(QuerySnafu { kind: "worktree" })?;
    let origin_ids = worktrees
        .iter()
        .map(|(_, id, _)| *id)
        .collect::<std::collections::HashSet<_>>();
    let origins = origin_repositories::table
        .order(origin_repositories::source_path.asc())
        .select((origin_repositories::id, origin_repositories::source_path))
        .load::<(OriginRepositoryId, CanonicalPath)>(connection)
        .context(QuerySnafu { kind: "origin" })?;
    let mut candidates = vec![Candidate {
        entity: EntityId::Workspace(workspace_id),
        path: workspace_path,
    }];
    candidates.extend(worktrees.into_iter().map(|(id, _, path)| Candidate {
        entity: EntityId::Worktree(id),
        path,
    }));
    candidates.extend(
        origins
            .into_iter()
            .filter(|(id, _)| origin_ids.contains(id))
            .map(|(id, path)| Candidate {
                entity: EntityId::Origin(id),
                path,
            }),
    );
    Ok(Some(candidates))
}

pub fn origin_candidate(
    connection: &mut SqliteConnection,
    id: OriginRepositoryId,
) -> Result<Option<Candidate>, CacheError> {
    origin_repositories::table
        .find(id)
        .select((origin_repositories::id, origin_repositories::source_path))
        .first::<(OriginRepositoryId, CanonicalPath)>(connection)
        .optional()
        .context(QuerySnafu { kind: "origin" })
        .map(|row| {
            row.map(|(id, path)| Candidate {
                entity: EntityId::Origin(id),
                path,
            })
        })
}

pub fn all_candidates(connection: &mut SqliteConnection) -> Result<Vec<Candidate>, CacheError> {
    let workspaces = workspaces::table
        .filter(workspaces::state.ne(WorkspaceState::Removed))
        .order(workspaces::canonical_path.asc())
        .select((workspaces::id, workspaces::canonical_path))
        .load::<(WorkspaceId, CanonicalPath)>(connection)
        .context(QuerySnafu { kind: "workspace" })?;
    let current_ids = workspaces
        .iter()
        .map(|(id, _)| *id)
        .collect::<std::collections::HashSet<_>>();
    let worktrees = repo_worktrees::table
        .filter(repo_worktrees::state.ne(RepoWorktreeState::Removed))
        .order(repo_worktrees::worktree_path.asc())
        .select((
            repo_worktrees::id,
            repo_worktrees::workspace_id,
            repo_worktrees::worktree_path,
        ))
        .load::<(RepoWorktreeId, WorkspaceId, CanonicalPath)>(connection)
        .context(QuerySnafu { kind: "worktree" })?;
    let origins = origin_repositories::table
        .order(origin_repositories::source_path.asc())
        .select((origin_repositories::id, origin_repositories::source_path))
        .load::<(OriginRepositoryId, CanonicalPath)>(connection)
        .context(QuerySnafu { kind: "origin" })?;
    let mut candidates = Vec::with_capacity(workspaces.len() + worktrees.len() + origins.len());
    candidates.extend(workspaces.into_iter().map(|(id, path)| Candidate {
        entity: EntityId::Workspace(id),
        path,
    }));
    candidates.extend(
        worktrees
            .into_iter()
            .filter(|(_, workspace_id, _)| current_ids.contains(workspace_id))
            .map(|(id, _, path)| Candidate {
                entity: EntityId::Worktree(id),
                path,
            }),
    );
    candidates.extend(origins.into_iter().map(|(id, path)| Candidate {
        entity: EntityId::Origin(id),
        path,
    }));
    Ok(candidates)
}

pub fn save_if_path_unchanged(
    connection: &mut SqliteConnection,
    entity: EntityId,
    path: &CanonicalPath,
    observation: Option<&Observation>,
) -> Result<bool, CacheError> {
    let json = observation
        .map(|observation| serde_json::to_string(observation).context(SerializeSnafu))
        .transpose()?;
    let changed = match entity {
        EntityId::Workspace(id) => diesel::update(
            workspaces::table
                .filter(workspaces::id.eq(id))
                .filter(workspaces::canonical_path.eq(path)),
        )
        .set(workspaces::disk_usage_json.eq(&json))
        .execute(connection)
        .context(QuerySnafu { kind: "workspace" })?,
        EntityId::Worktree(id) => diesel::update(
            repo_worktrees::table
                .filter(repo_worktrees::id.eq(id))
                .filter(repo_worktrees::worktree_path.eq(path)),
        )
        .set(repo_worktrees::disk_usage_json.eq(&json))
        .execute(connection)
        .context(QuerySnafu { kind: "worktree" })?,
        EntityId::Origin(id) => diesel::update(
            origin_repositories::table
                .filter(origin_repositories::id.eq(id))
                .filter(origin_repositories::source_path.eq(path)),
        )
        .set(origin_repositories::disk_usage_json.eq(&json))
        .execute(connection)
        .context(QuerySnafu { kind: "origin" })?,
    };
    Ok(changed == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{RepoWorktreeState, Timestamp, WorkspaceManagementMode, WorkspaceState};
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
    ) -> (WorkspaceId, RepoWorktreeId, OriginRepositoryId) {
        let workspace_id = WorkspaceId::new();
        let now = Timestamp::now();
        insert_managed_workspace(
            connection,
            &NewManagedWorkspace {
                id: workspace_id,
                canonical_path: path("/workspace"),
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
        let origin =
            ensure_origin_repository(connection, &path("/origin/.git"), &path("/origin")).unwrap();
        let worktree_id = RepoWorktreeId::new();
        insert_repo_worktree(
            connection,
            &NewRepoWorktree {
                id: worktree_id,
                workspace_id,
                origin_repository_id: origin.id,
                worktree_path: path("/workspace/repo"),
                state: RepoWorktreeState::Attached,
                last_head: None,
                last_observed_at: now,
            },
        )
        .unwrap();
        (workspace_id, worktree_id, origin.id)
    }

    fn observation(status: Completeness) -> Observation {
        Observation {
            observed_at: Timestamp::parse("2026-09-28T00:00:00Z").unwrap(),
            status,
            allocated_bytes: (status != Completeness::Unavailable).then_some(4096),
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
    fn new_columns_start_unknown_and_round_trip_observations() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let (workspace_id, worktree_id, origin_id) = fixture(&mut connection);
        assert_eq!(workspaces(&mut connection).unwrap()[&workspace_id], None);
        assert_eq!(worktrees(&mut connection).unwrap()[&worktree_id], None);
        assert_eq!(origins(&mut connection).unwrap()[&origin_id], None);

        for (entity, entity_path, status) in [
            (
                EntityId::Workspace(workspace_id),
                path("/workspace"),
                Completeness::Complete,
            ),
            (
                EntityId::Worktree(worktree_id),
                path("/workspace/repo"),
                Completeness::Partial,
            ),
            (
                EntityId::Origin(origin_id),
                path("/origin"),
                Completeness::Unavailable,
            ),
        ] {
            assert!(save_if_path_unchanged(
                &mut connection,
                entity,
                &entity_path,
                Some(&observation(status)),
            )
            .unwrap());
        }
        assert_eq!(
            workspaces(&mut connection).unwrap()[&workspace_id],
            Some(observation(Completeness::Complete))
        );
        assert_eq!(
            worktrees(&mut connection).unwrap()[&worktree_id],
            Some(observation(Completeness::Partial))
        );
        assert_eq!(
            origins(&mut connection).unwrap()[&origin_id],
            Some(observation(Completeness::Unavailable))
        );
    }

    #[test]
    fn path_changes_invalidate_caches_and_reject_stale_writes() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let (_, worktree_id, origin_id) = fixture(&mut connection);
        let measured = observation(Completeness::Complete);
        assert!(save_if_path_unchanged(
            &mut connection,
            EntityId::Origin(origin_id),
            &path("/origin"),
            Some(&measured),
        )
        .unwrap());
        assert!(save_if_path_unchanged(
            &mut connection,
            EntityId::Worktree(worktree_id),
            &path("/workspace/repo"),
            Some(&measured),
        )
        .unwrap());
        diesel::update(origin_repositories::table.find(origin_id))
            .set(origin_repositories::source_path.eq(path("/new-origin")))
            .execute(&mut connection)
            .unwrap();
        diesel::update(repo_worktrees::table.find(worktree_id))
            .set(repo_worktrees::worktree_path.eq(path("/workspace/new-repo")))
            .execute(&mut connection)
            .unwrap();
        assert_eq!(origins(&mut connection).unwrap()[&origin_id], None);
        assert_eq!(worktrees(&mut connection).unwrap()[&worktree_id], None);
        assert!(!save_if_path_unchanged(
            &mut connection,
            EntityId::Origin(origin_id),
            &path("/origin"),
            Some(&measured),
        )
        .unwrap());
        assert!(!save_if_path_unchanged(
            &mut connection,
            EntityId::Worktree(worktree_id),
            &path("/workspace/repo"),
            Some(&measured),
        )
        .unwrap());
    }

    #[test]
    fn rejects_malformed_or_inconsistent_cache_json() {
        let mut connection = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        let (workspace_id, _, _) = fixture(&mut connection);
        diesel::update(workspaces::table.find(workspace_id))
            .set(workspaces::disk_usage_json.eq(Some("{}")))
            .execute(&mut connection)
            .unwrap();
        assert!(matches!(
            workspaces(&mut connection),
            Err(CacheError::Decode { .. })
        ));
        let invalid = serde_json::json!({
            "observed_at": "2026-09-28T00:00:00Z",
            "status": "complete",
            "allocated_bytes": null,
            "issues": []
        })
        .to_string();
        diesel::update(workspaces::table.find(workspace_id))
            .set(workspaces::disk_usage_json.eq(Some(invalid)))
            .execute(&mut connection)
            .unwrap();
        assert!(matches!(
            workspaces(&mut connection),
            Err(CacheError::Invalid { .. })
        ));
    }
}
