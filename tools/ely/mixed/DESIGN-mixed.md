# Mixed Microsoft and Ely.by authentication

Apply `patch_ely_core.py` to a clean Horizon `7f548a1617` checkout. Keep the unchanged `patch_mixed.py` files from `out/mixed`, `out/mixed2`, and `out/mixed3` as separate inputs. Run `python3 -I patch_mixed.py ROOT ORIGINAL_MIXED MIXED2 MIXED3 PATCH_SUPPORT` before formatting the checkout. This revision verifies all three archived plan fingerprints and stages their edits together with the fourth pass. Run rustfmt after application.

Apply `p_wrapper_cosmetics.py` to the Prism wrapper first. Run `python3 -I patch_mixed_launch.py SERVER_ROOT ELY_LAUNCHER_PATCH HOST:PORT` on the deployment tree and a deployment copy of the launcher patch. Apply the revised launcher patch to the Horizon checkout before building. Use the same public address in both launch profiles.

Enable `mixed_providers = true` and `enabled = true` in `[proxy]`, `online_mode = true` in `[server]`, and `enabled = false` in `[transfer]`. Deploy matching patched supervisor, dedicated engine, and Horizon clients. The public proxy verifies encrypted login and forwards the verified identity with the existing Velocity HMAC to the private IPv4 loopback engine. The default is legacy mode. Rendered configurations omit `mixed_providers` when it is false, including when another proxy option causes the `[proxy]` table to be written.

## Identity and migration

The proxy checks Mojang first and Ely.by after a miss or verifier failure. An Ely.by success asserts only an Ely.by identity. A backend admission refusal is final for that login. Provider requests use fixed HTTPS endpoints, bounded responses and timeouts, and disabled redirects. A provider response must contain the requested name and a supported UUID.

Mojang RFC-variant version-4 UUIDs stay unchanged. Ely.by RFC-variant version-4 and version-5 UUIDs use effective version tags 7 and 8 respectively. The mapping preserves the other bits. UUID-keyed permissions, bans, inventories, advancement data, statistics, and other persistent records use the effective UUID.

Stop the server and back up the complete directory and external mod data. Establish provider provenance for every identity-bearing record and quarantine ambiguous records. Move proven Ely.by data to the effective UUID paths, including embedded references, and check destination collisions. Qualify all player rows in `ops.json`, `whitelist.json`, and `banned-players.json` with `mojang:` or `ely:`. Preserve moderation metadata. Archive the old user cache. Audit scores, teams, and mods that key storage by display name.

Create `.horizon/mixed-identities-v1` containing `mixed-identities-v1` after completing migration. Mixed backups include that marker. Whole-world and dimension restores validate the open archive before replacing live data. A mixed setting, a destination marker, or valid mixed rows in the destination require a mixed archive. A mixed archive must contain the valid marker and any included player lists must pass the mixed list parser. A clean legacy destination with legacy settings rejects an archive carrying the mixed marker. Restore replaces world data and leaves the destination configuration and lists in place.

Legacy startup checks the marker, live lists, and durable recovery copies. A marker is evidence on its own. A list row is evidence when its qualified name and UUID pass mixed list validation. The UUID version must be 4 for `mojang:` and 7 or 8 for `ely:`. Offline UUIDs are version 3, including for a name such as `ely:x`. To return to legacy mode, restore the complete pre-migration directory and matching external mod data into a clean folder.

An identity configuration refusal logs a reason and exits 0. At cold start, an I/O error during the identity check is fatal. A running supervisor pins the original identity mode and checks the directory before and after queued restore and release work. On an identity-check I/O error during restart, it logs the error and retains the last successful start verdict. A confirmed refusal still stops startup.

## Moderation and reconnects

Use qualified names for typed additions and bans. `deop`, `pardon`, and `whitelist remove` prefer the stored qualified name or effective UUID. Duplicate names require a UUID. A missing qualified-name row can fall back to its provider lookup where supported. Console misses and ambiguous rows produce the normal unknown-player feedback.

Mixed `allowlist/set`, `bans/set`, and `operators/set` resolve against rows already stored in the respective list. An unrecognized or ambiguous entry rejects the replacement before mutation. Use the corresponding add command or route for new identities. Removing entries, clearing a list, and changing supported metadata on retained entries remain available through replacement.

Only an admitted connection can create a name claim. During downtime, a reconnect can inherit an existing claim for the same effective UUID and case-insensitive name. A different UUID cannot replace an active or held claimant. Same-UUID initial backend logins and reattach attempts are serialized. The engine rechecks authorization on reattach.

## Clients, skins, and cosmetics

The client compares the configured address with the logical connection address before SRV routing and requires a matching provider property in the login profile. DNS host comparison is case-insensitive and the port must match. LAN, local, friend-tunnel, and transferred logins stay on their existing paths. The Prism wrapper and `ely-play` set `HORIZON_MIXED_CLIENT=HOST:PORT` and `HORIZON_COSMETICS=0` and remove `HORIZON_MIXED_PROVIDERS` from the game environment. Custom launchers should apply the same environment settings. Other addresses in the same game process use legacy rendering.

Mixed textures require the signing key for the effective UUID's provider and a matching signed raw `profileId`. Verified URLs use HTTPS, provider-specific paths, bounded downloads, and no redirects. The verified download cache is separate from the legacy cache. Key fetches run in background threads. Player-list properties that arrive before the keys are retained and checked again. The client displays a default skin while verification is pending.

Mixed item and placed heads can resolve a qualified name or an effective Ely.by UUID. Bare version-4 UUIDs in dynamic head data are ambiguous. Embedded texture properties use the default skin in mixed mode. Item-head parsing uses bounded, mode-specific caches. Dynamic lookups still advance each draw, and the mixed resolved cache is consulted after provider verification. Resource-pack texture overrides retain their existing behavior. The player list uses the authenticated effective UUID, including version-4 Mojang UUIDs.

Typed player events use a qualified `name`, bare `display_name`, and effective `uuid`. The user cache keeps display names for rename checks. Mixed native online profile lookup requires a qualified name. Teams, scoreboards, and mods with their own name-keyed storage keep their existing semantics and need a deployment audit.
