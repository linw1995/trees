use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;

use crate::domain::{ClaimId, OriginRepositoryId, WorkspaceId, WorkspaceState};
use crate::schema::{origin_repositories, workspace_claims, workspaces};

pub fn workspaces(
    connection: &mut SqliteConnection,
) -> QueryResult<Vec<(WorkspaceId, WorkspaceState)>> {
    workspaces::table
        .select((workspaces::id, workspaces::state))
        .load(connection)
}

pub fn claims(connection: &mut SqliteConnection) -> QueryResult<Vec<ClaimId>> {
    workspace_claims::table
        .inner_join(workspaces::table)
        .filter(workspaces::state.ne(WorkspaceState::Removed))
        .select(workspace_claims::id)
        .load(connection)
}

pub fn origins(connection: &mut SqliteConnection) -> QueryResult<Vec<OriginRepositoryId>> {
    origin_repositories::table
        .select(origin_repositories::id)
        .load(connection)
}
