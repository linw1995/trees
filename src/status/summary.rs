use time::{OffsetDateTime, UtcOffset};

use super::combined::Snapshot;
use super::processes::{Completeness, Observation};
use super::target::TargetSelector;
use super::{repos, LeaseStatus, WorkspaceStatus};
use crate::domain::Timestamp;

pub fn render(
    snapshot: &Snapshot,
    selector: &TargetSelector,
    color: bool,
    processes: Option<&Observation>,
) -> String {
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
            render_target(target, selector, color, processes)
        ),
        None => inventory,
    }
}

fn render_target(
    target: &WorkspaceStatus,
    selector: &TargetSelector,
    color: bool,
    processes: Option<&Observation>,
) -> String {
    render_target_with_offset(target, selector, color, processes, |instant| {
        UtcOffset::local_offset_at(instant).ok()
    })
}

fn render_target_with_offset(
    target: &WorkspaceStatus,
    selector: &TargetSelector,
    color: bool,
    processes: Option<&Observation>,
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
    if let Some(processes) = processes {
        rows.push(("Processes", process_count(processes)));
    }
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
        if label == "Processes" {
            if let Some(processes) = processes {
                output.push_str(&process_table(processes, target));
            }
        }
    }
    output
}

fn process_count(observation: &Observation) -> String {
    let count = observation.count.unwrap_or(0);
    let reasons = observation
        .issues
        .iter()
        .map(|issue| issue.code.reason())
        .collect::<Vec<_>>()
        .join("; ");
    match observation.status {
        Completeness::Complete => count.to_string(),
        Completeness::Partial => format!("{count} (partial: {reasons})"),
        Completeness::Unavailable => format!("unavailable ({reasons})"),
    }
}

fn process_table(observation: &Observation, target: &WorkspaceStatus) -> String {
    if observation.processes.is_empty() {
        return String::new();
    }
    let rows = observation
        .processes
        .iter()
        .map(|process| {
            let relative = process
                .cwd
                .strip_prefix(target.path.as_path())
                .unwrap_or(&process.cwd);
            let cwd = if relative.as_os_str().is_empty() {
                ".".into()
            } else {
                relative.to_string_lossy()
            };
            [
                process.pid.to_string(),
                super::escape_human_label(process.name.as_deref().unwrap_or("unknown")),
                super::escape_human_label(&cwd),
            ]
        })
        .collect::<Vec<_>>();
    super::render_table(&["PID", "NAME", "CWD"], &rows)
        .lines()
        .map(|line| format!("\n    {line}"))
        .collect()
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

    fn empty_observation() -> Observation {
        Observation {
            observed_at: Timestamp::now(),
            status: Completeness::Complete,
            count: Some(0),
            processes: vec![],
            issues: vec![],
        }
    }

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
            render_target_with_offset(target, &selector, false, None, |_| {
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
        let summary = render_target_with_offset(
            &target,
            &selector,
            false,
            Some(&empty_observation()),
            |_| UtcOffset::from_hms(8, 0, 0).ok(),
        );
        let example = format!("{summary}\n\nPools\nNo workspace pools.");
        assert!(include_str!("../../docs/status.md").contains(&example));
    }

    #[test]
    fn renders_process_lists_completeness_and_escaped_text() {
        use super::super::processes::{Issue, IssueCode, Process};
        let target = target();
        let selector = TargetSelector::Id(target.workspace_id);
        let mut observation = empty_observation();
        observation.processes = vec![
            Process {
                pid: 1201,
                name: Some("zsh".into()),
                cwd: "/work/api".into(),
            },
            Process {
                pid: 1248,
                name: Some("cargo".into()),
                cwd: "/work/api/api".into(),
            },
            Process {
                pid: 1302,
                name: Some("node".into()),
                cwd: "/work/api/web".into(),
            },
        ];
        observation.count = Some(3);
        let output = render_target(&target, &selector, false, Some(&observation));
        assert!(output.contains("  Processes   3\n    PID   NAME   CWD\n    1201  zsh    .\n    1248  cargo  api\n    1302  node   web\n  Reconciled"));
        observation.status = Completeness::Partial;
        observation.issues = vec![Issue {
            code: IssueCode::CwdUnreadable,
            affected_count: Some(2),
        }];
        assert_eq!(
            process_count(&observation),
            "3 (partial: some process working directories could not be read)"
        );
        observation.processes.clear();
        observation.count = Some(0);
        let output = render_target(&target, &selector, false, Some(&observation));
        assert!(output.contains("Processes   0 (partial:"));
        assert!(!output.contains("PID"));
        let unavailable = Observation::unavailable(Timestamp::now(), IssueCode::EnumerationFailed);
        assert_eq!(
            process_count(&unavailable),
            "unavailable (process enumeration failed)"
        );
        assert!(process_table(&unavailable, &target).is_empty());
        observation.processes = vec![
            Process {
                pid: 1,
                name: Some("line\n\u{1b}[31m".into()),
                cwd: "/work/api/dir\tname".into(),
            },
            Process {
                pid: 2,
                name: None,
                cwd: "/work/api".into(),
            },
        ];
        let table = process_table(&observation, &target);
        assert_eq!(table.lines().count(), 4);
        assert!(!table.contains('\u{1b}'));
        assert!(table.contains("line\\n\\u{1b}[31m"));
        assert!(table.contains("dir\\tname"));
        assert!(table.contains("unknown"));
    }

    #[cfg(unix)]
    #[test]
    fn represents_non_utf8_paths_only_at_the_output_boundary() {
        use super::super::processes::Process;
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let mut observation = empty_observation();
        observation.processes.push(Process {
            pid: 1,
            name: None,
            cwd: std::path::PathBuf::from(OsString::from_vec(b"/work/api/\xff".to_vec())),
        });
        observation.count = Some(1);
        assert!(process_table(&observation, &target()).contains('\u{fffd}'));
        let json = serde_json::to_value(&observation).unwrap();
        assert_eq!(json["processes"][0]["cwd"], "/work/api/\u{fffd}");
        assert!(observation.processes[0].cwd.starts_with("/work/api"));
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
        let plain = render_target(&target, &selector, false, None);
        assert_eq!(plain.lines().count(), 8);
        assert!(!plain.contains('\u{1b}'));
        assert!(plain.contains("/work/line\\n\\u{1b}[31m"));
        assert!(plain.contains("release\\n\\u{1b} / running"));
        assert!(plain.contains("0/1 api(dirty)"));
        let colored = render_target(&target, &selector, true, None);
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
            assert_eq!(render(&snapshot, &selector, false, None), empty);
            match &mut snapshot {
                Snapshot::Pools(snapshot) => snapshot.target_workspace = Some(target.clone()),
                Snapshot::Workspaces(snapshot) => snapshot.target_workspace = Some(target.clone()),
                Snapshot::Repos(snapshot) => snapshot.target_workspace = Some(target.clone()),
            }
            let output = render(&snapshot, &selector, false, None);
            assert!(output.starts_with("Workspace (current directory)\n"));
            assert!(output.ends_with(&format!("\n\n{heading}\n{empty}")));
        }
        let mut snapshot = StatusSnapshot::empty();
        snapshot.workspaces.push(target.clone());
        let inventory = super::super::render_workspaces_human(&snapshot, false);
        assert_eq!(
            render(
                &Snapshot::Workspaces(snapshot.clone()),
                &selector,
                false,
                None
            ),
            inventory
        );
        snapshot.target_workspace = Some(target.clone());
        let output = render(&Snapshot::Workspaces(snapshot), &selector, false, None);
        assert!(output.ends_with(&format!("\n\nWorkspaces\n{inventory}")));
        assert_eq!(output.matches(&target.workspace_id.to_string()).count(), 2);
    }
}
