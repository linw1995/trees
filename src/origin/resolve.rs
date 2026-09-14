use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use snafu::{ensure, ResultExt, Snafu};

use super::{input::RepositoryInput, provision, reservation};
use crate::{database, git, storage, validation};

pub fn resolve(
    inputs: &[PathBuf],
    offline: bool,
    workspace_path: Option<&Path>,
) -> Result<Vec<git::RepositoryInfo>, ResolveError> {
    resolve_inputs(inputs, offline, workspace_path, false, &mut |_| {})
}

pub(crate) fn resolve_add_with_progress(
    inputs: &[PathBuf],
    offline: bool,
    published: &mut dyn FnMut(&storage::OriginRepositoryRow),
) -> Result<Vec<git::RepositoryInfo>, ResolveError> {
    resolve_inputs(inputs, offline, None, true, published)
}

struct ResolvedInputs {
    repositories: Vec<git::RepositoryInfo>,
    pending: Vec<(usize, String)>,
    input_order: HashMap<crate::domain::CanonicalPath, usize>,
}

fn resolve_inputs(
    inputs: &[PathBuf],
    offline: bool,
    workspace_path: Option<&Path>,
    deduplicate: bool,
    published: &mut dyn FnMut(&storage::OriginRepositoryRow),
) -> Result<Vec<git::RepositoryInfo>, ResolveError> {
    let parsed = inputs
        .iter()
        .map(|value| RepositoryInput::parse(value))
        .collect::<Result<Vec<_>, _>>()?;
    let catalog = load_catalog(&parsed)?;
    let mut selected = resolve_known_inputs(parsed, &catalog, offline, deduplicate)?;
    validate_target(workspace_path, &selected.repositories)?;
    provision_inputs(&mut selected, published)?;
    let ResolvedInputs {
        mut repositories,
        input_order,
        ..
    } = selected;
    if deduplicate {
        repositories.sort_by_key(|info| input_order[&info.common_dir]);
        let mut seen = HashSet::new();
        repositories.retain(|info| seen.insert(info.common_dir.clone()));
    }
    check_identities(&repositories)?;
    Ok(repositories)
}

fn resolve_known_inputs(
    parsed: Vec<RepositoryInput>,
    catalog: &[storage::OriginRepositoryRow],
    offline: bool,
    deduplicate: bool,
) -> Result<ResolvedInputs, ResolveError> {
    let mut resolved = Vec::new();
    let mut urls = HashSet::new();
    let mut pending = Vec::new();
    let mut names = HashSet::new();
    let mut input_order = HashMap::new();
    for (index, input) in parsed.into_iter().enumerate() {
        let info = match input {
            RepositoryInput::Path(path) => {
                let paths = validation::validate_repositories(&[path])?;
                Some(inspect_primary(&paths[0])?)
            }
            RepositoryInput::Name(name) => {
                let row = storage::origin::resolve_name_in(catalog, &name)?;
                provision::validate_existing(&row)?;
                Some(inspect_primary(&row.source_path)?)
            }
            RepositoryInput::Url(url) => {
                if !urls.insert(url.clone()) {
                    ensure!(deduplicate, DuplicateSnafu);
                    continue;
                }
                let row = super::lookup::find_url(catalog, &url)?;
                if let Some(row) = row {
                    provision::validate_existing(&row)?;
                    Some(inspect_primary(&row.source_path)?)
                } else {
                    ensure!(!offline, OfflineSnafu);
                    ensure!(names.insert(reservation::directory_name(&url)), LayoutSnafu);
                    pending.push((index, url));
                    None
                }
            }
        };
        if let Some(info) = info {
            if deduplicate
                && resolved
                    .iter()
                    .any(|existing: &git::RepositoryInfo| existing.common_dir == info.common_dir)
            {
                continue;
            }
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
            input_order.entry(info.common_dir.clone()).or_insert(index);
            resolved.push(info);
        }
    }
    Ok(ResolvedInputs {
        repositories: resolved,
        pending,
        input_order,
    })
}

fn validate_target(
    workspace_path: Option<&Path>,
    resolved: &[git::RepositoryInfo],
) -> Result<(), ResolveError> {
    check_identities(resolved)?;
    if let Some(target) = workspace_path {
        let target = validation::resolve_workspace_path(target)?;
        for info in resolved {
            ensure!(
                !target.as_path().starts_with(info.root.as_path()),
                NestedWorkspaceSnafu {
                    path: target.as_path()
                }
            );
        }
    }
    Ok(())
}

fn provision_inputs(
    selected: &mut ResolvedInputs,
    published: &mut dyn FnMut(&storage::OriginRepositoryRow),
) -> Result<(), ResolveError> {
    let ResolvedInputs {
        repositories: resolved,
        pending,
        input_order,
    } = selected;
    if !pending.is_empty() {
        let root = crate::config::origins_directory()?;
        let locks = crate::paths::state_directory()?.join("origin-locks");
        let mut connection = database::open_default()?;
        for (index, url) in std::mem::take(pending) {
            let row = provision::provision(&mut connection, &url, &root, &locks)?;
            published(&row);
            eprintln!(
                "Origin available for reuse: {} ({})",
                row.id, row.source_path
            );
            let info = inspect_primary(&row.source_path)?;
            input_order.entry(info.common_dir.clone()).or_insert(index);
            resolved.push(info);
        }
    }
    Ok(())
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
        Ok(mut connection) => storage::origin::list(&mut connection).context(StorageSnafu),
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
    Lookup { source: super::lookup::LookupError },
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
