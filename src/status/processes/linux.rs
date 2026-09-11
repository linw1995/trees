use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use snafu::{ensure, OptionExt, ResultExt};

use super::{InvalidSnafu, IssueCode, ProbeError, Process, RacedSnafu, ReadSnafu, Source};

pub(super) struct Procfs {
    root: PathBuf,
}

impl Procfs {
    pub(super) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn identity(&self, pid: u32) -> Result<Identity, ProbeError> {
        let data = fs::read(self.root.join(pid.to_string()).join("stat")).context(ReadSnafu {
            code: IssueCode::UserUnreadable,
        })?;
        parse_stat(&data)
    }

    fn read_process(&self, pid: u32, uid: u32) -> Result<Option<Process>, ProbeError> {
        let before = self.identity(pid)?;
        if before.exited {
            return Ok(None);
        }
        let root = self.root.join(pid.to_string());
        let status = fs::read(root.join("status")).context(ReadSnafu {
            code: IssueCode::UserUnreadable,
        })?;
        if effective_uid(&status)? != uid {
            return Ok(None);
        }
        let cwd = read_cwd(&root.join("cwd"));
        // Check identity after the cwd read, including failed reads, to distinguish exits.
        let after = self.identity(pid)?;
        if after.exited {
            return Ok(None);
        }
        ensure!(before.start == after.start, RacedSnafu);
        let after_status = fs::read(root.join("status")).context(ReadSnafu {
            code: IssueCode::UserUnreadable,
        })?;
        ensure!(effective_uid(&after_status)? == uid, RacedSnafu);
        Ok(Some(Process {
            pid,
            name: before.name,
            cwd: cwd?,
        }))
    }
}

impl Source for Procfs {
    fn pids(&self) -> Result<Vec<u32>, ProbeError> {
        let entries = fs::read_dir(&self.root).context(ReadSnafu {
            code: IssueCode::EnumerationFailed,
        })?;
        let mut pids = Vec::new();
        for entry in entries {
            let entry = entry.context(ReadSnafu {
                code: IssueCode::EnumerationFailed,
            })?;
            if let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok())
            {
                pids.push(pid);
            }
        }
        Ok(pids)
    }

    fn process(&self, pid: u32, uid: u32) -> Result<Option<Process>, ProbeError> {
        match self.read_process(pid, uid) {
            // Only a missing process directory confirms an exit; a missing cwd does not.
            Err(error) => match fs::metadata(self.root.join(pid.to_string())) {
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(error),
            },
            result => result,
        }
    }
}

fn read_cwd(link: &Path) -> Result<PathBuf, ProbeError> {
    let path = fs::read_link(link).context(ReadSnafu {
        code: IssueCode::CwdUnreadable,
    })?;
    let metadata = fs::metadata(link).context(ReadSnafu {
        code: IssueCode::CwdUnreadable,
    })?;
    super::physical_cwd(&path, metadata.dev(), metadata.ino())
}

struct Identity {
    start: u64,
    exited: bool,
    name: Option<String>,
}

fn parse_stat(data: &[u8]) -> Result<Identity, ProbeError> {
    let invalid = || InvalidSnafu {
        code: IssueCode::UserUnreadable,
    };
    let begin = data
        .iter()
        .position(|&byte| byte == b'(')
        .context(invalid())?;
    let end = data
        .iter()
        .rposition(|&byte| byte == b')')
        .context(invalid())?;
    ensure!(begin < end, invalid());
    let fields: Vec<_> = data[end + 1..]
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|field| !field.is_empty())
        .collect();
    // Field 22 is the start time; fields after the command begin with field 3.
    let start = fields
        .get(19)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|text| text.parse().ok())
        .context(invalid())?;
    let name = String::from_utf8_lossy(&data[begin + 1..end]).into_owned();
    Ok(Identity {
        start,
        exited: matches!(fields.first(), Some(&b"Z" | &b"X")),
        name: (!name.is_empty()).then_some(name),
    })
}

fn effective_uid(data: &[u8]) -> Result<u32, ProbeError> {
    data.split(|&byte| byte == b'\n')
        .find_map(|line| {
            line.strip_prefix(b"Uid:").and_then(|values| {
                values
                    .split(|byte| byte.is_ascii_whitespace())
                    .filter(|field| !field.is_empty())
                    .nth(1)
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(|text| text.parse().ok())
            })
        })
        .context(InvalidSnafu {
            code: IssueCode::UserUnreadable,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::WorkspaceId;

    #[test]
    fn parses_names_with_delimiters_and_effective_uid() {
        let stat = format!("12 (a ) b\n) S {} 987 0", vec!["0"; 18].join(" "));
        let identity = parse_stat(stat.as_bytes()).unwrap();
        assert_eq!(identity.start, 987);
        assert_eq!(identity.name.as_deref(), Some("a ) b\n"));
        assert_eq!(effective_uid(b"Name:\tx\nUid:\t1\t2\t3\t4\n").unwrap(), 2);
        assert!(parse_stat(b"broken").is_err());
        assert!(effective_uid(b"Uid: invalid").is_err());
    }

    #[test]
    fn procfs_distinguishes_other_users_missing_cwd_and_exits() {
        let root = std::env::temp_dir().join(format!("trees-procfs-{}", WorkspaceId::new()));
        fs::create_dir_all(root.join("12")).unwrap();
        fs::write(
            root.join("12/stat"),
            format!("12 (shell) S {} 987", vec!["0"; 18].join(" ")),
        )
        .unwrap();
        fs::write(root.join("12/status"), b"Uid:\t1\t2\t3\t4\n").unwrap();
        let source = Procfs::new(root.clone());
        assert!(source.process(12, 1).unwrap().is_none());
        assert_eq!(
            source.process(12, 2).unwrap_err().code(),
            IssueCode::CwdUnreadable
        );
        assert!(source.process(13, 2).unwrap().is_none());
        assert_eq!(source.pids().unwrap(), vec![12]);
        fs::remove_dir_all(root).unwrap();
    }
}
