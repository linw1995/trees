use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use diesel::SqliteConnection;
use snafu::{ensure, IntoError, ResultExt, Snafu};

use super::reservation::{self, CloneReservation, Reservation};
use crate::domain::{CanonicalPath, OriginRepositoryId};
use crate::storage::{origin, OriginRepositoryRow};

pub const OWNER_FILE: &str = ".trees-origin-owner";

pub fn provision(
    connection: &mut SqliteConnection,
    url: &str,
    root: &Path,
    locks: &Path,
) -> Result<OriginRepositoryRow, ProvisionError> {
    let input = super::input::RepositoryInput::parse(Path::new(url))?;
    ensure!(
        matches!(input, super::input::RepositoryInput::Url(_)),
        RemoteInputSnafu
    );
    match reservation::reserve(connection, url, root, locks)? {
        Reservation::Existing(row) => {
            validate_existing(&row)?;
            origin::set_registered(connection, row.id, true).context(StorageSnafu)?;
            Ok(OriginRepositoryRow {
                registered: true,
                ..row
            })
        }
        Reservation::Pending(reservation) => {
            if reservation.abandoned {
                super::recovery::clean_partial(connection, &reservation).context(
                    RecoverySnafu {
                        id: reservation.pending.id,
                    },
                )?;
            }
            match clone_reserved(connection, &reservation) {
                Ok(row) => Ok(row),
                Err(source) => {
                    // A commit can succeed even when its acknowledgement fails.
                    if let Some(row) =
                        origin::find(connection, reservation.pending.id).context(StorageSnafu)?
                    {
                        return Ok(row);
                    }
                    if let Err(cleanup) = super::recovery::clean_partial(connection, &reservation) {
                        return Err(CleanupSnafu {
                            id: reservation.pending.id,
                            cleanup: Box::new(cleanup),
                        }
                        .into_error(Box::new(source)));
                    }
                    origin::delete_pending(connection, reservation.pending.id)
                        .context(StorageSnafu)?;
                    Err(source)
                }
            }
        }
    }
}

pub fn validate_existing(row: &OriginRepositoryRow) -> Result<(), ProvisionError> {
    let info = crate::git::inspect_repository(&row.source_path).context(InspectSnafu {
        id: row.id,
        path: row.source_path.as_path(),
    })?;
    ensure!(
        info.common_dir == row.repository_identity && info.root == row.source_path,
        IdentitySnafu {
            id: row.id,
            path: row.source_path.as_path()
        }
    );
    Ok(())
}

pub(crate) fn clone_reserved(
    connection: &mut SqliteConnection,
    reservation: &CloneReservation,
) -> Result<OriginRepositoryRow, ProvisionError> {
    let pending = &reservation.pending;
    let container = pending.managed_root.as_path().join(pending.id.to_string());
    ensure!(
        CanonicalPath::resolve(pending.managed_root.as_path())? == pending.managed_root
            && pending.source_path.as_path().parent() == Some(container.as_path()),
        IdentitySnafu {
            id: pending.id,
            path: &container
        }
    );
    fs::create_dir(&container).context(IoSnafu { path: &container })?;
    let marker = container.join(OWNER_FILE);
    let mut owner = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .context(IoSnafu { path: &marker })?;
    owner
        .write_all(pending.ownership_token.as_bytes())
        .context(IoSnafu { path: &marker })?;
    owner.sync_all().context(IoSnafu { path: &marker })?;
    let status = Command::new("git")
        .arg("clone")
        .arg("--")
        .arg(&pending.remote_url)
        .arg(pending.source_path.as_path())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .context(LaunchSnafu)?;
    ensure!(
        status.success(),
        CloneFailedSnafu {
            id: pending.id,
            status
        }
    );
    let source = CanonicalPath::resolve(pending.source_path.as_path())?;
    let info = crate::git::inspect_repository(&source).context(InspectSnafu {
        id: pending.id,
        path: source.as_path(),
    })?;
    ensure!(
        info.root == pending.source_path,
        IdentitySnafu {
            id: pending.id,
            path: source.as_path()
        }
    );
    origin::publish_clone(connection, pending, &info.common_dir).context(StorageSnafu)
}

#[derive(Debug, Snafu)]
pub enum ProvisionError {
    #[snafu(transparent)]
    Input { source: super::input::InputError },
    #[snafu(display("automatic repository provisioning requires a remote URL"))]
    RemoteInput,
    #[snafu(transparent)]
    Reservation {
        source: reservation::ReservationError,
    },
    #[snafu(transparent)]
    Path {
        source: crate::domain::CanonicalPathError,
    },
    #[snafu(display("origin storage operation failed: {source}"))]
    Storage { source: diesel::result::Error },
    #[snafu(display("origin filesystem operation failed for {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to start Git clone: {source}"))]
    Launch { source: std::io::Error },
    #[snafu(display("Git clone failed for origin {id}: {status}"))]
    CloneFailed {
        id: OriginRepositoryId,
        status: ExitStatus,
    },
    #[snafu(display("cannot inspect origin {id} at {}: {source}", path.display()))]
    Inspect {
        id: OriginRepositoryId,
        path: PathBuf,
        source: crate::git::GitError,
    },
    #[snafu(display("origin {id} identity mismatch at {}", path.display()))]
    Identity {
        id: OriginRepositoryId,
        path: PathBuf,
    },
    #[snafu(display("origin operation {id} recovery failed: {source}"))]
    Recovery {
        id: OriginRepositoryId,
        source: super::recovery::RecoveryError,
    },
    #[snafu(display("origin operation {id} failed: {source}; cleanup failed: {cleanup}"))]
    Cleanup {
        id: OriginRepositoryId,
        source: Box<ProvisionError>,
        cleanup: Box<super::recovery::RecoveryError>,
    },
}
