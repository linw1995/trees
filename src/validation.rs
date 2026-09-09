use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::{CanonicalPath, CanonicalPathError};
use snafu::{ResultExt, Snafu};

pub fn validate_workspace_root(
    workspace_path: &Path,
    managed_root: &Path,
    expected_worktree_paths: &[PathBuf],
) -> Result<(), WorkspaceRootError> {
    if !workspace_path.is_absolute() || !managed_root.is_absolute() {
        return Err(WorkspaceRootError::NotAbsolute {
            workspace: workspace_path.to_owned(),
            managed_root: managed_root.to_owned(),
        });
    }
    if !workspace_path.starts_with(managed_root) {
        return Err(WorkspaceRootError::OutsideManagedRoot {
            workspace: workspace_path.to_owned(),
            managed_root: managed_root.to_owned(),
        });
    }
    if !workspace_path.is_dir() {
        return Err(WorkspaceRootError::NotDirectory {
            path: workspace_path.to_owned(),
        });
    }

    if expected_worktree_paths == [workspace_path] {
        return Ok(());
    }

    let expected = expected_worktree_paths
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    for entry in fs::read_dir(workspace_path).context(ReadDirectoryRootSnafu {
        path: workspace_path,
    })? {
        let entry = entry.context(ReadDirectoryRootSnafu {
            path: workspace_path,
        })?;
        if !expected.contains(entry.path().as_path()) {
            return Err(WorkspaceRootError::UnexpectedEntry { path: entry.path() });
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct ValidatedCreateInput {
    pub workspace_path: CanonicalPath,
    pub repositories: Vec<CanonicalPath>,
}

pub fn validate_repositories(
    repository_paths: &[PathBuf],
) -> Result<Vec<CanonicalPath>, ValidationError> {
    if repository_paths.is_empty() {
        return Err(ValidationError::NoRepositories);
    }

    let mut repositories = Vec::with_capacity(repository_paths.len());
    let mut identities = HashSet::with_capacity(repository_paths.len());
    for repository_path in repository_paths {
        let metadata = fs::metadata(repository_path).context(PathIoSnafu {
            path: repository_path,
        })?;
        if !metadata.is_dir() {
            return Err(ValidationError::NotDirectory {
                path: repository_path.clone(),
            });
        }

        let repository = CanonicalPath::resolve(repository_path).context(CanonicalizeSnafu)?;
        if !repository.as_path().join(".git").exists() {
            return Err(ValidationError::NotGitRepository {
                path: repository.into_path_buf(),
            });
        }
        if !identities.insert(repository.as_path().to_owned()) {
            return Err(ValidationError::DuplicateRepository {
                path: repository.into_path_buf(),
            });
        }

        repositories.push(repository);
    }

    Ok(repositories)
}

pub fn validate_create(
    workspace_path: &Path,
    repository_paths: &[PathBuf],
) -> Result<ValidatedCreateInput, ValidationError> {
    if repository_paths.is_empty() {
        return Err(ValidationError::NoRepositories);
    }

    let workspace_path = resolve_workspace_path(workspace_path)?;
    if workspace_path.as_path().exists() {
        return Err(ValidationError::WorkspaceExists {
            path: workspace_path.into_path_buf(),
        });
    }

    let repositories = validate_repositories(repository_paths)?;
    for repository in &repositories {
        if workspace_path.as_path().starts_with(repository.as_path()) {
            return Err(ValidationError::WorkspaceInsideRepository {
                workspace: workspace_path.as_path().to_owned(),
                repository: repository.as_path().to_owned(),
            });
        }
    }

    Ok(ValidatedCreateInput {
        workspace_path,
        repositories,
    })
}

pub fn validate_new_workspace_target(path: &Path) -> Result<CanonicalPath, ValidationError> {
    let path = resolve_workspace_path(path)?;
    snafu::ensure!(
        !path.as_path().exists(),
        WorkspaceExistsSnafu {
            path: path.as_path()
        }
    );
    Ok(path)
}

pub fn resolve_workspace_path(path: &Path) -> Result<CanonicalPath, ValidationError> {
    if path.exists() {
        return CanonicalPath::resolve(path).context(CanonicalizeSnafu);
    }

    let absolute_path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .context(PathIoSnafu { path })?
            .join(path)
    };
    let file_name =
        absolute_path
            .file_name()
            .ok_or_else(|| ValidationError::InvalidWorkspacePath {
                path: absolute_path.clone(),
            })?;
    let parent = absolute_path
        .parent()
        .ok_or_else(|| ValidationError::InvalidWorkspacePath {
            path: absolute_path.clone(),
        })?;
    let parent = CanonicalPath::resolve(parent).context(CanonicalizeSnafu)?;

    CanonicalPath::from_absolute(parent.as_path().join(file_name)).context(CanonicalizeSnafu)
}

#[derive(Debug, Snafu)]
pub enum ValidationError {
    #[snafu(display("at least one repository is required"))]
    NoRepositories,
    #[snafu(display("workspace path already exists: {}", path.display()))]
    WorkspaceExists { path: PathBuf },
    #[snafu(display("invalid workspace path: {}", path.display()))]
    InvalidWorkspacePath { path: PathBuf },
    #[snafu(display("failed to inspect {}: {source}", path.display()))]
    PathIo {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("{source}"), visibility(pub(crate)))]
    Canonicalize { source: CanonicalPathError },
    #[snafu(display("path is not a directory: {}", path.display()))]
    NotDirectory { path: PathBuf },
    #[snafu(display("path is not a Git repository: {}", path.display()))]
    NotGitRepository { path: PathBuf },
    #[snafu(display("repository was provided more than once: {}", path.display()))]
    DuplicateRepository { path: PathBuf },
    #[snafu(display(
        "workspace {} is inside source repository {}",
        workspace.display(),
        repository.display()
    ))]
    WorkspaceInsideRepository {
        workspace: PathBuf,
        repository: PathBuf,
    },
}

#[derive(Debug, Snafu)]
#[snafu(context(suffix(RootSnafu)))]
pub enum WorkspaceRootError {
    #[snafu(display(
        "workspace and managed root must be absolute: {} / {}",
        workspace.display(),
        managed_root.display()
    ))]
    NotAbsolute {
        workspace: PathBuf,
        managed_root: PathBuf,
    },
    #[snafu(display(
        "workspace {} is outside managed root {}",
        workspace.display(),
        managed_root.display()
    ))]
    OutsideManagedRoot {
        workspace: PathBuf,
        managed_root: PathBuf,
    },
    #[snafu(display("workspace root is not a directory: {}", path.display()))]
    NotDirectory { path: PathBuf },
    #[snafu(display("failed to read workspace root {}: {source}", path.display()))]
    ReadDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("workspace root contains unexpected entry: {}", path.display()))]
    UnexpectedEntry { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-validation-{}", uuid::Uuid::now_v7()))
    }

    fn fake_repository(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(path.join(".git")).expect("fake repository should be created");
        path
    }

    #[test]
    fn validates_and_canonicalizes_create_inputs() {
        let root = test_root();
        fs::create_dir_all(&root).expect("test root should be created");
        let repository = fake_repository(&root, "repo");
        let workspace = root.join("workspace");

        let input = validate_create(&workspace, std::slice::from_ref(&repository))
            .expect("create input should be valid");

        assert!(input.workspace_path.as_path().is_absolute());
        assert!(input.repositories[0].as_path().is_absolute());
        assert!(!workspace.exists());
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rejects_existing_workspace_and_duplicate_repositories() {
        let root = test_root();
        fs::create_dir_all(&root).expect("test root should be created");
        let repository = fake_repository(&root, "repo");
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace should be created");

        assert!(matches!(
            validate_create(&workspace, std::slice::from_ref(&repository)),
            Err(ValidationError::WorkspaceExists { .. })
        ));
        fs::remove_dir(&workspace).expect("workspace should be removable");
        assert!(matches!(
            validate_create(&workspace, &[repository.clone(), repository]),
            Err(ValidationError::DuplicateRepository { .. })
        ));
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn rejects_workspace_inside_source_repository() {
        let root = test_root();
        fs::create_dir_all(&root).expect("test root should be created");
        let repository = fake_repository(&root, "repo");
        let workspace = repository.join("workspace");

        assert!(matches!(
            validate_create(&workspace, std::slice::from_ref(&repository)),
            Err(ValidationError::WorkspaceInsideRepository { .. })
        ));
        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn formats_validation_errors() {
        let path = PathBuf::from("/tmp/repository");
        let errors = [
            ValidationError::NoRepositories,
            ValidationError::WorkspaceExists { path: path.clone() },
            ValidationError::InvalidWorkspacePath { path: path.clone() },
            ValidationError::PathIo {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
            },
            ValidationError::Canonicalize {
                source: CanonicalPathError::NotAbsolute { path: path.clone() },
            },
            ValidationError::NotDirectory { path: path.clone() },
            ValidationError::NotGitRepository { path: path.clone() },
            ValidationError::DuplicateRepository { path: path.clone() },
            ValidationError::WorkspaceInsideRepository {
                workspace: path.clone(),
                repository: path,
            },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }

    #[test]
    fn validates_workspace_root_contents_and_containment() {
        let root = test_root();
        let managed_root = root.join("managed");
        let workspace = managed_root.join("workspace");
        fs::create_dir_all(workspace.join("repo")).expect("workspace should be created");
        let managed_root = fs::canonicalize(&managed_root).expect("managed root should resolve");
        let workspace = fs::canonicalize(&workspace).expect("workspace should resolve");
        let worktree = fs::canonicalize(workspace.join("repo")).expect("worktree should resolve");

        validate_workspace_root(&workspace, &managed_root, std::slice::from_ref(&worktree))
            .expect("workspace root should contain only expected worktrees");
        fs::write(workspace.join("unexpected"), "content\n")
            .expect("unexpected file should be written");
        assert!(matches!(
            validate_workspace_root(&workspace, &managed_root, std::slice::from_ref(&worktree)),
            Err(WorkspaceRootError::UnexpectedEntry { .. })
        ));

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn formats_workspace_root_errors() {
        let path = PathBuf::from("/tmp/workspace");
        let errors = [
            WorkspaceRootError::NotAbsolute {
                workspace: path.clone(),
                managed_root: PathBuf::from("/tmp/managed"),
            },
            WorkspaceRootError::OutsideManagedRoot {
                workspace: path.clone(),
                managed_root: PathBuf::from("/tmp/managed"),
            },
            WorkspaceRootError::NotDirectory { path: path.clone() },
            WorkspaceRootError::ReadDirectory {
                path: path.clone(),
                source: std::io::Error::other("read failed"),
            },
            WorkspaceRootError::UnexpectedEntry { path },
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
            let _ = std::error::Error::source(&error);
        }
    }
}
