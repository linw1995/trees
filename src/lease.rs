use crate::domain::{CheckoutId, Timestamp, WorkspaceId};
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

    pub fn renew(&mut self) {
        let now = Timestamp::now();
        self.last_heartbeat_at = now;
        self.lease_expires_at = Timestamp::after_seconds(DEFAULT_LEASE_SECONDS);
    }
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
        assert!(!lease.lease_expires_at.has_expired());

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
        assert!(lease.lease_expires_at.has_expired());
    }
}
