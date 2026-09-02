use serde::{Deserialize, Serialize};

use crate::domain::CanonicalPath;

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
        assert_eq!(first.as_str(), r#"["/repo/api","/repo/web"]"#);
    }
}
