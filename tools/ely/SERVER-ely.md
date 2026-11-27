# Dedicated Ely.by canary

Build the patched `horizon-server` and matching dedicated engine from `7f548a1617`. Use the same Noelcraft build and native mod package as the Windows player ZIP. Start in an initialized staging server directory with prepared resources.

For the supervisor, merge these keys into the existing `server.toml` tables.

    [server]
    online_mode = true
    whitelist = true

    [properties]
    enforce-secure-profile = false
    prevent-proxy-connections = false

    [proxy]
    enabled = true

The supervisor writes `online-mode=true` and `white-list=true` into `server.properties` before applying the private engine overrides for forwarding. With the proxy enabled, keep only the public proxy port reachable. Its engine listens on loopback and receives the verified identity through Velocity forwarding. For a direct dedicated engine, set `online-mode=true`, `white-list=true`, `enforce-secure-profile=false` and `prevent-proxy-connections=false` in `server.properties` and omit the supervisor proxy. Secure chat certificates are optional in this canary. IP binding on the direct authentication path is a separate Ely.by check.

With the server stopped, put the actual Ely.by profile UUID and canonical name in `whitelist.json` beside the server properties. The UUID below is an example.

    [
      {"uuid":"12345678-1234-4234-8234-123456789abc","name":"ExamplePlayer"}
    ]

Start the foreground supervisor in that directory on Linux. Set the same environment variable on the engine process if running it directly.

    HORIZON_API_DISCOVERY=ely.by ./horizon-server run

For a supervised canary, check for `Proxy session verification uses Ely.by` at startup. On a successful join, check `NAME joined through the proxy (UUID HEX)` and the engine's `NAME joined the game` in the foreground output or server logs. The 32 hex digits must equal the whitelisted Ely.by UUID after removing hyphens. On a direct engine, check for `UUID of player NAME is UUID` and the subsequent join. Correlate the join with the player's successful entry into the world.

Try a second valid Ely.by account whose UUID is absent from the whitelist and check that the player receives a whitelist refusal and has no successful world join. Confirm that an unverified session is refused and that service failure closes the login. Watch the client for the authentication disconnect reason when the proxy has no successful join entry to correlate. Test a signed remote skin while both clients are in the world.
