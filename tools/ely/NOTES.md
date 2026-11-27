# Ely.by port for Horizon 7f548a1617

Save the two patch scripts together. From a writable checkout run `python3 -I patch_ely_core.py ROOT`, with `ROOT` set to the clean target checkout or the generated Windows build tree. Generate the latter from `7f548a1617` with the Noelcraft overlay and existing CI packaging steps. The sample `client_v3.sh` names `65906193d4`, so substitute the target revision for this canary. Apply the patch before building and commit the resulting sources before `xtask package`, which archives preparer sources from `HEAD`. The script edits fork sources and adds the embedded discovery document. It does not use workspace-member ordering, the generated wrapper member or Noelcraft crate paths as anchors.

For Windows, extract the ZIP and place `ely_play.py` beside `horizon-client.exe` and `horizon-prepare.exe` in `noelcraft-client-win-x64`. Run from an interactive terminal.

    py -3 -I .\ely_play.py --server HOST:PORT

Use `--bundle DIR` if the runner lives elsewhere. `--prism-instance DIR` selects a specific instance folder and `--minecraft DIR` selects the official game directory. The runner checks 26.3 metadata, the client JAR and the asset store before local preparation. Rust validates the pinned import and handles missing required libraries or assets. The runner checks Prism-managed Java in the selected Prism root and the default Prism root, or uses `--java-home DIR`. Java 25 is the target pin. Use a fresh extraction for each source-path acceptance test because Horizon shares verified import state under the bundle's `.local`. Resource preparation reads installed game files. The candidate's texture endpoints still allow skin images from `textures.minecraft.net` when referenced by a provider profile.

## Changes from the candidate

- The `AuthSession::new` anchor now includes `xuid: None`. The old constructor suffix also matched `AuthSession::parse` in the target. The Prism selection anchor now includes `= inputs`, matching the current chained expression in `GameFiles::with`. The official-local import edits the current server selection and fallback log as separate anchors. The candidate's identity replacement at `write_import_metadata` was removed. The other candidate anchors still matched in the supplied source.
- The player runner uses the packaged Windows companion executables and root lock file. It tries a matching Prism instance first and passes that instance to `--prism-dir`, then falls back to the official local layout. It prepares before asking for credentials. It clears inherited preparation and session-file overrides and sends the Ely.by session JSON through the client's standard input. The HTTP 401 error fields gate the single hidden TOTP retry. [Ely.by authentication documentation](https://docs.ely.by/en/minecraft-auth.html)
- The core keeps the embedded provider discovery, the existing encrypted join and verify handshakes, and provider-key verification of the original encoded texture value. The Ely.by verify requests disable redirects in both the proxy and direct engine. The proxy emits a startup marker when online authentication uses Ely.by. The Ely.by join transport and HTTP errors avoid logging provider-supplied details. The Mojang and ordinary custom discovery branches retain their existing agents and error handling.
- `patch_ely_launcher.py` is deferred. The Windows canary uses the standalone runner.

## Linux tests

Two tests are added. No existing tests are edited.

- `horizon-client-render` — `skins::ely_texture_signatures_use_the_encoded_value_and_skip_the_placeholder`. `cargo test -p horizon-client-render --lib skins::ely_texture_signatures_use_the_encoded_value_and_skip_the_placeholder -- --exact`
- `horizon-server` — `proxy::auth::ely_discovery_routes_to_has_joined`. `cargo test -p horizon-server --lib proxy::auth::ely_discovery_routes_to_has_joined -- --exact`

The workspace was read-only. I inspected the source and compared candidate anchors in memory. Patch application, repeat-application rejection, generated-tree application, compilation and runtime behavior remain unverified. Windows local preparation with networking disconnected, the Noelcraft build and resource match, the real Ely.by canary, and the default Microsoft regression remain to be exercised.

## Live checks carried over from TESTS-ely.md

- Confirm `POST /session/join` accepts `accessToken`, undashed `selectedProfile` and the signed hexadecimal `serverId`. Record success and refusal statuses and whether `selectedProfile` is required. Confirm `GET /session/hasJoined` accepts `username` and `serverId`, returns the `id`, `name` and `properties` the proxy expects, and refuses wrong hashes, missing joins and expired or wrong-provider sessions. Check support and enforcement for `ip` separately on the direct engine.
- Confirm the authenticated identity survives encrypted login and proxy forwarding, an unlisted UUID is refused, provider outage fails closed and ordinary chat works with secure profiles disabled.
- Confirm HTTPS DER key availability, SPKI format and SHA1withRSA over the original base64 property. Check a real skin, slim metadata, cape, changed payload, wrong key, malformed signature, `Cg==`, unlisted host and redirects. Exercise own and remote skins plus name-resolved heads and UUID-resolved mannequins. Restart the client after a key retrieval failure or provider key rotation because the lookup result is cached for the process.
- Confirm actual image paths, redirects, lowercase name lookup and misses, and the UUID profile endpoint with `unsigned=false`, including empty results and extra properties. Check whether the Mojang fallback consists of a Mojang-signed property or a Mojang-hosted image inside an Ely.by-signed property.
- The deferred OAuth experiment still needs a real consent grant with `offline_access`, `account_info` and `minecraft_server_session`, followed by encrypted joins with the initial and refreshed token. Confirm whether a token exchange, special request fields, PKCE or confidential-client handling is needed.
