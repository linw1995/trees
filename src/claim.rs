use crate::domain::{ClaimId, Timestamp, WorkspaceId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceClaim {
    pub id: ClaimId,
    pub workspace_id: WorkspaceId,
    pub claimed_at: Timestamp,
}

impl WorkspaceClaim {
    /// Creates the persistent usage marker for an automatic workspace.
    pub fn new(workspace_id: WorkspaceId) -> Self {
        let now = Timestamp::now();
        Self {
            id: ClaimId::new(),
            workspace_id,
            claimed_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_workspace_claim() {
        let workspace_id = WorkspaceId::new();
        let claim = WorkspaceClaim::new(workspace_id);

        assert_eq!(claim.workspace_id, workspace_id);
        assert!(claim.claimed_at.as_str().contains('T'));
    }

    #[test]
    fn claim_identity_and_claim_time_are_stable() {
        let workspace_id = WorkspaceId::new();
        let claim = WorkspaceClaim::new(workspace_id);
        let claim_id = claim.id;
        let claimed_at = claim.claimed_at.clone();

        assert_eq!(claim.id, claim_id);
        assert_eq!(claim.claimed_at, claimed_at);
    }
}
