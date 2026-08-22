use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::{CanonicalPath, CanonicalPathError};

#[derive(Debug, Clone)]
pub struct ValidatedCreateInput {
    pub workspace_path: CanonicalPath,
    pub repositories: Vec<CanonicalPath>,
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
        return Err(ValidationError::WorkspaceExists(
            workspace_path.into_path_buf(),
        ));
    }

    let mut repositories = Vec::with_capacity(repository_paths.len());
    let mut identities = HashSet::with_capacity(repository_paths.len());
    for repository_path in repository_paths {
        let metadata = fs::metadata(repository_path).map_err(|source| ValidationError::PathIo {
            path: repository_path.clone(),
            source,
        })?;
        if !metadata.is_dir() {
            return Err(ValidationError::NotDirectory(repository_path.clone()));
        }

        let repository =
            CanonicalPath::resolve(repository_path).map_err(ValidationError::Canonicalize)?;
        if !repository.as_path().join(".git").exists() {
            return Err(ValidationError::NotGitRepository(
                repository.into_path_buf(),
            ));
        }
        if workspace_path.as_path().starts_with(repository.as_path()) {
            return Err(ValidationError::WorkspaceInsideRepository {
                workspace: workspace_path.into_path_buf(),
                repository: repository.into_path_buf(),
            });
        }
        if !identities.insert(repository.as_path().to_owned()) {
            return Err(ValidationError::DuplicateRepository(
                repository.into_path_buf(),
            ));
        }

        repositories.push(repository);
    }

    Ok(ValidatedCreateInput {
        workspace_path,
        repositories,
    })
}

pub fn resolve_workspace_path(path: &Path) -> Result<CanonicalPath, ValidationError> {
    if path.exists() {
        return CanonicalPath::resolve(path).map_err(ValidationError::Canonicalize);
    }

    let absolute_path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|source| ValidationError::PathIo {
                path: path.to_owned(),
                source,
            })?
            .join(path)
    };
    let file_name = absolute_path
        .file_name()
        .ok_or_else(|| ValidationError::InvalidWorkspacePath(absolute_path.clone()))?;
    let parent = absolute_path
        .parent()
        .ok_or_else(|| ValidationError::InvalidWorkspacePath(absolute_path.clone()))?;
    let parent = CanonicalPath::resolve(parent).map_err(ValidationError::Canonicalize)?;

    CanonicalPath::from_absolute(parent.as_path().join(file_name))
        .map_err(ValidationError::Canonicalize)
}

#[derive(Debug)]
pub enum ValidationError {
    NoRepositories,
    WorkspaceExists(PathBuf),
    InvalidWorkspacePath(PathBuf),
    PathIo {
        path: PathBuf,
        source: std::io::Error,
    },
    Canonicalize(CanonicalPathError),
    NotDirectory(PathBuf),
    NotGitRepository(PathBuf),
    DuplicateRepository(PathBuf),
    WorkspaceInsideRepository {
        workspace: PathBuf,
        repository: PathBuf,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRepositories => formatter.write_str("at least one repository is required"),
            Self::WorkspaceExists(path) => {
                write!(
                    formatter,
                    "workspace path already exists: {}",
                    path.display()
                )
            }
            Self::InvalidWorkspacePath(path) => {
                write!(formatter, "invalid workspace path: {}", path.display())
            }
            Self::PathIo { path, source } => {
                write!(
                    formatter,
                    "failed to inspect {}: {}",
                    path.display(),
                    source
                )
            }
            Self::Canonicalize(error) => error.fmt(formatter),
            Self::NotDirectory(path) => {
                write!(formatter, "path is not a directory: {}", path.display())
            }
            Self::NotGitRepository(path) => {
                write!(
                    formatter,
                    "path is not a Git repository: {}",
                    path.display()
                )
            }
            Self::DuplicateRepository(path) => {
                write!(
                    formatter,
                    "repository was provided more than once: {}",
                    path.display()
                )
            }
            Self::WorkspaceInsideRepository {
                workspace,
                repository,
            } => write!(
                formatter,
                "workspace {} is inside source repository {}",
                workspace.display(),
                repository.display()
            ),
        }
    }
}

impl std::error::Error for ValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PathIo { source, .. } => Some(source),
            Self::Canonicalize(error) => Some(error),
            _ => None,
        }
    }
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
            Err(ValidationError::WorkspaceExists(_))
        ));
        fs::remove_dir(&workspace).expect("workspace should be removable");
        assert!(matches!(
            validate_create(&workspace, &[repository.clone(), repository]),
            Err(ValidationError::DuplicateRepository(_))
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
            ValidationError::WorkspaceExists(path.clone()),
            ValidationError::InvalidWorkspacePath(path.clone()),
            ValidationError::PathIo {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
            },
            ValidationError::Canonicalize(CanonicalPathError::NotAbsolute { path: path.clone() }),
            ValidationError::NotDirectory(path.clone()),
            ValidationError::NotGitRepository(path.clone()),
            ValidationError::DuplicateRepository(path.clone()),
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
}
