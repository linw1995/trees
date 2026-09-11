use crate::domain::{CanonicalPath, OriginRepositoryId, Timestamp};
use diesel::prelude::*;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct RepoSnapshot {
    pub schema_version: u8,
    pub view: super::StatusView,
    pub snapshot_at: Timestamp,
    pub target_workspace: Option<super::WorkspaceStatus>,
    pub repos: Vec<RepoStatus>,
}

#[derive(Debug, Serialize)]
pub struct RepoStatus {
    pub origin_repository_id: OriginRepositoryId,
    pub source_path: CanonicalPath,
    pub label: String,
    pub repository_identity: CanonicalPath,
}

impl RepoSnapshot {
    pub fn empty() -> Self {
        Self {
            schema_version: super::STATUS_SCHEMA_VERSION,
            view: super::StatusView::Repos,
            snapshot_at: Timestamp::now(),
            target_workspace: None,
            repos: Vec::new(),
        }
    }
}

pub fn load(connection: &mut SqliteConnection) -> QueryResult<RepoSnapshot> {
    connection.transaction(|connection| load_in_transaction(connection, Timestamp::now()))
}

pub(super) fn load_in_transaction(
    connection: &mut SqliteConnection,
    snapshot_at: Timestamp,
) -> QueryResult<RepoSnapshot> {
    let mut snapshot = RepoSnapshot::empty();
    snapshot.snapshot_at = snapshot_at;
    let rows = crate::storage::origin::list(connection)?;
    let paths = rows
        .iter()
        .map(|row| row.source_path.clone())
        .collect::<Vec<_>>();
    let labels = super::shortest_unique_path_labels(&paths);
    snapshot.repos = rows
        .into_iter()
        .zip(labels)
        .map(|(row, label)| RepoStatus {
            origin_repository_id: row.id,
            source_path: row.source_path,
            label,
            repository_identity: row.repository_identity,
        })
        .collect();
    Ok(snapshot)
}

pub fn render(snapshot: &RepoSnapshot) -> String {
    if snapshot.repos.is_empty() {
        return "No repositories.".to_owned();
    }
    let rows = snapshot
        .repos
        .iter()
        .map(|row| {
            [
                super::escape_human_label(&row.label),
                super::escape_human_label(&row.source_path.to_string()),
                row.origin_repository_id.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    super::render_table(&["REPO", "PATH", "ID"], &rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_stored_origins_with_unique_labels_and_safe_output() {
        let mut db = crate::database::connect(std::path::Path::new(":memory:")).unwrap();
        for value in ["/teams/one/api", "/teams/two/api", "/missing/tab\nrepo"] {
            let path = CanonicalPath::from_absolute(value).unwrap();
            crate::storage::ensure_origin_repository(&mut db, &path, &path).unwrap();
        }
        let snapshot = load(&mut db).unwrap();
        assert_eq!(snapshot.repos.len(), 3);
        assert_eq!(snapshot.repos[1].label, "one/api");
        assert_eq!(snapshot.repos[2].label, "two/api");
        let output = render(&snapshot);
        assert_eq!(output.lines().count(), 4);
        assert!(output.contains("tab\\nrepo"));
    }
}
