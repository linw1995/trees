# Verification Evidence

## Commands

- `cargo test --all-targets --all-features`: 266 tests passed in the complete application regression suite.
- `prek -a`: complete repository hook suite; all checks passed.
- `nix flake check --no-build`: passed for the native platform.
- `bash scripts/run-cov.sh`: coverage generated with all 262 tests present at that point passing; subsequent additional regression tests also passed.
- `nix develop .#crap --command bash scripts/run-crap.sh`: 585 functions analyzed, zero above the threshold of 30.
- `openspec validate add-origin-repository-management --strict`: planning artifacts validated.

## Scenario Coverage

Test names below refer to `tests/origin_provision.rs` unless another file is specified.

| Specification scenarios | Verification |
| --- | --- |
| Create directly from a remote URL; resolve a directory name; reject an unknown name | `creates_from_url_then_reuses_directory_name` |
| Mix a local checkout and a remote; preserve both workspace modes | `mixed_manual_and_automatic_origins_support_both_workspace_modes` |
| Reject an ambiguous directory name; register a manual repository inside the managed root; keep same-named repositories distinct | `manual_paths_inside_the_managed_root_remain_manual_and_names_can_be_ambiguous` |
| Preserve explicit path interpretation | `src/origin/input.rs`: `classifies_urls_names_and_explicit_paths` |
| Reject duplicate resolved origins; validate an expected identity before fetching | `rejects_duplicate_inputs_and_changed_identity_before_workspace_mutation` |
| Respect offline mode; reuse a URL across workspace creations | `repeated_url_reuses_pool_and_offline_requires_a_known_clone` |
| Preserve existing path-based behavior | Existing `tests/create.rs` and `tests/workspace_reuse.rs` regression suites |
| Look up either management mode by name | `creates_from_url_then_reuses_directory_name`; `src/storage/origin.rs`: `names_preserve_distinct_identities_and_registration` |
| Resolve an existing automatic origin by path | `src/storage/origin.rs`: `path_registration_preserves_automatic_url_and_root`; existing linked-worktree creation regressions |
| Change the root for new clones | `src/config.rs`: `origins_setting_preserves_other_sections_without_creating_storage`; `preflight_prevents_clone_and_later_failures_preserve_published_sources` |
| Preserve the recorded root for a known URL; clone a new source; reject a broken retained clone | `clones_and_reuses_origins_across_configuration_changes` |
| Fail during clone | `failed_clone_does_not_publish_an_origin`; `unusable_head_is_not_published_and_its_clone_is_cleaned` |
| Preserve a committed origin | `cleanup_never_removes_a_published_origin`; `preflight_prevents_clone_and_later_failures_preserve_published_sources` |
| Recover after interruption | `recovers_interrupted_owned_clone_and_preserves_unproven_content`; `publication_failure_rolls_back_and_cleans_only_its_clone` |
| Reject concurrent provisioning | `src/origin/reservation.rs`: `serializes_urls_and_retains_abandoned_intent` |
| Default to pool allocation status; select workspace details; include reclaimed workspace details; reject all for pool status | Existing `tests/status.rs` and `src/cli.rs` status tests |
| Select origin repositories; inspect without side effects; emit repos JSON | `repos_json_is_versioned_and_reports_missing_sources_without_mutation` |
| Display same-named origins | `src/status/repos.rs`: `reports_stored_origins_with_unique_labels_and_safe_output` |
| Reject an incompatible status schema without migration | `old_schema_status_fails_without_migrating_or_emitting_partial_json` |
| Remove a manual workspace; remove an automatic workspace | Existing `tests/remove.rs` and `tests/workspace_reuse.rs` explicit removal tests |
| Dispatch an origin identifier; reject ambiguous entity identity | `src/storage/removal.rs`: `resolves_entity_type_and_rejects_cross_table_collision` |
| Preview repository registration removal; force does not delete an origin; remove a referenced automatic origin; register a retained origin again | `unregister_preserves_sources_claims_and_identity_for_both_modes` |
| Preserve migration identity and require pending recovery before rollback | `tests/origin_migrations.rs`; updated legacy migration cases in `tests/persistence.rs` |

## Scope

Repository removal changes registration only. Source deletion, alias management,
a separate repository command group, and remote publishing are not included.
Automatic clones are reused by exact URL; names use primary source directory
base names with existing local paths taking precedence.
