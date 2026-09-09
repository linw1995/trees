use std::fs;
use std::path::PathBuf;

use diesel::SqliteConnection;
use snafu::{ensure, ResultExt, Snafu};

use super::provision::OWNER_FILE;
use super::reservation::CloneReservation;
use crate::domain::{CanonicalPath, OriginRepositoryId};
use crate::storage::origin;

pub fn clean_partial(
    connection: &mut SqliteConnection,
    reservation: &CloneReservation,
) -> Result<(), RecoveryError> {
    let pending = &reservation.pending;
    if origin::find(connection, pending.id)
        .context(StorageSnafu)?
        .is_some()
    {
        return Ok(());
    }
    let root = pending.managed_root.as_path();
    let container = root.join(pending.id.to_string());
    let metadata = match fs::symlink_metadata(&container) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(RecoveryError::Io {
                path: container,
                source,
            })
        }
    };
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        OwnershipSnafu {
            id: pending.id,
            path: &container
        }
    );
    ensure!(
        CanonicalPath::resolve(root)? == pending.managed_root,
        OwnershipSnafu {
            id: pending.id,
            path: &container
        }
    );
    ensure!(
        pending.source_path.as_path().parent() == Some(container.as_path()),
        OwnershipSnafu {
            id: pending.id,
            path: &container
        }
    );
    let marker = container.join(OWNER_FILE);
    let marker_metadata = fs::symlink_metadata(&marker).context(IoSnafu { path: &marker })?;
    ensure!(
        marker_metadata.is_file() && !marker_metadata.file_type().is_symlink(),
        OwnershipSnafu {
            id: pending.id,
            path: &container
        }
    );
    let token = fs::read_to_string(&marker).context(IoSnafu { path: &marker })?;
    ensure!(
        token == pending.ownership_token,
        OwnershipSnafu {
            id: pending.id,
            path: &container
        }
    );
    fs::remove_dir_all(&container).context(IoSnafu { path: &container })?;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RecoveryError {
    #[snafu(display("cannot prove ownership for origin operation {id} at {}; pending recovery retained", path.display()))]
    Ownership {
        id: OriginRepositoryId,
        path: PathBuf,
    },
    #[snafu(display("origin recovery failed at {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("origin recovery storage failed: {source}"))]
    Storage { source: diesel::result::Error },
    #[snafu(transparent)]
    Path {
        source: crate::domain::CanonicalPathError,
    },
}
