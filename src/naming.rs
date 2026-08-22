use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use crate::domain::CanonicalPath;
use crate::validation::ValidatedCreateInput;

#[derive(Debug, Clone)]
pub struct WorktreePlan {
    pub repository: CanonicalPath,
    pub name: OsString,
    pub worktree_path: PathBuf,
}

pub fn plan_worktrees(input: &ValidatedCreateInput) -> Result<Vec<WorktreePlan>, NamingError> {
    let mut names = HashMap::with_capacity(input.repositories.len());
    let mut plans = Vec::with_capacity(input.repositories.len());

    for repository in &input.repositories {
        let name = repository
            .as_path()
            .file_name()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| NamingError::MissingRepositoryName(repository.clone()))?
            .to_owned();
        if let Some(previous_repository) = names.insert(name.clone(), repository.clone()) {
            return Err(NamingError::DuplicateName {
                name,
                first_repository: previous_repository,
                second_repository: repository.clone(),
            });
        }

        plans.push(WorktreePlan {
            repository: repository.clone(),
            worktree_path: input.workspace_path.as_path().join(&name),
            name,
        });
    }

    Ok(plans)
}

#[derive(Debug)]
pub enum NamingError {
    MissingRepositoryName(CanonicalPath),
    DuplicateName {
        name: OsString,
        first_repository: CanonicalPath,
        second_repository: CanonicalPath,
    },
}

impl fmt::Display for NamingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRepositoryName(repository) => {
                write!(
                    formatter,
                    "repository has no usable base name: {repository}"
                )
            }
            Self::DuplicateName {
                name,
                first_repository,
                second_repository,
            } => write!(
                formatter,
                "repository name {:?} collides between {} and {}",
                name, first_repository, second_repository
            ),
        }
    }
}

impl std::error::Error for NamingError {}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::validation::validate_create;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-naming-{}", uuid::Uuid::now_v7()))
    }

    fn fake_repository(path: &std::path::Path) {
        fs::create_dir_all(path.join(".git")).expect("fake repository should be created");
    }

    #[test]
    fn derives_direct_child_worktree_paths() {
        let root = test_root();
        let first = root.join("first").join("alpha");
        let second = root.join("second").join("beta");
        fs::create_dir_all(root.join("first")).expect("first parent should be created");
        fs::create_dir_all(root.join("second")).expect("second parent should be created");
        fake_repository(&first);
        fake_repository(&second);
        let input = validate_create(&root.join("workspace"), &[first.clone(), second.clone()])
            .expect("create input should be valid");

        let plans = plan_worktrees(&input).expect("worktree plan should be valid");

        assert_eq!(plans[0].name, OsString::from("alpha"));
        assert_eq!(plans[1].name, OsString::from("beta"));
        assert_eq!(
            plans[0].worktree_path,
            input.workspace_path.as_path().join("alpha")
        );
        assert_eq!(
            plans[1].worktree_path,
            input.workspace_path.as_path().join("beta")
        );
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rejects_duplicate_repository_base_names() {
        let root = test_root();
        let first = root.join("first").join("repo");
        let second = root.join("second").join("repo");
        fs::create_dir_all(root.join("first")).expect("first parent should be created");
        fs::create_dir_all(root.join("second")).expect("second parent should be created");
        fake_repository(&first);
        fake_repository(&second);
        let input = validate_create(&root.join("workspace"), &[first, second])
            .expect("create input should be valid");

        assert!(matches!(
            plan_worktrees(&input),
            Err(NamingError::DuplicateName { .. })
        ));
        fs::remove_dir_all(root).expect("test root should be removable");
    }
}
