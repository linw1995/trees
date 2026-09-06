use serde::{Deserialize, Serialize};

use crate::domain::OriginRepositoryId;

const HASH_PREFIX: &str = "blake3:";

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositorySetKey {
    hash_key: String,
    repository_ids: String,
}

impl RepositorySetKey {
    pub fn from_repository_ids(repository_ids: &[OriginRepositoryId]) -> Self {
        let canonical = canonical_repository_ids(repository_ids);
        let digest = blake3::hash(canonical.as_bytes());
        Self {
            hash_key: format!("{HASH_PREFIX}{}", digest.to_hex()),
            repository_ids: canonical,
        }
    }

    pub fn hash_key(&self) -> &str {
        &self.hash_key
    }

    pub fn repository_ids(&self) -> &str {
        &self.repository_ids
    }
}

fn canonical_repository_ids(repository_ids: &[OriginRepositoryId]) -> String {
    let mut ids = repository_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    ids.sort();
    serde_json::to_string(&ids).expect("repository IDs should serialize as JSON")
}

impl std::fmt::Display for RepositorySetKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.hash_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_set_key_is_sorted_and_order_independent() {
        let web = OriginRepositoryId::new();
        let api = OriginRepositoryId::new();
        let first = RepositorySetKey::from_repository_ids(&[web, api]);
        let second = RepositorySetKey::from_repository_ids(&[api, web]);
        let mut expected = [web.to_string(), api.to_string()];
        expected.sort();

        assert_eq!(first, second);
        assert!(first.hash_key().starts_with(HASH_PREFIX));
        assert_eq!(first.hash_key().len(), HASH_PREFIX.len() + 64);
        assert_eq!(
            first.repository_ids(),
            serde_json::to_string(&expected).unwrap()
        );
    }
}
