use serde::{Deserialize, Serialize};

use crate::domain::CanonicalPath;

const HASH_PREFIX: &str = "blake3:";

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositorySetKey(String);

impl RepositorySetKey {
    pub fn from_repositories(repositories: &[CanonicalPath]) -> Self {
        let canonical = canonical_repository_set(repositories);
        let digest = blake3::hash(canonical.as_bytes());
        Self(format!("{HASH_PREFIX}{}", digest.to_hex()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn legacy_repository_set_key(repositories: &[CanonicalPath]) -> String {
    canonical_repository_set(repositories)
}

fn canonical_repository_set(repositories: &[CanonicalPath]) -> String {
    let mut identities = repositories
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    identities.sort();
    serde_json::to_string(&identities).expect("repository identities should serialize as JSON")
}

impl std::fmt::Display for RepositorySetKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: &str) -> CanonicalPath {
        CanonicalPath::from_absolute(value).expect("test path should be absolute")
    }

    #[test]
    fn repository_set_key_is_sorted_and_order_independent() {
        let first = RepositorySetKey::from_repositories(&[path("/repo/web"), path("/repo/api")]);
        let second = RepositorySetKey::from_repositories(&[path("/repo/api"), path("/repo/web")]);

        assert_eq!(first, second);
        assert!(first.as_str().starts_with(HASH_PREFIX));
        assert_eq!(first.as_str().len(), HASH_PREFIX.len() + 64);
        assert_ne!(
            first.as_str(),
            legacy_repository_set_key(&[path("/repo/api"), path("/repo/web")])
        );
    }
}
