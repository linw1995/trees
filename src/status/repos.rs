use crate::domain::{CanonicalPath, OriginRepositoryId, RepositoryManagementMode, Timestamp};
use diesel::prelude::*;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct RepoSnapshot {
    pub schema_version: u8,
    pub view: super::StatusView,
    pub snapshot_at: Timestamp,
    pub repos: Vec<RepoStatus>,
}

#[derive(Debug, Serialize)]
pub struct RepoStatus {
    pub origin_repository_id: OriginRepositoryId,
    pub source_path: CanonicalPath,
    pub label: String,
    pub repository_identity: CanonicalPath,
    pub management_mode: RepositoryManagementMode,
    pub registered: bool,
    pub managed_root: Option<CanonicalPath>,
    pub remote_url: Option<String>,
}

impl RepoSnapshot {
    pub fn empty() -> Self {
        Self {
            schema_version: super::STATUS_SCHEMA_VERSION,
            view: super::StatusView::Repos,
            snapshot_at: Timestamp::now(),
            repos: Vec::new(),
        }
    }
}

pub fn load(connection: &mut SqliteConnection, all: bool) -> QueryResult<RepoSnapshot> {
    connection.transaction(|connection| {
        let mut snapshot = RepoSnapshot::empty();
        let rows = crate::storage::origin::list(connection, all)?;
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
                management_mode: row.management_mode,
                registered: row.registered,
                managed_root: row.managed_root,
                remote_url: row.remote_url,
            })
            .collect();
        Ok(snapshot)
    })
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
                match row.management_mode {
                    RepositoryManagementMode::Automatic => "🤖",
                    RepositoryManagementMode::Manual => "👤",
                }
                .to_owned(),
                if row.registered {
                    "registered"
                } else {
                    "unregistered"
                }
                .to_owned(),
                super::escape_human_label(&row.source_path.to_string()),
                row.origin_repository_id.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    super::render_table(&["REPO", "MODE", "STATUS", "PATH", "ID"], &rows)
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
        let snapshot = load(&mut db, false).unwrap();
        assert_eq!(snapshot.repos.len(), 3);
        assert_eq!(snapshot.repos[1].label, "one/api");
        assert_eq!(snapshot.repos[2].label, "two/api");
        let output = render(&snapshot);
        assert_eq!(output.lines().count(), 4);
        assert!(output.contains("tab\\nrepo"));
        crate::storage::origin::set_registered(
            &mut db,
            snapshot.repos[0].origin_repository_id,
            false,
        )
        .unwrap();
        assert_eq!(load(&mut db, false).unwrap().repos.len(), 2);
        assert_eq!(load(&mut db, true).unwrap().repos.len(), 3);
    }
}
