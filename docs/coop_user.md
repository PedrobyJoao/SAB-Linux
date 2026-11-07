# Co-op breaks

Co-op mode lets one host own the schedule while guests mirror its
next break, active break, automatic wake grace, and permitted actions.
Each computer still renders its own native overlay with its own
monitor and message settings.

Open `hyprbreathe settings` and select **Collaboration** to host, copy an invite,
join, leave, and inspect the live room status without using the terminal. A
Tailscale host can enter a MagicDNS name and port such as
`lambda-1.example.ts.net:8787`; hyprbreathe turns it into the relay URL automatically.
The relay still needs to be running and reachable on that address.

Start the relay on a server (or use a relay you trust), then create a room:

```bash
hyprbreathe coop host --relay wss://breaks.example.net/ws
```

Send the printed invite to your friend. They join by quoting the complete value:

```bash
hyprbreathe coop join 'wss://breaks.example.net/ws#hyprbreathe=<room-token>'
hyprbreathe coop status
```

When using Tailscale, use the host's MagicDNS name in the relay URL rather than
its numeric `100.x.y.z` address. A machine shared between tailnets can appear
under different Tailscale IPv4 addresses to its owner and guest; MagicDNS maps
the same hostname correctly for both. The invite is consumed by `hyprbreathe coop
join` and will not join a room when opened in a browser.

The host remains authoritative for anything that coordinates the
room: cadence, break duration and kind, automatic wake grace,
strict/postpone rules, manual resume, and notification enablement
and lead times. A guest's postpone or manual-break command is
sent to the host, and the resulting host snapshot is mirrored back.
If manual resume is enabled by the host, a key press or click from
any participant resumes the room after the countdown reaches zero.

Presentation stays local: each participant keeps their own monitor
selection, display mode, pointer behavior, messages, completion sound,
and tray preference. Both systems should have normal network time
synchronization enabled because scheduled starts use absolute
timestamps. If snapshots stop for 10 seconds, the guest starts a fresh
local schedule; reconnecting adopts the host again. Leave at any time
with `hyprbreathe coop leave`.

The relay has no database, accounts, schedule engine, or desktop dependencies.
It retains only the latest snapshot while a host is connected. Room tokens are
sent in the WebSocket `Authorization` header rather than the request URL; the
invite keeps the token in its URL fragment. Anyone holding an invite can enter
that room, so share it as a secret and create a new room to rotate it. See
[Co-op deployment and protocol](docs/coop.md) for relay setup and operational
details.
