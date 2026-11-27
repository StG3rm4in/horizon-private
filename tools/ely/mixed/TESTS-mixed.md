# Mixed-provider tests

Apply the core patch and the fourth pass to an isolated copy of `7f548a1617`, then run rustfmt. Use the exact filters below from the workspace root. The fourth pass was checked by source inspection.

## Patch tests by crate

| Crate | Command |
|---|---|
| `horizon-server` | `cargo test -p horizon-server --lib reviewed_mixed_api_accepts_full_length_names_and_removal_ids` |
| `horizon-server` | `cargo test -p horizon-server --lib reviewed_mixed_classifies_provider_misses_and_names` |
| `horizon-server` | `cargo test -p horizon-server --lib mixed_verification_orders_calls_and_records_provider` |
| `horizon-server` | `cargo test -p horizon-server --lib reviewed_mixed_claims_inherit_cancel_and_keep_replacement_ownership` |
| `horizon-server` | `cargo test -p horizon-server --lib reviewed_mixed_backup_marker_and_restore_mode_follow_the_folder` |
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

`unattended_init_writes_everything` already carries the daily ignore attribute. For the incorporated engine fix, the corresponding test is `cargo test -p horizon-server --lib a_relative_folder_reaches_the_game_whole -- --ignored` on Linux.

## Existing regression suites

Run `cargo test -p horizon-server --lib`, `cargo test -p horizon-server-files --lib`, `cargo test -p horizon-client-render --lib skins::`, `cargo test -p horizon-client-render --lib auth::`, and `cargo test -p horizon-client-lib --lib mixed` in the complete deployment workspace. Repeat the previous `cargo check` with and without `upstream-parallel-mob-islands` and the three CI clippy invocations with `-D warnings`.

The cosmetics account flow uses its existing behavior. Run its account tests explicitly.

| Crate | Command |
|---|---|
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib hiding_goes_through_mojang_and_sends_only_the_services_session` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib ownership_is_whether_the_accounts_own_list_has_anything` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib mojang_refusing_stops_before_the_service_is_asked_to_trust_it` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib emotes_sign_in_once_and_again_only_when_the_session_is_gone` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib the_wheel_gets_the_owned_emotes_files_and_names` |
| `horizon-cosmetics` | `cargo test -p horizon-cosmetics --lib leaving_the_list_signs_in_the_same_way` |

## Deployment acceptance

1. Apply the launcher patch to a Prism wrapper with the cosmetics patch and to a deployment copy of `patch_ely_launcher.py`. Check that applying again fails during preflight. Launch both with a parent environment containing `HORIZON_MIXED_PROVIDERS=1` and `HORIZON_COSMETICS=1`. Verify the game receives the deployment address and cosmetics value `0` and that its environment lacks `HORIZON_MIXED_PROVIDERS`.
2. Start legacy mode with offline bans for `ely:x` and `mojang:x`, then with a bare foreign v7/v8 UUID. Each start should proceed. Repeat with valid qualified live and durable rows, and with the marker alone. Each confirmed refusal should log the clean-folder rollback instruction and exit 0. Mixed startup with an absent or invalid marker or an unmet mixed prerequisite should also exit 0. An I/O error in the cold-start identity check should exit non-zero.
3. Restart the engine with a temporary identity-check I/O failure at either of the supervisor's two rechecks. Verify that the supervisor logs the failure, retains its prior start verdict, and continues supervising. Clear the error and verify that a later confirmed refusal stops startup. Edit the mode while the supervisor runs and verify that it retains its pinned mode.
4. Inspect both manual and scheduled mixed ZIPs for `.horizon/mixed-identities-v1`. Exercise whole-world, dimension, and queued restores using a valid mixed archive. Repeat with a missing marker, malformed marker, incompatible row, and duplicate effective UUID. Check that refused restores leave the live world untouched. With the setting off, test both restore commands using a legacy archive and a destination with mixed evidence. Also verify that a clean legacy destination refuses an archive carrying the mixed marker.
5. Start a legacy proxied deployment using a rendered config with `[proxy]` and mixed mode off. Check that the older server accepts the file. Confirm that default init, init from a vanilla properties file, explicit `mixed_providers = false`, and a true mixed config all produce the expected render and parse results.
6. Join from each launcher with Mojang and Ely.by profiles. Test a different host or port that sends a matching-looking property. Verify case-insensitive host matching and the default port when configured for `25565`. Confirm cosmetics stay disabled on the two launch paths and legacy cosmetics-enabled sign-in keeps the existing challenge handling.
7. Delay each texture-key endpoint in turn. Login should reach configuration and pending skins should update after the keys arrive. A transient failure should recover after the cooldown. Try unsigned and invalid properties. Render mixed item and placed heads for explicit provider names and effective Ely.by IDs. Check that embedded properties and ambiguous bare v4 IDs give the default skin, resource-pack overrides still work, and legacy heads retain their embedded textures.
8. While the engine is up, attempt an unlisted same-name login alongside an admitted account. Verify that backend refusal leaves the eventual admission available. While the engine is down, check same-name inheritance, UUID conflicts, case variants, renames, and overlapping login and reattach. The unit test covers claim ownership and connection cancellation. This scenario covers the session and backend wiring.
9. Exercise stored-row removal over console, in-game commands, stopped CLI, and control API. Test mixed bulk replacement with retained rows, blank input, unresolved names, mismatched UUID/name pairs, and an unreachable profile endpoint. Invalid requests should leave the lists intact. Check qualified events, whitelist reload error feedback, UUID anticheat kicks, chat keys, and the installed Noelcraft permissions and ownership checks with both providers.
