use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::Timestamp;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    EntryChanged,
    EntryUnreadable,
    RootMissing,
    RootUnreadable,
    SizeOverflow,
    UnsupportedPlatform,
}

impl IssueCode {
    pub fn reason(self) -> &'static str {
        match self {
            Self::EntryChanged => "some workspace entries changed during observation",
            Self::EntryUnreadable => "some workspace entries could not be read",
            Self::RootMissing => "workspace directory missing",
            Self::RootUnreadable => "workspace directory could not be read",
            Self::SizeOverflow => "workspace disk usage exceeded the supported range",
            Self::UnsupportedPlatform => "disk usage observation is not supported on this platform",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub affected_count: Option<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub struct Observation {
    pub observed_at: Timestamp,
    pub status: Completeness,
    pub allocated_bytes: Option<u64>,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStatus {
    Unknown,
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct CachedObservation {
    pub observed_at: Option<Timestamp>,
    pub status: CacheStatus,
    pub allocated_bytes: Option<u64>,
    pub issues: Vec<Issue>,
}

impl Default for CachedObservation {
    fn default() -> Self {
        Self {
            observed_at: None,
            status: CacheStatus::Unknown,
            allocated_bytes: None,
            issues: Vec::new(),
        }
    }
}

impl From<Option<Observation>> for CachedObservation {
    fn from(value: Option<Observation>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        Self {
            observed_at: Some(value.observed_at),
            status: match value.status {
                Completeness::Complete => CacheStatus::Complete,
                Completeness::Partial => CacheStatus::Partial,
                Completeness::Unavailable => CacheStatus::Unavailable,
            },
            allocated_bytes: value.allocated_bytes,
            issues: value.issues,
        }
    }
}

impl Observation {
    fn unavailable(observed_at: Timestamp, code: IssueCode) -> Self {
        Self {
            observed_at,
            status: Completeness::Unavailable,
            allocated_bytes: None,
            issues: vec![Issue {
                code,
                affected_count: None,
            }],
        }
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit = 0;
    let mut scale = 1_u128;
    while unit + 1 < UNITS.len() && u128::from(bytes) >= scale * 1024 {
        unit += 1;
        scale *= 1024;
    }
    let tenths = (u128::from(bytes) * 10 + scale / 2) / scale;
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

pub fn summary_cell(observation: &CachedObservation) -> String {
    let reasons = observation
        .issues
        .iter()
        .map(|issue| issue.code)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(IssueCode::reason)
        .collect::<Vec<_>>()
        .join("; ");
    match observation.status {
        CacheStatus::Unknown => "unknown".to_owned(),
        CacheStatus::Complete => format_bytes(observation.allocated_bytes.unwrap_or_default()),
        CacheStatus::Partial => format!(
            "{} (partial: {reasons})",
            format_bytes(observation.allocated_bytes.unwrap_or_default())
        ),
        CacheStatus::Unavailable => format!("unavailable ({reasons})"),
    }
}

pub fn inventory_cell(observation: &CachedObservation) -> String {
    match observation.status {
        CacheStatus::Unknown => "unknown".to_owned(),
        CacheStatus::Complete => format_bytes(observation.allocated_bytes.unwrap_or_default()),
        CacheStatus::Partial => format!(
            "{} (partial)",
            format_bytes(observation.allocated_bytes.unwrap_or_default())
        ),
        CacheStatus::Unavailable => "unavailable".to_owned(),
    }
}

#[derive(Default)]
struct Accumulator {
    allocated_bytes: u64,
    seen: HashSet<(u64, u64)>,
    issues: BTreeMap<IssueCode, usize>,
}

impl Accumulator {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn add(&mut self, metadata: &libc::stat) -> Result<bool, IssueCode> {
        let identity = identity(metadata);
        let track_identity =
            metadata.st_nlink > 1 || metadata.st_mode & libc::S_IFMT == libc::S_IFDIR;
        if track_identity && self.seen.contains(&identity) {
            return Ok(false);
        }
        let blocks = u64::try_from(metadata.st_blocks).map_err(|_| IssueCode::SizeOverflow)?;
        let bytes = blocks.checked_mul(512).ok_or(IssueCode::SizeOverflow)?;
        self.allocated_bytes = self
            .allocated_bytes
            .checked_add(bytes)
            .ok_or(IssueCode::SizeOverflow)?;
        if track_identity {
            self.seen.insert(identity);
        }
        Ok(true)
    }

    fn issue(&mut self, code: IssueCode) {
        *self.issues.entry(code).or_default() += 1;
    }

    fn finish(self, observed_at: Timestamp) -> Observation {
        let status = if self.issues.is_empty() {
            Completeness::Complete
        } else {
            Completeness::Partial
        };
        Observation {
            observed_at,
            status,
            allocated_bytes: Some(self.allocated_bytes),
            issues: self
                .issues
                .into_iter()
                .map(|(code, affected_count)| Issue {
                    code,
                    affected_count: Some(affected_count),
                })
                .collect(),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn identity(metadata: &libc::stat) -> (u64, u64) {
    #[cfg(target_os = "linux")]
    let device = metadata.st_dev;
    #[cfg(target_os = "macos")]
    let device = u64::from(metadata.st_dev as u32);
    (device, metadata.st_ino)
}

pub fn observe(root: &Path) -> Observation {
    let observed_at = Timestamp::now();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::scan(root, observed_at)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = root;
        Observation::unavailable(observed_at, IssueCode::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use std::ffi::{CStr, CString};
    use std::io;
    use std::mem::MaybeUninit;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::OsStrExt;

    use super::*;

    struct Directory(*mut libc::DIR);

    impl Directory {
        fn from_fd(fd: OwnedFd) -> io::Result<Self> {
            // fdopendir takes ownership only on success.
            let directory = unsafe { libc::fdopendir(fd.as_raw_fd()) };
            if directory.is_null() {
                return Err(io::Error::last_os_error());
            }
            let _ = fd.into_raw_fd();
            Ok(Self(directory))
        }

        fn fd(&self) -> RawFd {
            unsafe { libc::dirfd(self.0) }
        }

        fn next(&mut self) -> io::Result<Option<CString>> {
            unsafe {
                *errno_location() = 0;
                let entry = libc::readdir(self.0);
                if entry.is_null() {
                    let errno = *errno_location();
                    return if errno == 0 {
                        Ok(None)
                    } else {
                        Err(io::Error::from_raw_os_error(errno))
                    };
                }
                Ok(Some(CStr::from_ptr((*entry).d_name.as_ptr()).to_owned()))
            }
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe { libc::closedir(self.0) };
        }
    }

    #[cfg(target_os = "linux")]
    unsafe fn errno_location() -> *mut libc::c_int {
        libc::__errno_location()
    }

    #[cfg(target_os = "macos")]
    unsafe fn errno_location() -> *mut libc::c_int {
        libc::__error()
    }

    fn open_directory(path: &CStr) -> io::Result<OwnedFd> {
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        }
    }

    fn open_child(parent: RawFd, name: &CStr) -> io::Result<OwnedFd> {
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        }
    }

    fn stat_fd(fd: RawFd) -> io::Result<libc::stat> {
        let mut metadata = MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd, metadata.as_mut_ptr()) } < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { metadata.assume_init() })
        }
    }

    fn stat_child(parent: RawFd, name: &CStr) -> io::Result<libc::stat> {
        let mut metadata = MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                parent,
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } < 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { metadata.assume_init() })
        }
    }

    fn is_directory(metadata: &libc::stat) -> bool {
        metadata.st_mode & libc::S_IFMT == libc::S_IFDIR
    }

    fn changed(error: &io::Error) -> bool {
        matches!(
            error.raw_os_error(),
            Some(libc::ENOENT | libc::ELOOP | libc::ENOTDIR)
        )
    }

    fn entry_issue(error: &io::Error) -> IssueCode {
        if changed(error) {
            IssueCode::EntryChanged
        } else {
            IssueCode::EntryUnreadable
        }
    }

    pub(super) fn scan(root: &Path, observed_at: Timestamp) -> Observation {
        scan_with_hook(root, observed_at, |_| {})
    }

    fn scan_with_hook(
        root: &Path,
        observed_at: Timestamp,
        mut before_child_open: impl FnMut(&CStr),
    ) -> Observation {
        let Ok(path) = CString::new(root.as_os_str().as_bytes()) else {
            return Observation::unavailable(observed_at, IssueCode::RootUnreadable);
        };
        let root_fd = match open_directory(&path) {
            Ok(fd) => fd,
            Err(error) => {
                let code = if error.kind() == io::ErrorKind::NotFound {
                    IssueCode::RootMissing
                } else {
                    IssueCode::RootUnreadable
                };
                return Observation::unavailable(observed_at, code);
            }
        };
        let root_metadata = match stat_fd(root_fd.as_raw_fd()) {
            Ok(metadata) => metadata,
            Err(_) => return Observation::unavailable(observed_at, IssueCode::RootUnreadable),
        };
        let mut accumulator = Accumulator::default();
        if accumulator.add(&root_metadata).is_err() {
            return Observation::unavailable(observed_at, IssueCode::SizeOverflow);
        }
        let root_stream = match Directory::from_fd(root_fd) {
            Ok(stream) => stream,
            Err(_) => return Observation::unavailable(observed_at, IssueCode::RootUnreadable),
        };
        let mut stack = vec![root_stream];
        while !stack.is_empty() {
            let (parent, next) = {
                let directory = stack.last_mut().expect("stack is nonempty");
                (directory.fd(), directory.next())
            };
            let name = match next {
                Ok(Some(name)) => name,
                Ok(None) => {
                    stack.pop();
                    continue;
                }
                Err(_) => {
                    accumulator.issue(IssueCode::EntryUnreadable);
                    stack.pop();
                    continue;
                }
            };
            if name.as_bytes() == b"." || name.as_bytes() == b".." {
                continue;
            }
            let metadata = match stat_child(parent, &name) {
                Ok(metadata) => metadata,
                Err(error) => {
                    accumulator.issue(entry_issue(&error));
                    continue;
                }
            };
            match accumulator.add(&metadata) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(_) => return Observation::unavailable(observed_at, IssueCode::SizeOverflow),
            }
            if !is_directory(&metadata) {
                continue;
            }
            before_child_open(&name);
            let child_fd = match open_child(parent, &name) {
                Ok(fd) => fd,
                Err(error) => {
                    accumulator.issue(entry_issue(&error));
                    continue;
                }
            };
            match stat_fd(child_fd.as_raw_fd()) {
                Ok(opened) if identity(&opened) == identity(&metadata) => {}
                Ok(_) => {
                    accumulator.issue(IssueCode::EntryChanged);
                    continue;
                }
                Err(_) => {
                    accumulator.issue(IssueCode::EntryUnreadable);
                    continue;
                }
            }
            match Directory::from_fd(child_fd) {
                Ok(stream) => stack.push(stream),
                Err(_) => accumulator.issue(IssueCode::EntryUnreadable),
            }
        }
        accumulator.finish(observed_at)
    }

    #[cfg(test)]
    mod tests {
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        use super::*;
        use crate::domain::WorkspaceId;

        fn fixture() -> std::path::PathBuf {
            let root =
                std::env::temp_dir().join(format!("trees-disk-usage-{}", WorkspaceId::new()));
            fs::create_dir_all(&root).unwrap();
            root
        }

        #[test]
        fn counts_allocated_bytes_once_per_inode_without_following_links() {
            let root = fixture();
            let outside = fixture();
            fs::write(root.join(".hidden"), vec![1; 8192]).unwrap();
            fs::hard_link(root.join(".hidden"), root.join("linked")).unwrap();
            fs::create_dir(root.join("child")).unwrap();
            let sparse = fs::File::create(root.join("child/sparse")).unwrap();
            sparse.set_len(8 * 1024 * 1024).unwrap();
            fs::write(outside.join("outside"), vec![2; 8192]).unwrap();
            std::os::unix::fs::symlink(&outside, root.join("external")).unwrap();
            let expected = [
                root.clone(),
                root.join(".hidden"),
                root.join("child"),
                root.join("child/sparse"),
                root.join("external"),
            ]
            .iter()
            .map(|path| fs::symlink_metadata(path).unwrap().blocks() * 512)
            .sum();
            let observation = scan(&root, Timestamp::now());
            assert_eq!(observation.status, Completeness::Complete);
            assert_eq!(observation.allocated_bytes, Some(expected));
            assert!(observation.issues.is_empty());
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
        }

        #[test]
        fn replacement_symlink_cannot_redirect_directory_scan() {
            let root = fixture();
            let outside = fixture();
            fs::create_dir(root.join("child")).unwrap();
            fs::write(outside.join("secret"), vec![1; 1024 * 1024]).unwrap();
            let observation = scan_with_hook(&root, Timestamp::now(), |name| {
                if name.to_bytes() == b"child" {
                    fs::rename(root.join("child"), root.join("old-child")).unwrap();
                    std::os::unix::fs::symlink(&outside, root.join("child")).unwrap();
                }
            });
            assert_eq!(observation.status, Completeness::Partial);
            assert_eq!(observation.issues[0].code, IssueCode::EntryChanged);
            assert!(observation.allocated_bytes.unwrap() < 1024 * 1024);
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
        }

        #[test]
        fn classifies_entry_errors_and_rejects_overflow() {
            assert_eq!(
                entry_issue(&io::Error::from_raw_os_error(libc::ENOENT)),
                IssueCode::EntryChanged
            );
            assert_eq!(
                entry_issue(&io::Error::from_raw_os_error(libc::EACCES)),
                IssueCode::EntryUnreadable
            );
            let mut metadata = unsafe { std::mem::zeroed::<libc::stat>() };
            metadata.st_blocks = i64::MAX;
            assert_eq!(
                Accumulator::default().add(&metadata),
                Err(IssueCode::SizeOverflow)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_all_completeness_states_and_ordered_issues() {
        let instant = Timestamp::parse("2026-09-28T00:00:00Z").unwrap();
        let complete = Accumulator::default().finish(instant.clone());
        let json = serde_json::to_value(complete).unwrap();
        assert_eq!(json["status"], "complete");
        assert_eq!(json["allocated_bytes"], 0);
        assert_eq!(json["issues"], serde_json::json!([]));

        let mut partial = Accumulator {
            allocated_bytes: 512,
            ..Accumulator::default()
        };
        partial.issue(IssueCode::EntryUnreadable);
        partial.issue(IssueCode::EntryChanged);
        partial.issue(IssueCode::EntryUnreadable);
        let json = serde_json::to_value(partial.finish(instant.clone())).unwrap();
        assert_eq!(json["status"], "partial");
        assert_eq!(json["allocated_bytes"], 512);
        assert_eq!(json["issues"][0]["code"], "entry_changed");
        assert_eq!(json["issues"][1]["affected_count"], 2);

        let json = serde_json::to_value(Observation::unavailable(instant, IssueCode::RootMissing))
            .unwrap();
        assert_eq!(json["status"], "unavailable");
        assert!(json["allocated_bytes"].is_null());
        assert_eq!(json["issues"][0]["affected_count"], serde_json::Value::Null);
    }

    #[test]
    fn renders_exact_binary_units_and_compact_incomplete_cells() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GiB");
        assert_eq!(format_bytes(u64::MAX), "16.0 EiB");

        let observed_at = Timestamp::now();
        let partial = Observation {
            observed_at: observed_at.clone(),
            status: Completeness::Partial,
            allocated_bytes: Some(1536),
            issues: vec![
                Issue {
                    code: IssueCode::EntryUnreadable,
                    affected_count: Some(1),
                },
                Issue {
                    code: IssueCode::EntryChanged,
                    affected_count: Some(1),
                },
                Issue {
                    code: IssueCode::EntryChanged,
                    affected_count: Some(1),
                },
            ],
        };
        let partial = CachedObservation::from(Some(partial));
        assert_eq!(inventory_cell(&partial), "1.5 KiB (partial)");
        assert_eq!(
            summary_cell(&partial),
            "1.5 KiB (partial: some workspace entries changed during observation; some workspace entries could not be read)"
        );
        let unavailable = CachedObservation::from(Some(Observation::unavailable(
            observed_at,
            IssueCode::RootMissing,
        )));
        assert_eq!(inventory_cell(&unavailable), "unavailable");
        assert_eq!(
            summary_cell(&unavailable),
            "unavailable (workspace directory missing)"
        );
        let unknown = CachedObservation::default();
        assert_eq!(summary_cell(&unknown), "unknown");
        assert_eq!(inventory_cell(&unknown), "unknown");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn missing_root_is_unavailable() {
        let root = std::env::temp_dir().join(format!(
            "trees-missing-{}",
            crate::domain::WorkspaceId::new()
        ));
        let observation = observe(&root);
        assert_eq!(observation.status, Completeness::Unavailable);
        assert_eq!(observation.issues[0].code, IssueCode::RootMissing);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn non_directory_root_is_unavailable() {
        let root =
            std::env::temp_dir().join(format!("trees-file-{}", crate::domain::WorkspaceId::new()));
        std::fs::write(&root, b"data").unwrap();
        let observation = observe(&root);
        assert_eq!(observation.status, Completeness::Unavailable);
        assert_eq!(observation.issues[0].code, IssueCode::RootUnreadable);
        std::fs::remove_file(root).unwrap();
    }
}
