use serde::{Deserialize, Serialize};

use crate::domain::CanonicalPath;

const HASH_PREFIX: &str = "blake3:";

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositorySetKey {
    hash_key: String,
    repositories_json: String,
}

impl RepositorySetKey {
    pub fn from_repositories(repositories: &[CanonicalPath]) -> Self {
        let canonical = canonical_repository_set(repositories);
        let digest = blake3::hash(canonical.as_bytes());
        Self {
            hash_key: format!("{HASH_PREFIX}{}", digest.to_hex()),
            repositories_json: canonical,
        }
    }

    pub fn hash_key(&self) -> &str {
        &self.hash_key
    }

    pub fn repositories_json(&self) -> &str {
        &self.repositories_json
    }
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
        formatter.write_str(self.hash_key())
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
        assert!(first.hash_key().starts_with(HASH_PREFIX));
        assert_eq!(first.hash_key().len(), HASH_PREFIX.len() + 64);
        assert_eq!(first.repositories_json(), r#"["/repo/api","/repo/web"]"#);
    }
}
