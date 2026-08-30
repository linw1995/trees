use crate::domain::{
    CheckoutId, OperationState, RepoWorktreeState, Timestamp, WorkspaceId, WorkspaceManagementMode,
    WorkspaceState,
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_LEASE_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceLease {
    pub id: CheckoutId,
    pub workspace_id: WorkspaceId,
    pub owner_id: String,
    pub checked_out_at: Timestamp,
    pub lease_expires_at: Timestamp,
    pub last_heartbeat_at: Timestamp,
}

impl WorkspaceLease {
    pub fn new(workspace_id: WorkspaceId, owner_id: impl Into<String>) -> Self {
        let now = Timestamp::now();
        Self {
            id: CheckoutId::new(),
            workspace_id,
            owner_id: owner_id.into(),
            checked_out_at: now.clone(),
            lease_expires_at: Timestamp::after_seconds(DEFAULT_LEASE_SECONDS),
            last_heartbeat_at: now,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.lease_expires_at.has_expired()
    }

    pub fn renew(&mut self) {
        let now = Timestamp::now();
        self.last_heartbeat_at = now;
        self.lease_expires_at = Timestamp::after_seconds(DEFAULT_LEASE_SECONDS);
    }
}

pub fn can_allocate_workspace(
    mode: WorkspaceManagementMode,
    workspace_state: WorkspaceState,
    worktree_states: &[RepoWorktreeState],
    lease: Option<&WorkspaceLease>,
    operation_state: Option<OperationState>,
) -> bool {
    mode.is_automatic()
        && workspace_state == WorkspaceState::Ready
        && !worktree_states.is_empty()
        && worktree_states
            .iter()
            .all(|state| *state == RepoWorktreeState::Attached)
        && lease.is_none()
        && operation_state.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_renews_a_workspace_lease() {
        let workspace_id = WorkspaceId::new();
        let mut lease = WorkspaceLease::new(workspace_id, "process:1");
        let original_id = lease.id;

        assert_eq!(lease.workspace_id, workspace_id);
        assert_eq!(lease.owner_id, "process:1");
        assert!(!lease.is_expired());

        lease.renew();

        assert_eq!(lease.id, original_id);
        let heartbeat = time::OffsetDateTime::parse(
            lease.last_heartbeat_at.as_str(),
            &time::format_description::well_known::Rfc3339,
        )
        .expect("heartbeat should be an RFC 3339 timestamp");
        let expiry = time::OffsetDateTime::parse(
            lease.lease_expires_at.as_str(),
            &time::format_description::well_known::Rfc3339,
        )
        .expect("expiry should be an RFC 3339 timestamp");
        assert_eq!((expiry - heartbeat).whole_seconds(), DEFAULT_LEASE_SECONDS);
        assert!(Timestamp::parse(lease.last_heartbeat_at.to_string()).is_ok());

        lease.lease_expires_at = Timestamp::parse("2020-01-01T00:00:00Z").unwrap();
        assert!(lease.is_expired());
    }

    #[test]
    fn allocation_requires_automatic_ready_clean_attached_unleased_workspace() {
        let states = [RepoWorktreeState::Attached];

        assert!(can_allocate_workspace(
            WorkspaceManagementMode::Automatic,
            WorkspaceState::Ready,
            &states,
            None,
            None
        ));
        assert!(!can_allocate_workspace(
            WorkspaceManagementMode::Manual,
            WorkspaceState::Ready,
            &states,
            None,
            None
        ));
        assert!(!can_allocate_workspace(
            WorkspaceManagementMode::Automatic,
            WorkspaceState::Degraded,
            &states,
            None,
            None
        ));
        assert!(!can_allocate_workspace(
            WorkspaceManagementMode::Automatic,
            WorkspaceState::Ready,
            &[RepoWorktreeState::Dirty],
            None,
            None
        ));
        assert!(!can_allocate_workspace(
            WorkspaceManagementMode::Automatic,
            WorkspaceState::Ready,
            &states,
            Some(&WorkspaceLease::new(WorkspaceId::new(), "process:test")),
            None
        ));
        assert!(!can_allocate_workspace(
            WorkspaceManagementMode::Automatic,
            WorkspaceState::Ready,
            &states,
            None,
            Some(OperationState::Running)
        ));
    }
}
