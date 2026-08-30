use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::domain::{CanonicalPath, Timestamp, WorkspaceId};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositorySetKey(String);

impl RepositorySetKey {
    pub fn from_repositories(repositories: &[CanonicalPath]) -> Self {
        let mut identities = repositories
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        identities.sort();
        Self(
            serde_json::to_string(&identities)
                .expect("repository identities should serialize as JSON"),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RepositorySetKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PoolCandidate {
    pub id: WorkspaceId,
    pub last_checked_in_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

pub fn compare_candidates(left: &PoolCandidate, right: &PoolCandidate) -> Ordering {
    idle_timestamp(left)
        .cmp(idle_timestamp(right))
        .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
}

fn idle_timestamp(candidate: &PoolCandidate) -> &Timestamp {
    candidate
        .last_checked_in_at
        .as_ref()
        .unwrap_or(&candidate.created_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: &str) -> CanonicalPath {
        CanonicalPath::from_absolute(value).expect("test path should be absolute")
    }

    fn timestamp(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("test timestamp should be valid")
    }

    #[test]
    fn repository_set_key_is_sorted_and_order_independent() {
        let first = RepositorySetKey::from_repositories(&[path("/repo/web"), path("/repo/api")]);
        let second = RepositorySetKey::from_repositories(&[path("/repo/api"), path("/repo/web")]);

        assert_eq!(first, second);
        assert_eq!(first.as_str(), r#"["/repo/api","/repo/web"]"#);
    }

    #[test]
    fn candidates_use_last_checkin_then_creation_and_id() {
        let created = timestamp("2026-01-01T00:00:00Z");
        let checked_in = timestamp("2026-02-01T00:00:00Z");
        let mut candidates = vec![
            PoolCandidate {
                id: WorkspaceId::new(),
                last_checked_in_at: Some(checked_in.clone()),
                created_at: created.clone(),
            },
            PoolCandidate {
                id: WorkspaceId::new(),
                last_checked_in_at: None,
                created_at: created,
            },
        ];

        candidates.sort_by(compare_candidates);

        assert!(candidates[0].last_checked_in_at.is_none());
    }
}
