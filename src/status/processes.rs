use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};
use snafu::Snafu;

use crate::domain::{CanonicalPath, Timestamp, WorkspaceId};

#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, Clone)]
pub struct Boundary {
    pub workspace_id: WorkspaceId,
    pub path: CanonicalPath,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Observation {
    pub observed_at: Timestamp,
    pub status: Completeness,
    pub count: Option<usize>,
    pub processes: Vec<Process>,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Process {
    pub pid: u32,
    pub name: Option<String>,
    #[serde(serialize_with = "serialize_path")]
    pub cwd: PathBuf,
}

fn serialize_path<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub affected_count: Option<usize>,
}

// Declaration order is also the serialized issue ordering.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    CurrentUserUnavailable,
    CwdUnreadable,
    EnumerationFailed,
    ProcessRaced,
    UnsupportedPlatform,
    UserUnreadable,
}

impl IssueCode {
    pub fn reason(self) -> &'static str {
        match self {
            Self::CurrentUserUnavailable => "current user could not be identified",
            Self::CwdUnreadable => "some process working directories could not be read",
            Self::EnumerationFailed => "process enumeration failed",
            Self::ProcessRaced => "some process identities changed during observation",
            Self::UnsupportedPlatform => "process observation is not supported on this platform",
            Self::UserUnreadable => "some process users could not be identified",
        }
    }
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
enum ProbeError {
    #[snafu(display("failed to read process metadata: {source}"))]
    Read { code: IssueCode, source: io::Error },
    #[snafu(display("invalid process metadata"))]
    Invalid { code: IssueCode },
    #[snafu(display("process identity changed during observation"))]
    Raced,
}

impl ProbeError {
    fn code(&self) -> IssueCode {
        match self {
            Self::Read { code, .. } | Self::Invalid { code } => *code,
            Self::Raced => IssueCode::ProcessRaced,
        }
    }
}

trait Source {
    fn pids(&self) -> Result<Vec<u32>, ProbeError>;
    fn process(&self, pid: u32, uid: u32) -> Result<Option<Process>, ProbeError>;
}

pub fn observe(target: WorkspaceId, boundaries: &[Boundary]) -> Observation {
    let observed_at = Timestamp::now();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // geteuid has no failure mode and takes no pointers.
        let uid = unsafe { libc::geteuid() };
        #[cfg(target_os = "linux")]
        let source = linux::Procfs::new(PathBuf::from("/proc"));
        #[cfg(target_os = "macos")]
        let source = macos::Native;
        collect(
            &source,
            target,
            boundaries,
            uid,
            &[std::process::id()],
            observed_at,
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (target, boundaries);
        Observation::unavailable(observed_at, IssueCode::UnsupportedPlatform)
    }
}

impl Observation {
    pub fn unavailable(observed_at: Timestamp, code: IssueCode) -> Self {
        Self {
            observed_at,
            status: Completeness::Unavailable,
            count: None,
            processes: Vec::new(),
            issues: vec![Issue {
                code,
                affected_count: None,
            }],
        }
    }
}

fn collect(
    source: &impl Source,
    target: WorkspaceId,
    boundaries: &[Boundary],
    uid: u32,
    excluded: &[u32],
    observed_at: Timestamp,
) -> Observation {
    let mut pids = match source.pids() {
        Ok(pids) => pids,
        Err(_) => return Observation::unavailable(observed_at, IssueCode::EnumerationFailed),
    };
    pids.sort_unstable();
    pids.dedup();
    let mut processes = Vec::new();
    let mut issues = BTreeMap::new();
    for pid in pids {
        if excluded.contains(&pid) {
            continue;
        }
        match source.process(pid, uid) {
            Ok(Some(process)) if owner(&process.cwd, boundaries) == Some(target) => {
                processes.push(process);
            }
            Ok(_) => {}
            Err(error) => *issues.entry(error.code()).or_insert(0) += 1,
        }
    }
    Observation {
        observed_at,
        status: if issues.is_empty() {
            Completeness::Complete
        } else {
            Completeness::Partial
        },
        count: Some(processes.len()),
        processes,
        issues: issues
            .into_iter()
            .map(|(code, count)| Issue {
                code,
                affected_count: Some(count),
            })
            .collect(),
    }
}

fn owner(cwd: &Path, boundaries: &[Boundary]) -> Option<WorkspaceId> {
    boundaries
        .iter()
        .filter(|boundary| cwd.starts_with(boundary.path.as_path()))
        .max_by_key(|boundary| boundary.path.as_path().components().count())
        .map(|boundary| boundary.workspace_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl Source for Fake {
        fn pids(&self) -> Result<Vec<u32>, ProbeError> {
            Ok(vec![4, 3, 2, 1, 2, 5, 6])
        }
        fn process(&self, pid: u32, _: u32) -> Result<Option<Process>, ProbeError> {
            match pid {
                3 | 4 => InvalidSnafu {
                    code: IssueCode::CwdUnreadable,
                }
                .fail(),
                5 => RacedSnafu.fail(),
                6 => Ok(None),
                _ => Ok(Some(Process {
                    pid,
                    name: None,
                    cwd: "/work/api".into(),
                })),
            }
        }
    }

    #[test]
    fn attributes_to_nearest_boundary_by_components() {
        let outer = WorkspaceId::new();
        let inner = WorkspaceId::new();
        let boundaries = vec![
            Boundary {
                workspace_id: outer,
                path: CanonicalPath::from_absolute("/work/api").unwrap(),
            },
            Boundary {
                workspace_id: inner,
                path: CanonicalPath::from_absolute("/work/api/nested").unwrap(),
            },
        ];
        for (path, expected) in [
            ("/work/api", Some(outer)),
            ("/work/api/repo", Some(outer)),
            ("/work/api/nested/repo", Some(inner)),
            ("/work/api-extra", None),
            ("/elsewhere", None),
        ] {
            assert_eq!(owner(Path::new(path), &boundaries), expected);
        }
        let result = collect(&Fake, outer, &boundaries, 1, &[1], Timestamp::now());
        assert_eq!(result.status, Completeness::Partial);
        assert_eq!(result.count, Some(1));
        assert_eq!(result.processes[0].pid, 2);
        assert_eq!(
            result.issues,
            vec![
                Issue {
                    code: IssueCode::CwdUnreadable,
                    affected_count: Some(2)
                },
                Issue {
                    code: IssueCode::ProcessRaced,
                    affected_count: Some(1)
                }
            ]
        );
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["processes"][0]["name"], serde_json::Value::Null);
        assert_eq!(json["status"], "partial");
    }

    struct Empty {
        fail: bool,
    }
    impl Source for Empty {
        fn pids(&self) -> Result<Vec<u32>, ProbeError> {
            if self.fail {
                InvalidSnafu {
                    code: IssueCode::EnumerationFailed,
                }
                .fail()
            } else {
                Ok(Vec::new())
            }
        }
        fn process(&self, _: u32, _: u32) -> Result<Option<Process>, ProbeError> {
            panic!("empty sources must not probe processes")
        }
    }

    #[test]
    fn distinguishes_empty_partial_and_failed_enumeration() {
        let target = WorkspaceId::new();
        let complete = collect(
            &Empty { fail: false },
            target,
            &[],
            1,
            &[],
            Timestamp::now(),
        );
        assert_eq!(complete.status, Completeness::Complete);
        assert_eq!(complete.count, Some(0));
        assert!(complete.issues.is_empty());
        let failed = collect(&Empty { fail: true }, target, &[], 1, &[], Timestamp::now());
        assert_eq!(failed.status, Completeness::Unavailable);
        assert_eq!(failed.count, None);
        let partial = collect(&Fake, target, &[], 1, &[], Timestamp::now());
        assert_eq!(partial.status, Completeness::Partial);
        assert_eq!(partial.count, Some(0));
    }

    #[test]
    fn unavailable_has_no_numeric_count() {
        let json = serde_json::to_value(Observation::unavailable(
            Timestamp::now(),
            IssueCode::EnumerationFailed,
        ))
        .unwrap();
        assert_eq!(json["count"], serde_json::Value::Null);
        assert_eq!(json["processes"], serde_json::json!([]));
        assert_eq!(json["issues"][0]["affected_count"], serde_json::Value::Null);
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod native_tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};

    use super::*;

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn observes_a_synchronized_child_through_a_symlink_and_skips_its_exit() {
        let root = std::env::temp_dir().join(format!("trees-process-{}", WorkspaceId::new()));
        std::fs::create_dir_all(root.join("workspace/repo")).unwrap();
        std::os::unix::fs::symlink(root.join("workspace"), root.join("alias")).unwrap();
        let target = WorkspaceId::new();
        let boundaries = vec![Boundary {
            workspace_id: target,
            path: CanonicalPath::resolve(root.join("workspace")).unwrap(),
        }];
        let mut child = ChildGuard(
            Command::new("sh")
                .args(["-c", "printf 'ready\\n'; read -r line"])
                .current_dir(root.join("alias/repo"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let result = observe(target, &boundaries);
        let process = result
            .processes
            .iter()
            .find(|process| process.pid == child.0.id())
            .unwrap_or_else(|| panic!("child missing from observation: {result:?}"));
        assert_eq!(process.cwd, boundaries[0].path.as_path().join("repo"));
        assert!(process.name.is_some());
        assert!(!result
            .processes
            .iter()
            .any(|process| process.pid == std::process::id()));
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let result = observe(target, &boundaries);
        assert!(!result
            .processes
            .iter()
            .any(|process| process.pid == child.0.id()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
