use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use diesel::SqliteConnection;
use snafu::{ensure, ResultExt};

use super::*;
use crate::domain::{LeaseId, RepoWorktreeState};
use crate::git;

pub fn absent(path: &Path) -> Result<(), AddError> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(IoSnafu { path }.into_error(source)),
        Ok(_) => UnsafeSnafu { path }.fail(),
    }
}

fn resolve_with_lease(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    inputs: &[PathBuf],
    offline: bool,
) -> Result<Vec<git::RepositoryInfo>, AddError> {
    // Source provisioning owns its own database connection and reservation locks.
    // Renew this workspace's independent lease while waiting for it.
    enum Progress {
        Published(storage::OriginRepositoryRow),
        Finished(Result<Vec<git::RepositoryInfo>, crate::origin::resolve::ResolveError>),
    }
    std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel(1);
        scope.spawn(move || {
            let result =
                crate::origin::resolve::resolve_add_with_progress(inputs, offline, &mut |origin| {
                    let _ = sender.send(Progress::Published(origin.clone()));
                });
            let _ = sender.send(Progress::Finished(result));
        });
        loop {
            persistence::renew(db, lease)?;
            match receiver.recv_timeout(Duration::from_secs(5)) {
                Ok(Progress::Finished(result)) => return Ok(result?),
                Ok(Progress::Published(origin)) => {
                    persistence::event(
                        db,
                        lease,
                        "workspace_add_origin_available",
                        document(&serde_json::json!({
                            "origin_repository_id": origin.id, "source_path": origin.source_path,
                        }))?,
                    )?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return JournalSnafu.fail(),
            }
        }
    })
}

pub fn observe(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    repository: &RepositoryPlan,
    path: &CanonicalPath,
) -> Result<RepoWorktreeState, AddError> {
    ensure!(
        !std::fs::symlink_metadata(path.as_path())
            .context(IoSnafu {
                path: path.as_path()
            })?
            .file_type()
            .is_symlink(),
        UnsafeSnafu {
            path: path.as_path()
        }
    );
    if let Some(expected) = &repository.git_file {
        let file = path.as_path().join(".git");
        let actual = std::fs::read_to_string(&file).context(IoSnafu { path: &file })?;
        ensure!(
            &actual == expected,
            UnsafeSnafu {
                path: path.as_path()
            }
        );
    }
    let identity =
        git::inspect_repository_identity_with_heartbeat(&repository.source_path, || {
            persistence::renew(db, lease)
        })?;
    ensure!(
        identity == repository.repository_identity,
        UnsafeSnafu {
            path: repository.source_path.as_path()
        }
    );
    let worktrees = git::list_worktrees_with_heartbeat(&repository.source_path, || {
        persistence::renew(db, lease)
    })?;
    let found = worktrees
        .iter()
        .find(|tree| tree.path == *path)
        .ok_or_else(|| {
            UnsafeSnafu {
                path: path.as_path(),
            }
            .build()
        })?;
    ensure!(
        found.prunable.is_none() && !found.bare,
        UnsafeSnafu {
            path: path.as_path()
        }
    );
    ensure!(
        git::inspect_worktree_identity_with_heartbeat(path.as_path(), || persistence::renew(
            db, lease
        ))? == identity,
        UnsafeSnafu {
            path: path.as_path()
        }
    );
    if !git::is_worktree_clean_with_heartbeat(path.as_path(), || persistence::renew(db, lease))? {
        return Ok(RepoWorktreeState::Dirty);
    }
    if !found.detached || found.head.as_deref() != Some(&repository.head) {
        return Ok(RepoWorktreeState::Diverged);
    }
    Ok(RepoWorktreeState::Attached)
}

pub fn prepare(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    target: &AddTarget,
    request: &AddRequest,
) -> Result<AddPlan, AddError> {
    ensure!(!request.repositories.is_empty(), EmptySnafu);
    persistence::ensure_resolved(db, &target.workspace.id)?;
    let root = &target.workspace.canonical_path;
    let existing = inspect_existing(db, lease, target)?;
    let selection = select_additions(db, lease, root, &existing, request)?;
    let relocation = plan_relocation(db, lease, root, &existing, &selection.additions)?;
    Ok(AddPlan {
        version: 1,
        workspace_id: target.workspace.id,
        workspace_path: root.clone(),
        claim_id: target.claim_id,
        previous_pool_id: target.workspace.pool_id,
        existing,
        additions: selection.additions,
        requested: selection.requested,
        relocation,
    })
}

fn inspect_existing(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    target: &AddTarget,
) -> Result<Vec<RepositoryPlan>, AddError> {
    let root = &target.workspace.canonical_path;
    ensure!(
        CanonicalPath::resolve(root.as_path())? == *root,
        UnsafeSnafu {
            path: root.as_path()
        }
    );
    let rows = persistence::active_repositories(db, &target.workspace.id)?;
    ensure!(
        !rows.is_empty(),
        UnsafeSnafu {
            path: root.as_path()
        }
    );
    let mut existing = Vec::new();
    for row in rows {
        ensure!(
            !matches!(
                row.state,
                RepoWorktreeState::Pending | RepoWorktreeState::Failed
            ),
            UnsafeSnafu {
                path: row.worktree_path.as_path()
            }
        );
        let repo = RepositoryPlan {
            origin_repository_id: row.origin_repository_id,
            worktree_id: row.id,
            source_path: row.source_path,
            repository_identity: row.repository_identity,
            git_file: Some(
                std::fs::read_to_string(row.worktree_path.as_path().join(".git")).context(
                    IoSnafu {
                        path: row.worktree_path.as_path(),
                    },
                )?,
            ),
            worktree_path: row.worktree_path,
            head: row.last_head.ok_or_else(|| JournalSnafu.build())?,
        };
        observe(db, lease, &repo, &repo.worktree_path)?;
        ensure!(
            repo.worktree_path == *root
                || repo.worktree_path.as_path().parent() == Some(root.as_path()),
            UnsafeSnafu {
                path: repo.worktree_path.as_path()
            }
        );
        existing.push(repo);
    }
    Ok(existing)
}

struct RepositorySelection {
    additions: Vec<RepositoryPlan>,
    requested: Vec<OriginRepositoryId>,
}

fn select_additions(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    root: &CanonicalPath,
    existing: &[RepositoryPlan],
    request: &AddRequest,
) -> Result<RepositorySelection, AddError> {
    let resolved = resolve_with_lease(db, lease, &request.repositories, request.offline)?;
    let mut additions = Vec::new();
    let mut requested = Vec::new();
    for info in resolved {
        if let Some(repo) = existing
            .iter()
            .find(|repo| repo.repository_identity == info.common_dir)
        {
            requested.push(repo.origin_repository_id);
            continue;
        }
        ensure!(
            !root.as_path().starts_with(info.root.as_path())
                && !info.root.as_path().starts_with(root.as_path()),
            UnsafeSnafu {
                path: info.root.as_path()
            }
        );
        let revision = if request.offline {
            git::inspect_upstream_repository(&info.root)?
        } else {
            git::inspect_fetched_upstream_repository_with_heartbeat(&info.root, || {
                persistence::renew(db, lease)
            })?
        };
        ensure!(
            revision.common_dir == info.common_dir,
            UnsafeSnafu {
                path: info.root.as_path()
            }
        );
        let origin = storage::ensure_origin_repository(db, &info.common_dir, &info.root)?;
        eprintln!(
            "Origin available for reuse: {} ({})",
            origin.id, origin.source_path
        );
        persistence::event(
            db,
            lease,
            "workspace_add_origin_available",
            document(&serde_json::json!({
                "origin_repository_id": origin.id, "source_path": origin.source_path,
            }))?,
        )?;
        let name = info.root.as_path().file_name().ok_or_else(|| {
            UnsafeSnafu {
                path: info.root.as_path(),
            }
            .build()
        })?;
        let path = CanonicalPath::from_absolute(root.as_path().join(name))?;
        additions.push(RepositoryPlan {
            origin_repository_id: origin.id,
            worktree_id: RepoWorktreeId::new(),
            source_path: info.root,
            repository_identity: info.common_dir,
            worktree_path: path,
            head: revision.head,
            git_file: None,
        });
        requested.push(origin.id);
    }
    Ok(RepositorySelection {
        additions,
        requested,
    })
}

fn plan_relocation(
    db: &mut SqliteConnection,
    lease: &LeaseId,
    root: &CanonicalPath,
    existing: &[RepositoryPlan],
    additions: &[RepositoryPlan],
) -> Result<Option<Relocation>, AddError> {
    let mut relocation = None;
    if !additions.is_empty() {
        let mut names = HashSet::new();
        for repo in existing {
            let name = if repo.worktree_path == *root {
                ensure!(
                    existing.len() == 1,
                    UnsafeSnafu {
                        path: root.as_path()
                    }
                );
                repo.source_path.as_path().file_name()
            } else {
                repo.worktree_path.as_path().file_name()
            }
            .ok_or_else(|| JournalSnafu.build())?;
            ensure!(
                names.insert(name.to_owned()),
                UnsafeSnafu {
                    path: root.as_path()
                }
            );
            if repo.worktree_path == *root {
                let parent = root
                    .as_path()
                    .parent()
                    .ok_or_else(|| JournalSnafu.build())?;
                let staging = parent.join(format!(".trees-add-{}", lease));
                absent(&staging)?;
                ensure!(
                    git::list_worktrees(&repo.source_path)?
                        .first()
                        .is_some_and(|tree| tree.path != *root),
                    UnsafeSnafu {
                        path: root.as_path()
                    }
                );
                relocation = Some(Relocation {
                    worktree_id: repo.worktree_id,
                    previous_path: root.clone(),
                    worktree_path: CanonicalPath::from_absolute(root.as_path().join(name))?,
                    staging_path: CanonicalPath::from_absolute(staging)?,
                });
            }
        }
        for repo in additions {
            let name = repo
                .worktree_path
                .as_path()
                .file_name()
                .ok_or_else(|| JournalSnafu.build())?;
            ensure!(
                names.insert(name.to_owned()),
                UnsafeSnafu {
                    path: repo.worktree_path.as_path()
                }
            );
            if relocation.is_none() {
                absent(repo.worktree_path.as_path())?;
            }
        }
        for (_, boundary) in storage::repository::list_workspace_boundaries(db)? {
            ensure!(
                boundary == *root || !boundary.as_path().starts_with(root.as_path()),
                UnsafeSnafu {
                    path: boundary.as_path()
                }
            );
        }
    }
    Ok(relocation)
}

use snafu::IntoError;
