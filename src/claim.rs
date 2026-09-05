use crate::domain::{ClaimId, Timestamp, WorkspaceId};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CLAIM_TTL_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceClaim {
    pub id: ClaimId,
    pub workspace_id: WorkspaceId,
    pub owner_id: String,
    pub claimed_at: Timestamp,
    pub lease_expires_at: Timestamp,
    pub last_heartbeat_at: Timestamp,
}

impl WorkspaceClaim {
    /// Creates an active claim with a finite recovery window.
    pub fn new(workspace_id: WorkspaceId, owner_id: impl Into<String>) -> Self {
        let now = Timestamp::now();
        Self {
            id: ClaimId::new(),
            workspace_id,
            owner_id: owner_id.into(),
            claimed_at: now.clone(),
            lease_expires_at: Timestamp::after_seconds(DEFAULT_CLAIM_TTL_SECONDS),
            last_heartbeat_at: now,
        }
    }

    /// Extends the claim without keeping a SQLite transaction open.
    pub fn renew(&mut self) {
        let now = Timestamp::now();
        self.last_heartbeat_at = now;
        self.lease_expires_at = Timestamp::after_seconds(DEFAULT_CLAIM_TTL_SECONDS);
    }

    pub fn is_expired(&self) -> bool {
        self.lease_expires_at.has_expired()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_workspace_claim() {
        let workspace_id = WorkspaceId::new();
        let claim = WorkspaceClaim::new(workspace_id, "process:1");

        assert_eq!(claim.workspace_id, workspace_id);
        assert_eq!(claim.owner_id, "process:1");
        assert!(!claim.is_expired());
        assert!(claim.claimed_at.as_str().contains('T'));
    }

    #[test]
    fn renewal_preserves_identity_and_claim_time() {
        let workspace_id = WorkspaceId::new();
        let mut claim = WorkspaceClaim::new(workspace_id, "process:1");
        let claim_id = claim.id;
        let claimed_at = claim.claimed_at.clone();
        let original_expiry = claim.lease_expires_at.clone();

        claim.renew();

        assert_eq!(claim.id, claim_id);
        assert_eq!(claim.claimed_at, claimed_at);
        assert!(claim.lease_expires_at >= original_expiry);
        assert!(!claim.is_expired());
        assert!(claim.last_heartbeat_at >= claim.claimed_at);
    }
}
