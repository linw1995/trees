use std::fmt;
use std::str::FromStr;

use diesel::sqlite::SqliteConnection;

use crate::domain::{CanonicalPath, Timestamp, WorkspaceState};
use crate::storage::{find_running_operation, find_workspace_lease, list_automatic_workspaces};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct GcDuration {
    seconds: i64,
}

impl GcDuration {
    pub const fn seconds(self) -> i64 {
        self.seconds
    }
}

impl FromStr for GcDuration {
    type Err = GcDurationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err(GcDurationError::new("duration must not be empty"));
        }

        let mut total = 0_i64;
        let mut index = 0;
        while index < value.len() {
            let number_start = index;
            while value.as_bytes()[index].is_ascii_digit() {
                index += 1;
                if index == value.len() {
                    break;
                }
            }
            if number_start == index || index == value.len() {
                return Err(GcDurationError::new(
                    "duration components require a number and a unit",
                ));
            }
            let number = value[number_start..index]
                .parse::<i64>()
                .map_err(|_| GcDurationError::new("duration number is out of range"))?;
            let unit = value.as_bytes()[index] as char;
            index += 1;
            let multiplier = match unit {
                's' => 1,
                'm' => 60,
                'h' => 60 * 60,
                'd' => 24 * 60 * 60,
                'w' => 7 * 24 * 60 * 60,
                _ => {
                    return Err(GcDurationError::new(
                        "duration units must be s, m, h, d, or w",
                    ));
                }
            };
            total = total
                .checked_add(
                    number
                        .checked_mul(multiplier)
                        .ok_or_else(|| GcDurationError::new("duration is out of range"))?,
                )
                .ok_or_else(|| GcDurationError::new("duration is out of range"))?;
        }
        if total <= 0 {
            return Err(GcDurationError::new("duration must be greater than zero"));
        }
        Ok(Self { seconds: total })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GcDurationError {
    message: String,
}

impl GcDurationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GcDurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GcDurationError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct GcCounts {
    pub automatic: usize,
    pub not_checked_out: usize,
    pub checked_out: usize,
    pub age_eligible: usize,
    pub safe_to_reclaim: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GcCandidateReason {
    Eligible,
    Young,
    CheckedOut,
    ExpiredLease,
    ActiveOperation,
    Unhealthy,
}

impl fmt::Display for GcCandidateReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Eligible => "eligible",
            Self::Young => "young",
            Self::CheckedOut => "checked_out",
            Self::ExpiredLease => "expired_lease",
            Self::ActiveOperation => "active_operation",
            Self::Unhealthy => "unhealthy",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone)]
pub struct GcCandidate {
    pub workspace: crate::storage::WorkspaceRow,
    pub idle_since: Timestamp,
    pub age_eligible: bool,
    pub checked_out: bool,
    pub expired_lease: bool,
    pub active_operation: bool,
    pub database_safe: bool,
}

impl GcCandidate {
    pub fn reason(&self) -> GcCandidateReason {
        if !self.age_eligible {
            GcCandidateReason::Young
        } else if self.checked_out {
            GcCandidateReason::CheckedOut
        } else if self.expired_lease {
            GcCandidateReason::ExpiredLease
        } else if self.active_operation {
            GcCandidateReason::ActiveOperation
        } else if self.workspace.state != WorkspaceState::Ready {
            GcCandidateReason::Unhealthy
        } else {
            GcCandidateReason::Eligible
        }
    }
}

#[derive(Debug, Clone)]
pub struct GcScan {
    pub cutoff: Timestamp,
    pub counts: GcCounts,
    pub candidates: Vec<GcCandidate>,
}

pub fn scan(
    connection: &mut SqliteConnection,
    workspace_root: &CanonicalPath,
    older_than: GcDuration,
) -> Result<GcScan, GcError> {
    let cutoff = Timestamp::before_seconds(older_than.seconds());
    let workspaces =
        list_automatic_workspaces(connection, workspace_root).map_err(GcError::Database)?;
    let mut counts = GcCounts {
        automatic: workspaces.len(),
        ..GcCounts::default()
    };
    let mut candidates = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        let lease = find_workspace_lease(connection, &workspace.id).map_err(GcError::Database)?;
        let checked_out = lease
            .as_ref()
            .is_some_and(|lease| !lease.lease_expires_at.has_expired());
        let expired_lease = lease
            .as_ref()
            .is_some_and(|lease| lease.lease_expires_at.has_expired());
        if checked_out {
            counts.checked_out += 1;
        } else {
            counts.not_checked_out += 1;
        }
        let idle_since = workspace
            .last_checked_in_at
            .clone()
            .unwrap_or_else(|| workspace.created_at.clone());
        let age_eligible = idle_since < cutoff;
        if age_eligible {
            counts.age_eligible += 1;
        }
        let active_operation = find_running_operation(connection, &workspace.id)
            .map_err(GcError::Database)?
            .is_some();
        let database_safe = age_eligible
            && !checked_out
            && !expired_lease
            && !active_operation
            && workspace.state == WorkspaceState::Ready;
        if database_safe {
            counts.safe_to_reclaim += 1;
        }
        candidates.push(GcCandidate {
            workspace,
            idle_since,
            age_eligible,
            checked_out,
            expired_lease,
            active_operation,
            database_safe,
        });
    }
    Ok(GcScan {
        cutoff,
        counts,
        candidates,
    })
}

#[derive(Debug)]
pub enum GcError {
    Database(diesel::result::Error),
}

impl fmt::Display for GcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "GC database operation failed: {error}"),
        }
    }
}

impl std::error::Error for GcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::database;
    use crate::domain::{WorkspaceId, WorkspaceManagementMode};
    use crate::lease::WorkspaceLease;
    use crate::storage::{
        insert_managed_workspace, insert_workspace_lease, NewManagedWorkspace, NewWorkspaceLease,
    };

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("trees-gc-{}", WorkspaceId::new()))
    }

    fn timestamp(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("timestamp should be valid")
    }

    #[test]
    fn parses_compound_gc_durations() {
        assert_eq!(
            "1w2d3h4m5s".parse::<GcDuration>().unwrap().seconds(),
            788_645
        );
        assert!("0s".parse::<GcDuration>().is_err());
        assert!("1x".parse::<GcDuration>().is_err());
    }

    #[test]
    fn scans_idle_and_checked_out_automatic_workspaces_by_root() {
        let root = test_root();
        let database_path = root.join("state.sqlite");
        let workspace_root = CanonicalPath::from_absolute(root.join("managed"))
            .expect("workspace root should be absolute");
        fs::create_dir_all(&root).expect("GC test root should be created");
        let mut connection = database::connect(&database_path).expect("database should open");
        let old = timestamp("2020-01-01T00:00:00Z");
        let young = timestamp("2099-01-01T00:00:00Z");
        let pool_key = Some("[\"/repo/example\"]".to_owned());
        let entries = [
            (WorkspaceState::Ready, old.clone(), None),
            (WorkspaceState::Ready, young, None),
            (WorkspaceState::Ready, old.clone(), Some("active")),
            (WorkspaceState::Ready, old.clone(), Some("expired")),
            (WorkspaceState::Degraded, old.clone(), None),
        ];
        let mut workspace_ids = Vec::new();
        for (state, idle_since, lease_kind) in entries {
            let id = WorkspaceId::new();
            workspace_ids.push((id, lease_kind));
            let workspace_path = CanonicalPath::from_absolute(root.join(id.to_string()))
                .expect("workspace path should be absolute");
            insert_managed_workspace(
                &mut connection,
                &NewManagedWorkspace {
                    id,
                    canonical_path: workspace_path,
                    state,
                    created_at: old.clone(),
                    updated_at: old.clone(),
                    last_reconciled_at: None,
                    management_mode: WorkspaceManagementMode::Automatic,
                    pool_key: pool_key.clone(),
                    workspace_root: Some(workspace_root.clone()),
                    last_checked_in_at: Some(idle_since),
                    reclaimed_at: None,
                },
            )
            .expect("workspace should be inserted");
            if let Some(lease_kind) = lease_kind {
                let mut lease = WorkspaceLease::new(id, lease_kind);
                if lease_kind == "expired" {
                    lease.lease_expires_at = old.clone();
                }
                insert_workspace_lease(&mut connection, &NewWorkspaceLease::from(&lease))
                    .expect("lease should be inserted");
            }
        }
        let manual_id = WorkspaceId::new();
        insert_managed_workspace(
            &mut connection,
            &NewManagedWorkspace {
                id: manual_id,
                canonical_path: CanonicalPath::from_absolute(root.join("manual"))
                    .expect("workspace path should be absolute"),
                state: WorkspaceState::Ready,
                created_at: old.clone(),
                updated_at: old.clone(),
                last_reconciled_at: None,
                management_mode: WorkspaceManagementMode::Manual,
                pool_key: None,
                workspace_root: None,
                last_checked_in_at: Some(old.clone()),
                reclaimed_at: None,
            },
        )
        .expect("manual workspace should be inserted");

        let result = scan(
            &mut connection,
            &workspace_root,
            "30d".parse().expect("duration should parse"),
        )
        .expect("GC scan should succeed");
        assert_eq!(result.counts.automatic, 5);
        assert_eq!(result.counts.not_checked_out, 4);
        assert_eq!(result.counts.checked_out, 1);
        assert_eq!(result.counts.age_eligible, 4);
        assert_eq!(result.counts.safe_to_reclaim, 1);
        assert!(result
            .candidates
            .iter()
            .any(|candidate| candidate.reason() == GcCandidateReason::ExpiredLease));
        assert_eq!(workspace_ids.len(), 5);

        drop(connection);
        fs::remove_file(database_path).expect("state database should be removable");
        fs::remove_dir_all(root).expect("GC test root should be removable");
    }
}
