# Netcode: the lobby layer, as far as the capture carries it

The network side is reached through the lobby's own debug/name strings. What is
read so far is the **client-side message plumbing and its registry**, not the
wire protocol. Coordinates: v2b session; range = v2b − `0x17AEC0000`, exec =
v2b − `0x71820000`.

## The one handler read

`FUN_7ff7c5380590` (v2b `0x7ff7c5380590`, range `0x7ff64a5a0590`, exec
`0x7ff753b60590`) — the handler that logs
`ClientLobbyMessageSystem_C::__LobbyServerConnectionUp`:

* looks the lobby client up by id through a registry call
  (`FUN_7ff7c52ac650(DAT_7ff7c89c93d0)` — the client registry global is v2b
  `0x7ff7c89c93d0`); a null result takes a non-returning assert path;
* branches on the client's **connection state at `client+0x94`** (compared
  against `3`); on the non-`3` path it builds a 0x40-byte message struct from
  the incoming frame and hands it to the message writer
  (`FUN_7ff7c7b8d1f0(*(client+0x18)+0x28, frame)`);
* logs through the shared logger `FUN_7ff7c5933ec0(client, level=2, message,
  name_string)`.

So the lobby client object is: registry-addressed, carries the connection
state as an int at `+0x94`, a sub-object at `+0x18` whose `+0x28` is the
message sink, and uses a reusable log entry point that takes the qualified
name as a string (which is why these names are the system's anchors).

## Anchors and their sites

| anchor (v2b) | site (v2b / exec) | note |
|---|---|---|
| `ClientLobbyMessageSystem_C::__LobbyServerConnectionUp` `0x7ff7c7d20588` | `0x7ff7c5380615` / `0x7ff753b60615` | the handler above |
| `GAME MODE LOADING` `0x7ff7c86a4b24` | `0x7ff7c54a5849` / `0x7ff753c85849` | the loading/mode-transition text path |
| `"commandframe"` `0x7ff7c7d2a338` | (no code ref) | the movement frame name; see `input-camera-audio.md` |
| `LobbySystem_C::LobbyConnectHelloErrorHandler` `0x7ff7c7d1bf10` | (no code ref) | name only |
| `AUTHENTICATED_LOBBY_CONNECTED`, `DISCONNECTED_LOBBY_CONNECTED`, `CONNECTING_LOBBY`, `AUTH_LOBBY` (`0x7ff7c86b7a34…`) | (no code ref) | state names in the message catalog, not code-referenced |
| `lobby_profile_oauth_prod.json`, `https://toolstrayapi.prod.team4.dev.cloud.blizzard.net/api/LobbyAdvertise` | — | the OAuth profile file the client loads and the advertise endpoint |
| `ClientLobbyPacketPipe` `0x7ff7c886a624` | — | the packet pipe's name |

Several state names have no reference anywhere (the same
message-catalog situation as the renderer's strings), so the **connection-state
machine's transitions are not readable from this capture either**; what is
readable is the state field's location (`client+0x94`), the registry, and the
message/log plumbing. The wire format, the connect handshake and the
matchmaking flow need the lobby code's pages, which the standing capture does
not carry.

## Method

String sweep for the lobby/network vocabulary (43 matches), RIP-relative scan
for six anchors, decompilation of the single analyzed site. Documented limits:
several anchors are name-only; the state values are compared numerically in
code but their labels are not co-located with the comparisons.
