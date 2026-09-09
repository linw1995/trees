use std::collections::HashSet;
use std::path::{Path, PathBuf};

use snafu::{ensure, ResultExt, Snafu};

use super::{input::RepositoryInput, provision, reservation};
use crate::{database, git, storage, validation};

pub fn resolve(
    inputs: &[PathBuf],
    offline: bool,
    workspace_path: Option<&Path>,
) -> Result<Vec<git::RepositoryInfo>, ResolveError> {
    let parsed = inputs
        .iter()
        .map(|value| RepositoryInput::parse(value))
        .collect::<Result<Vec<_>, _>>()?;
    let catalog = load_catalog(&parsed)?;
    let mut resolved = Vec::new();
    let mut urls = HashSet::new();
    let mut pending = Vec::new();
    let mut names = HashSet::new();
    for input in parsed {
        let info = match input {
            RepositoryInput::Path(path) => {
                let paths = validation::validate_repositories(&[path])?;
                Some(inspect_primary(&paths[0])?)
            }
            RepositoryInput::Name(name) => {
                let row = storage::origin::resolve_name_in(&catalog, &name)?;
                provision::validate_existing(&row)?;
                Some(inspect_primary(&row.source_path)?)
            }
            RepositoryInput::Url(url) => {
                ensure!(urls.insert(url.clone()), DuplicateSnafu);
                let row = catalog
                    .iter()
                    .find(|row| row.remote_url.as_deref() == Some(url.as_str()));
                if let Some(row) = row {
                    provision::validate_existing(row)?;
                    Some(inspect_primary(&row.source_path)?)
                } else {
                    ensure!(!offline, OfflineSnafu);
                    ensure!(names.insert(reservation::directory_name(&url)), LayoutSnafu);
                    pending.push(url);
                    None
                }
            }
        };
        if let Some(info) = info {
            ensure!(
                names.insert(
                    info.root
                        .as_path()
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                ),
                LayoutSnafu
            );
            resolved.push(info);
        }
    }
    check_identities(&resolved)?;
    if let Some(target) = workspace_path {
        let target = validation::resolve_workspace_path(target)?;
        for info in &resolved {
            ensure!(
                !target.as_path().starts_with(info.root.as_path()),
                NestedWorkspaceSnafu {
                    path: target.as_path()
                }
            );
        }
    }
    if !pending.is_empty() {
        let root = crate::config::origins_directory()?;
        let locks = crate::paths::state_directory()?.join("origin-locks");
        let mut connection = database::open_default()?;
        for url in pending {
            let row = provision::provision(&mut connection, &url, &root, &locks)?;
            eprintln!(
                "Origin available for reuse: {} ({})",
                row.id, row.source_path
            );
            resolved.push(inspect_primary(&row.source_path)?);
        }
    }
    check_identities(&resolved)?;
    Ok(resolved)
}

fn inspect_primary(
    path: &crate::domain::CanonicalPath,
) -> Result<git::RepositoryInfo, git::GitError> {
    let primary = git::inspect_upstream_repository(path)?;
    git::inspect_repository(&primary.root)
}

fn load_catalog(
    inputs: &[RepositoryInput],
) -> Result<Vec<storage::OriginRepositoryRow>, ResolveError> {
    if inputs
        .iter()
        .all(|input| matches!(input, RepositoryInput::Path(_)))
    {
        return Ok(Vec::new());
    }
    match database::open_read_only() {
        Ok(mut connection) => storage::origin::list(&mut connection, true).context(StorageSnafu),
        Err(database::DatabaseError::ReadOnlyDatabaseMissing { .. }) => Ok(Vec::new()),
        Err(source) => Err(source.into()),
    }
}

fn check_identities(repositories: &[git::RepositoryInfo]) -> Result<(), ResolveError> {
    let mut identities = HashSet::new();
    for repository in repositories {
        ensure!(identities.insert(&repository.common_dir), DuplicateSnafu);
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum ResolveError {
    #[snafu(transparent)]
    Input { source: super::input::InputError },
    #[snafu(transparent)]
    Database { source: database::DatabaseError },
    #[snafu(transparent)]
    Git { source: git::GitError },
    #[snafu(transparent)]
    Validation { source: validation::ValidationError },
    #[snafu(transparent)]
    Name { source: storage::origin::NameError },
    #[snafu(transparent)]
    Provision { source: provision::ProvisionError },
    #[snafu(transparent)]
    Configuration { source: crate::config::ConfigError },
    #[snafu(transparent)]
    Path { source: crate::paths::PathError },
    #[snafu(display("failed to resolve repository inputs: {source}"))]
    Storage { source: diesel::result::Error },
    #[snafu(display("workspace target is inside a source repository: {}", path.display()))]
    NestedWorkspace { path: PathBuf },
    #[snafu(display("duplicate repository input"))]
    Duplicate,
    #[snafu(display("repository directory names collide; use distinct source directories"))]
    Layout,
    #[snafu(display("offline creation cannot clone an unknown repository URL"))]
    Offline,
}
