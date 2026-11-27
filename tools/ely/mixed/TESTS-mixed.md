# Mixed-provider tests

Apply `patch_mixed5.py` to an isolated copy of the formatted `src-mixed4` tree and run `cargo fmt --all` in the checkout. The fifth pass was prepared by source inspection. Patch application, builds, tests, rustfmt, and clippy are pending on a writable checkout.

## Fifth-pass tests

| Crate | Command |
|---|---|
| `horizon-server` | `cargo test -p horizon-server --lib cancelling_a_bound_claim_ends_the_session_and_blocks_binding` |
| `horizon-server` | `cargo test -p horizon-server --lib identity_restart_keeps_verdict_on_read_error_and_stops_on_refusal` |
| `horizon-server` | `cargo test -p horizon-server --lib mixed_cold_start_uses_success_for_refusal_and_failure_for_read_errors` |
| `horizon-server` | `cargo test -p horizon-server --lib legacy_restores_continue_after_list_read_errors` |
| `horizon-server` | `cargo test -p horizon-server --lib stopped_legacy_edits_only_need_to_read_their_own_list` |
| `horizon-server` | `cargo test -p horizon-server --lib mixed_mode_can_be_set_locally_and_is_refused_remotely` |
| `horizon-client` | `cargo test -p horizon-client --lib legacy_identity_preflight_warns_on_reads_and_refuses_evidence` |
| `horizon-client` | `cargo test -p horizon-client --lib placed_head_uses_embedded_skin_only_in_legacy_mode` |

The fifth pass also updates two existing tests. Run the launcher test after applying the revised Ely launcher patch. The launcher is a separate Cargo workspace, so its command specifies the manifest.

| Crate | Command |
|---|---|
| `horizon-server` | `cargo test -p horizon-server --lib reviewed_mixed_backup_marker_and_restore_mode_follow_the_folder` |
| `horizon-launcher` | `cargo test --manifest-path launcher/Cargo.toml -p horizon-launcher --lib ely_client_disables_cosmetics_and_clears_overrides` |

## Earlier mixed-provider tests

| Crate | Command |
|---|---|
| `horizon-server` | `cargo test -p horizon-server --lib reviewed_mixed_api_accepts_full_length_names_and_removal_ids` |
| `horizon-server` | `cargo test -p horizon-server --lib reviewed_mixed_classifies_provider_misses_and_names` |
| `horizon-server` | `cargo test -p horizon-server --lib mixed_verification_orders_calls_and_records_provider` |
| `horizon-server` | `cargo test -p horizon-server --lib reviewed_mixed_claims_inherit_cancel_and_keep_replacement_ownership` |
| `horizon-server-files` | `cargo test -p horizon-server-files --lib mixed_list_lookup_routes_names_without_fallback` |
| `horizon-server-files` | `cargo test -p horizon-server-files --lib mixed_lists_require_qualified_namespace_matched_rows` |
| `horizon-server-files` | `cargo test -p horizon-server-files --lib reviewed_mixed_folder_evidence_requires_matching_uuid_and_name` |
| `horizon-server-files` | `cargo test -p horizon-server-files --lib reviewed_mixed_removal_prefers_stored_names_and_disambiguates_by_id` |
| `horizon-client-render` | `cargo test -p horizon-client-render --lib mixed_texture_key_is_chosen_by_the_players_provider` |
| `horizon-client-render` | `cargo test -p horizon-client-render --lib reviewed_mixed_texture_downloads_have_separate_cache_keys` |
| `horizon-client-render` | `cargo test -p horizon-client-render --lib reviewed_mixed_head_cache_drops_embedded_textures_only_in_mixed_mode` |
| `horizon-protocol` | `cargo test -p horizon-protocol --lib provider_namespaces_are_disjoint_and_reversible` |
| `horizon-protocol` | `cargo test -p horizon-protocol --lib reviewed_mixed_client_enablement_requires_address_and_cosmetics_off` |
| `horizon-server-launch` | `cargo test -p horizon-server-launch --lib reviewed_mixed_config_render_keeps_legacy_and_mixed_round_trips` |
| `horizon-server` | `cargo test -p horizon-server --lib unattended_init_writes_everything -- --ignored` |

For the incorporated engine fix, run `cargo test -p horizon-server --lib a_relative_folder_reaches_the_game_whole -- --ignored` on Linux.

## Existing regression suites

Run `cargo test -p horizon-server --lib`, `cargo test -p horizon-server-files --lib`, `cargo test -p horizon-client-render --lib skins::`, `cargo test -p horizon-client-render --lib auth::`, and `cargo test -p horizon-client-lib --lib mixed` in the deployment workspace. Repeat the previous `cargo check` with and without `upstream-parallel-mob-islands` and the three CI clippy invocations with `-D warnings`.

| Crate | Command |
|---|---|
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib hiding_goes_through_mojang_and_sends_only_the_services_session` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib ownership_is_whether_the_accounts_own_list_has_anything` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib mojang_refusing_stops_before_the_service_is_asked_to_trust_it` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib emotes_sign_in_once_and_again_only_when_the_session_is_gone` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib the_wheel_gets_the_owned_emotes_files_and_names` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib leaving_the_list_signs_in_the_same_way` |

## Deployment acceptance

1. Apply each fifth-pass script and verify that repeating the application fails during preflight. For both launch paths, start with `HORIZON_MIXED_PROVIDERS=1` and `HORIZON_COSMETICS=1` in the parent environment. Verify that the game receives the deployment address and cosmetics value `0`, and that its environment lacks `HORIZON_MIXED_PROVIDERS`.
2. Start legacy mode with offline bans for `ely:x` and `mojang:x`, then with a bare foreign v7/v8 UUID. Each start should proceed. Repeat with valid qualified live and durable rows and with the marker alone. Confirmed refusals should give the clean-folder instruction and exit 0. Mixed startup with an absent or invalid marker or an unmet mixed prerequisite should also exit 0. A cold-start identity-check I/O error should exit non-zero.
3. Restart the engine with a temporary read error at either supervisor recheck. Verify the warning and the retained verdict. Clear the error and verify that a later confirmed refusal stops startup. Edit the mode while the supervisor runs and verify that it retains its pinned mode. With mixed mode off, check that standalone engine preflight warns and proceeds past the identity check when a list is unreadable.
4. Inspect manual and scheduled mixed ZIPs for the migration marker. Exercise whole-world, dimension, queued, and update-rollback restores with valid archives. Repeat with missing markers, malformed markers, incompatible rows, and duplicate effective UUIDs. Check that refusal leaves the live world intact. With the setting off, exercise both restore commands with an unreadable list and a clean legacy ZIP. Confirm they proceed until readable mixed evidence is present. Verify that a legacy destination without evidence refuses a ZIP carrying the mixed marker.
5. Check default init, vanilla import, legacy `[proxy]` rendering, and the round trip with mixed mode on. Verify local `horizon-server config proxy.mixed_providers=true` and confirm the control API and panel refuse edits to that field.
6. Join from each launcher using both providers. Test different addresses with a matching-looking property, case-insensitive host matching, the default port, and the cosmetics settings. Confirm legacy cosmetics sign-in still uses its existing challenge handling.
7. Delay each provider's texture keys and confirm the default skin gives way to the verified skin after the keys arrive. Draw item and placed heads for qualified names and effective Ely.by UUIDs. Check that repeated draws reuse the completed profile result, embedded textures and ambiguous v4 IDs give the mixed-mode default, resource-pack overrides still work, and legacy heads retain their embedded textures.
8. Try an unlisted same-name login alongside an admitted account while the engine is up. Verify that backend refusal leaves admission available. During downtime, check claim inheritance, UUID conflicts, case variants, renames, and overlapping login and reattach. This exercises the login wiring with the backend.
9. Exercise stored-row removal over console, in-game commands, stopped CLI, and control API. Test mixed bulk replacement with retained rows, blank input, unresolved names, mismatched UUID/name pairs, and an unreachable profile endpoint. Verify list integrity, qualified events, whitelist reload feedback, UUID anticheat kicks, chat keys, and Noelcraft permissions and ownership under both providers.
