use time::{OffsetDateTime, UtcOffset};

use super::combined::Snapshot;
use super::target::TargetSelector;
use super::{repos, LeaseStatus, WorkspaceStatus};
use crate::domain::Timestamp;

pub fn render(snapshot: &Snapshot, selector: &TargetSelector, color: bool) -> String {
    let (heading, inventory) = match snapshot {
        Snapshot::Pools(snapshot) => ("Pools", super::render_pools_human(snapshot, color)),
        Snapshot::Workspaces(snapshot) => (
            "Workspaces",
            super::render_workspaces_human(snapshot, color),
        ),
        Snapshot::Repos(snapshot) => ("Repositories", repos::render(snapshot)),
    };
    match snapshot.target() {
        Some(target) => format!(
            "{}\n\n{heading}\n{inventory}",
            render_target(target, selector, color)
        ),
        None => inventory,
    }
}

fn render_target(target: &WorkspaceStatus, selector: &TargetSelector, color: bool) -> String {
    render_target_with_offset(target, selector, color, |instant| {
        UtcOffset::local_offset_at(instant).ok()
    })
}

fn render_target_with_offset(
    target: &WorkspaceStatus,
    selector: &TargetSelector,
    color: bool,
    offset_at: impl FnOnce(OffsetDateTime) -> Option<UtcOffset>,
) -> String {
    let heading = match selector {
        TargetSelector::Id(_) => "Workspace (selected by ID)",
        TargetSelector::Directory(_) => "Workspace (current directory)",
    };
    let mut rows = vec![
        ("ID", target.workspace_id.to_string()),
        ("Path", super::escape_human_label(&target.path.to_string())),
        ("Status", super::workspace_status_summary(target)),
        (
            "Mode",
            format!(
                "{} {}",
                target.management_mode,
                super::mode_symbol(target.management_mode)
            ),
        ),
    ];
    if let Some(operation) = &target.current_operation {
        let lease = match operation.lease_status {
            LeaseStatus::Active => "active",
            LeaseStatus::Expired => "expired",
            LeaseStatus::Inconsistent => "inconsistent",
        };
        rows.push((
            "Operation",
            format!(
                "{} / {} (lease {lease})",
                super::escape_human_label(&operation.kind),
                operation.state
            ),
        ));
    }
    rows.push((
        "Repos",
        super::repository_summary(&target.repo_worktrees, color),
    ));
    rows.push((
        "Reconciled",
        reconciliation_time(target.last_reconciled_at.as_ref(), offset_at),
    ));
    let width = rows
        .iter()
        .map(|(label, _)| super::display_width(label))
        .max()
        .unwrap_or(0);
    let mut output = heading.to_owned();
    for (label, value) in rows {
        output.push_str(&format!(
            "\n  {label}{}{value}",
            " ".repeat(width - super::display_width(label) + 2)
        ));
    }
    output
}

fn reconciliation_time(
    timestamp: Option<&Timestamp>,
    offset_at: impl FnOnce(OffsetDateTime) -> Option<UtcOffset>,
) -> String {
    let Some(timestamp) = timestamp else {
        return "never".to_owned();
    };
    let instant = super::parse_utc(timestamp);
    let local = instant.to_offset(offset_at(instant).unwrap_or(UtcOffset::UTC));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.year(),
        u8::from(local.month()),
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::*;
    use crate::status::{ClaimStatus, CurrentOperationStatus, StatusSnapshot};

    fn target() -> WorkspaceStatus {
        WorkspaceStatus {
            workspace_id: WorkspaceId::new(),
            path: CanonicalPath::from_absolute("/work/api").unwrap(),
            management_mode: WorkspaceManagementMode::Automatic,
            state: WorkspaceState::Ready,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
            last_reconciled_at: Some(Timestamp::parse("2026-09-11T06:32:05Z").unwrap()),
            last_released_at: None,
            removed_at: None,
            pool_id: None,
            claim: Some(ClaimStatus {
                claim_id: ClaimId::new(),
                claimed_at: Timestamp::now(),
            }),
            current_operation: None,
            repo_worktrees: vec![],
        }
    }

    #[test]
    fn renders_compact_summary_and_all_operation_leases() {
        let mut target = target();
        let selector = TargetSelector::Id(target.workspace_id);
        let render = |target: &WorkspaceStatus| {
            render_target_with_offset(target, &selector, false, |_| {
                UtcOffset::from_hms(8, 0, 0).ok()
            })
        };
        assert_eq!(render(&target), format!(
            "Workspace (selected by ID)\n  ID          {}\n  Path        /work/api\n  Status      ready 🔒\n  Mode        automatic 🤖\n  Repos       0/0\n  Reconciled  2026-09-11 14:32:05", target.workspace_id));
        target.management_mode = WorkspaceManagementMode::Manual;
        target.claim = None;
        target.last_reconciled_at = None;
        let output = render(&target);
        assert!(output.contains("Status      ready\n  Mode        manual 👤"));
        assert!(output.ends_with("Reconciled  never"));
        assert!(!output.contains("Operation"));
        for (lease_status, label) in [
            (LeaseStatus::Active, "active"),
            (LeaseStatus::Expired, "expired"),
            (LeaseStatus::Inconsistent, "inconsistent"),
        ] {
            target.current_operation = Some(CurrentOperationStatus {
                operation_id: OperationId::new(),
                kind: "release".into(),
                state: OperationState::Running,
                lease_id: LeaseId::new(),
                lease_expires_at: Timestamp::now(),
                lease_status,
            });
            assert!(render(&target).contains(&format!(
                "Operation   release / running (lease {label})\n  Repos"
            )));
        }
    }

    #[test]
    fn documentation_example_matches_the_rendered_summary() {
        let mut target = target();
        target.workspace_id = "01990000-0000-7000-8000-000000000001".parse().unwrap();
        let selector = TargetSelector::Directory(target.path.clone());
        let summary = render_target_with_offset(&target, &selector, false, |_| {
            UtcOffset::from_hms(8, 0, 0).ok()
        });
        let example = format!("{summary}\n\nPools\nNo workspace pools.");
        assert!(include_str!("../../docs/status.md").contains(&example));
    }

    #[test]
    fn converts_the_recorded_instant_with_date_boundary_and_utc_fallback() {
        let time = Timestamp::parse("2025-12-31T23:32:05Z").unwrap();
        let local = reconciliation_time(Some(&time), |instant| {
            assert_eq!(instant.year(), 2025);
            assert_eq!(instant.hour(), 23);
            UtcOffset::from_hms(8, 0, 0).ok()
        });
        assert_eq!(local, "2026-01-01 07:32:05");
        assert_eq!(
            reconciliation_time(Some(&time), |_| None),
            "2025-12-31 23:32:05"
        );
        assert_eq!(
            reconciliation_time(None, |_| panic!("missing timestamp needs no offset")),
            "never"
        );
    }

    #[test]
    fn escapes_persisted_text_and_reuses_repository_colors() {
        let mut target = target();
        target.path = CanonicalPath::from_absolute("/work/line\n\u{1b}[31m").unwrap();
        target.current_operation = Some(CurrentOperationStatus {
            operation_id: OperationId::new(),
            kind: "release\n\u{1b}".into(),
            state: OperationState::Running,
            lease_id: LeaseId::new(),
            lease_expires_at: Timestamp::now(),
            lease_status: LeaseStatus::Expired,
        });
        target.repo_worktrees.push(super::super::tests::repository(
            "/origin/api",
            RepoWorktreeState::Dirty,
        ));
        let selector = TargetSelector::Id(target.workspace_id);
        let plain = render_target(&target, &selector, false);
        assert_eq!(plain.lines().count(), 8);
        assert!(!plain.contains('\u{1b}'));
        assert!(plain.contains("/work/line\\n\\u{1b}[31m"));
        assert!(plain.contains("release\\n\\u{1b} / running"));
        assert!(plain.contains("0/1 api(dirty)"));
        let colored = render_target(&target, &selector, true);
        assert!(colored.contains("\u{1b}[31mapi(dirty)\u{1b}[0m"));
        for (plain, colored) in plain.lines().zip(colored.lines()) {
            assert_eq!(
                super::super::display_width(plain),
                super::super::display_width(colored)
            );
        }
    }

    #[test]
    fn composes_every_inventory_and_preserves_no_target_output_exactly() {
        let target = target();
        let selector = TargetSelector::Directory(target.path.clone());
        for (mut snapshot, heading, empty) in [
            (
                Snapshot::Pools(super::super::PoolStatusSnapshot::empty()),
                "Pools",
                "No workspace pools.",
            ),
            (
                Snapshot::Workspaces(StatusSnapshot::empty()),
                "Workspaces",
                "No workspaces.",
            ),
            (
                Snapshot::Repos(repos::RepoSnapshot::empty()),
                "Repositories",
                "No repositories.",
            ),
        ] {
            assert_eq!(render(&snapshot, &selector, false), empty);
            match &mut snapshot {
                Snapshot::Pools(snapshot) => snapshot.target_workspace = Some(target.clone()),
                Snapshot::Workspaces(snapshot) => snapshot.target_workspace = Some(target.clone()),
                Snapshot::Repos(snapshot) => snapshot.target_workspace = Some(target.clone()),
            }
            let output = render(&snapshot, &selector, false);
            assert!(output.starts_with("Workspace (current directory)\n"));
            assert!(output.ends_with(&format!("\n\n{heading}\n{empty}")));
        }
        let mut snapshot = StatusSnapshot::empty();
        snapshot.workspaces.push(target.clone());
        let inventory = super::super::render_workspaces_human(&snapshot, false);
        assert_eq!(
            render(&Snapshot::Workspaces(snapshot.clone()), &selector, false),
            inventory
        );
        snapshot.target_workspace = Some(target.clone());
        let output = render(&Snapshot::Workspaces(snapshot), &selector, false);
        assert!(output.ends_with(&format!("\n\nWorkspaces\n{inventory}")));
        assert_eq!(output.matches(&target.workspace_id.to_string()).count(), 2);
    }
}
