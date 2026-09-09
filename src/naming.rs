use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use snafu::Snafu;

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
    let single_repository = input.repositories.len() == 1;

    for repository in &input.repositories {
        let name = repository
            .as_path()
            .file_name()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| NamingError::MissingRepositoryName {
                repository: repository.clone(),
            })?
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
            worktree_path: if single_repository {
                input.workspace_path.as_path().to_owned()
            } else {
                input.workspace_path.as_path().join(&name)
            },
            name,
        });
    }

    Ok(plans)
}

#[derive(Debug, Snafu)]
pub enum NamingError {
    #[snafu(display("repository has no usable base name: {repository}"))]
    MissingRepositoryName { repository: CanonicalPath },
    #[snafu(display(
        "repository name {name:?} collides between {first_repository} and {second_repository}"
    ))]
    DuplicateName {
        name: OsString,
        first_repository: CanonicalPath,
        second_repository: CanonicalPath,
    },
}

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
    fn uses_the_workspace_path_for_a_single_repository() {
        let root = test_root();
        let repository = root.join("source").join("alpha");
        fs::create_dir_all(root.join("source")).expect("source parent should be created");
        fake_repository(&repository);
        let input = validate_create(&root.join("workspace"), &[repository])
            .expect("create input should be valid");

        let plans = plan_worktrees(&input).expect("worktree plan should be valid");

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].name, OsString::from("alpha"));
        assert_eq!(plans[0].worktree_path, input.workspace_path.as_path());
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
