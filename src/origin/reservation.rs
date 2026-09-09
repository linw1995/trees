use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use diesel::SqliteConnection;
use fs2::FileExt;
use snafu::{ResultExt, Snafu};

use crate::domain::{CanonicalPath, OriginRepositoryId};
use crate::storage::{models::PendingOriginClone, origin, OriginRepositoryRow};

pub enum Reservation {
    Existing(OriginRepositoryRow),
    Pending(CloneReservation),
}

pub struct CloneReservation {
    pub pending: PendingOriginClone,
    pub abandoned: bool,
    _lock: File,
}

pub fn reserve(
    connection: &mut SqliteConnection,
    url: &str,
    root: &Path,
    lock_root: &Path,
) -> Result<Reservation, ReservationError> {
    fs::create_dir_all(lock_root).context(IoSnafu { path: lock_root })?;
    let lock_path = lock_root.join(format!("{}.lock", blake3::hash(url.as_bytes()).to_hex()));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .context(IoSnafu { path: &lock_path })?;
    match lock.try_lock_exclusive() {
        Ok(()) => (),
        Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
            return InProgressSnafu.fail()
        }
        Err(source) => {
            return Err(ReservationError::Io {
                path: lock_path,
                source,
            })
        }
    }
    if let Some(row) = origin::find_by_url(connection, url).context(StorageSnafu)? {
        return Ok(Reservation::Existing(row));
    }
    if let Some(pending) = origin::pending_by_url(connection, url).context(StorageSnafu)? {
        return Ok(Reservation::Pending(CloneReservation {
            pending,
            abandoned: true,
            _lock: lock,
        }));
    }
    fs::create_dir_all(root).context(IoSnafu { path: root })?;
    let root = CanonicalPath::resolve(root)?;
    let id = OriginRepositoryId::new();
    let source = root
        .as_path()
        .join(id.to_string())
        .join(directory_name(url));
    let pending = PendingOriginClone {
        id,
        remote_url: url.to_owned(),
        managed_root: root,
        source_path: CanonicalPath::from_absolute(source)?,
        ownership_token: OriginRepositoryId::new().to_string(),
    };
    origin::insert_pending(connection, &pending).context(StorageSnafu)?;
    Ok(Reservation::Pending(CloneReservation {
        pending,
        abandoned: false,
        _lock: lock,
    }))
}

pub fn directory_name(url: &str) -> String {
    let segment = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default();
    let segment = segment.strip_suffix(".git").unwrap_or(segment);
    let name: String = segment
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name == "." || name == ".." {
        "repo".to_owned()
    } else {
        name
    }
}

#[derive(Debug, Snafu)]
pub enum ReservationError {
    #[snafu(display("repository provisioning is already in progress; retry after it finishes"))]
    InProgress,
    #[snafu(display("origin filesystem operation failed for {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("origin reservation storage failed: {source}"))]
    Storage { source: diesel::result::Error },
    #[snafu(transparent)]
    Path {
        source: crate::domain::CanonicalPathError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_urls_and_retains_abandoned_intent() {
        let base =
            std::env::temp_dir().join(format!("trees-reserve-{}", OriginRepositoryId::new()));
        let mut db = crate::database::connect(Path::new(":memory:")).unwrap();
        let root = base.join("origins");
        let locks = base.join("locks");
        let first = reserve(&mut db, "https://host/api.git", &root, &locks).unwrap();
        assert!(matches!(
            reserve(&mut db, "https://host/api.git", &root, &locks),
            Err(ReservationError::InProgress)
        ));
        let Reservation::Pending(first) = first else {
            panic!("expected pending clone")
        };
        assert_eq!(
            first.pending.source_path.as_path().file_name().unwrap(),
            "api"
        );
        assert!(!first.pending.source_path.as_path().exists());
        let id = first.pending.id;
        drop(first);
        let Reservation::Pending(second) =
            reserve(&mut db, "https://host/api.git", &base.join("other"), &locks).unwrap()
        else {
            panic!("expected pending clone")
        };
        assert!(second.abandoned);
        assert_eq!(second.pending.id, id);
        drop(second);
        fs::remove_dir_all(base).unwrap();
    }
}
