# Verification Evidence

## Checks

- `cargo test --all-targets --all-features`: 268 tests passed.
- `prek -a`: all repository checks passed.
- `nix flake check --no-build`: native platform checks passed.
- `bash scripts/run-cov.sh`: coverage generated; all 268 tests passed.
- `nix develop .#crap --command bash scripts/run-crap.sh`: complexity gate passed with zero functions above threshold 30.
- `openspec validate add-origin-repository-management --strict`: passed.

## Corrected Contract Coverage

| Behavior | Verification |
| --- | --- |
| Origin schema retains only the existing three columns | `tests/origin_migrations.rs`: `origin_upgrade_and_rollback_preserve_identity` |
| Clone operations remain recoverable and block premature rollback | `tests/origin_migrations.rs`: `rollback_requires_pending_clone_recovery` |
| Local sources inside the clone root need no mode metadata | `tests/origin_provision.rs`: `local_sources_inside_clone_root_need_no_modes_and_names_can_be_ambiguous` |
| URL lookup reuses local sources, observes Git changes, and rejects multiple matches | `tests/origin_provision.rs`: `url_lookup_uses_current_git_configuration_and_rejects_multiple_matches` |
| Root changes preserve matching existing sources | `tests/origin_provision.rs`: `clones_and_reuses_origins_across_configuration_changes` |
| URL reuse preserves workspace pools and offline behavior | `tests/origin_provision.rs`: `repeated_url_reuses_pool_and_offline_requires_a_known_clone` |
| Paths, URLs, and names retain existing creation behavior | Input tests and existing create tests; `tests/origin_provision.rs`: `creates_from_url_then_reuses_directory_name`, `rejects_duplicate_inputs_and_changed_identity_before_workspace_mutation` |
| Both workspace modes accept local and cloned sources | `tests/origin_provision.rs`: `mixes_local_and_cloned_sources_for_both_workspace_modes` |
| Failed and interrupted cloning never removes unrelated or published data | Reservation tests and the failure, recovery, and publication tests in `tests/origin_provision.rs` |
| Repos status exposes only identity and path metadata without Git probes | `tests/origin_provision.rs`: `repos_json_is_versioned_and_reports_missing_sources_without_mutation` |
| Repos status works before the new operation migration | `tests/origin_provision.rs`: `existing_schema_status_succeeds_without_migration` |
| Live and historical references prevent origin deletion | `tests/origin_provision.rs`: `rejects_origin_removal_with_live_and_historical_references` |
| Reference checks are repeated inside the deletion transaction | `src/storage/removal.rs`: `rejects_references_added_after_preflight` |
| A record without references can be removed while keeping source files | `tests/origin_provision.rs`: `removes_an_origin_without_references_but_keeps_source_files` |
| Existing workspace removal, release, GC, and status remain intact | Existing `tests/remove.rs`, `tests/workspace_reuse.rs`, and `tests/status.rs` suites |

## Migration Scope

Migration 3 creates only `pending_origin_clones`. There are no added columns
or URL indexes on `origin_repositories`. The migration has not been published;
the correction does not alter user databases already exercised with an
intermediate development build. Those databases can retain ignored extra
columns until explicitly restored to the pre-feature schema.
