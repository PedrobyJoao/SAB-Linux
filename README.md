# hyprbreathe

hyprbreathe is an opinitiated version of [breakd](https://github.com/simonwinther/breakd)
with more **strictness** and specifically encouraging the user
to practice **mindful breathing**.

Bringing more awareness, the user will naturally also use the breaks
to stretch, drink water or simply take a mental rest.

Also, one specifically _optional_ feature is not a simple reminder but
it will block the user of using the computer for the rest of the day
(even on reboot) after a long time (e.g.: 8 hours, configurable) of screen time.
This is of course not for everyone but for those who don't want to make
screens the main part of their day, rather having it as a tool for whatever
which is just a small slice of their lives.

See the [roadmap](roadmap.md) for visibility on features.

## Install

### Arch Linux

Install the AUR package and start the user service:

```bash
yay -S hyprbreathe
systemctl --user enable --now hyprbreathe.service
hyprbreathe status
```

`hyprbreathe` runs as your user. It does not need root access or access to `/dev/input`.

### NixOS

Add the flake and enable its NixOS module:

```nix
{
  inputs.hyprbreathe.url = "github:PedrobyJoao/hyprbreathe";

  outputs = { nixpkgs, hyprbreathe, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        hyprbreathe.nixosModules.default
        ({ ... }: { services.hyprbreathe.enable = true; })
      ];
    };
  };
}
```

Update the `hyprbreathe` input when a new release is available, then rebuild:

```bash
nix flake update hyprbreathe
sudo nixos-rebuild switch --flake .#my-host
```

For a non-declarative installation, install the package directly and enable its
user service:

```bash
nix profile install github:PedrobyJoao/hyprbreathe
systemctl --user enable --now hyprbreathe.service
```

The desktop package stays separate from the optional relay package. A NixOS
machine that will host co-op rooms can enable the small system service without
installing the GTK desktop application:

```nix
services.hyprbreathe-relay = {
  enable = true;
  listen = "127.0.0.1:8787";
  maxRoomSize = 8;
  maxRooms = 256;
};
```

Expose that private listener through a TLS reverse proxy before using it over
the internet. The package is also available directly as `.#hyprbreathe-relay`.

## Configure

`hyprbreathe` works without a configuration file. To change the defaults:

```bash
hyprbreathe settings
```

The settings window covers scheduling, break actions, strict mode, display
behavior, collaboration, idle reset, and tray visibility. It validates changes
before saving and reloads the running daemon. Advanced monitor, recovery,
message, and logging options remain available in TOML:

```bash
mkdir -p ~/.config/hyprbreathe
hyprbreathe example-config > ~/.config/hyprbreathe/config.toml
$EDITOR ~/.config/hyprbreathe/config.toml
hyprbreathe reload
```

The default schedule is a 20-second mini break every 10 minutes, a 5-minute long break every 30 minutes after 2 completed mini breaks, and a 30-minute rest break every 2 hours after 2 completed long breaks. Durations accept values such as `20s`, `7m 30s`, `1h`, and `1h 15m`.

Most changes take effect after `hyprbreathe reload`. Restart the service after changing `[idle]` or `[logging]`:

```bash
systemctl --user restart hyprbreathe.service
```

### Strict mode

Set `strict.mode` to one of these values:

- `off`: skip and postpone are available immediately.
- `delay`: skip and postpone are still available immediately; `strict.minimum_visible` only delays the pause and reset loopholes.
- `entire`: skip and postpone are locked for the whole break (`allow_postpone_during_lockout` still permits postponing).

Set `strict.inhibit_shortcuts = true` to request standard Wayland shortcut inhibition while the overlay is active. With `hyprland.submap_fallback = true`, the daemon also enters a temporary Hyprland `hyprbreathe` submap because layer surfaces do not reliably suppress compositor bindings on every Hyprland version. The submap is registered at runtime, checked throughout the break, and reset when the break or daemon exits.

This blocks normal Hyprland bindings, including the `SUPER+1..9` workspace bindings. It does not change or unbind them. If the daemon is forcibly killed, its systemd restart resets a stale `hyprbreathe` submap. The manual recovery command is `hyprctl dispatch submap reset`.

### Skip and postponement

Mini, long, and rest breaks have independent skip and postponement settings:

```toml
[skip.mini]
enabled = false

[skip.long]
enabled = true

[skip.rest]
enabled = true

[postpone.mini]
enabled = false
duration = "2m"

[postpone.long]
enabled = true
duration = "10m"
max_postponements = 2

[postpone.rest]
enabled = true
duration = "10m"
```

`max_postponements` counts postponements within one break cycle. Omit it for unlimited postponements, which is the default. When an action is disabled or its limit has been reached, its button is omitted and its CLI command returns an error. Disabling skip also prevents pause, reset, or toggle from dismissing an active break early. The policies for each break kind do not affect each other.

### Manual resume

Manual resume keeps the overlay open after the countdown reaches zero. Press any
key or click the overlay when you return, and the next work interval starts from
that confirmation. The setting applies to every break kind:

```toml
[completion]
manual_resume = true
sound = "warm-rise"
```

The bundled sound choices are `warm-rise`, `soft-bloom`, `deep-halo`, and
`clean-chime`. You can select the sound and enable manual resume under Actions in
`hyprbreathe settings`. While the overlay is waiting at zero, it captures the
confirmation even when normal pointer input is set to `controls` or `none`. The
selected sound plays when the countdown reaches zero with either manual or
automatic resume.

### Tray

The StatusNotifierItem tray is enabled by default:

```toml
[tray]
enabled = true
```

The tray shows the current schedule status and provides pause/resume, manual break, skip, postpone, reset, and reload actions. It needs a StatusNotifierItem host, such as Waybar's `tray` module. Disabling the tray does not affect the scheduler or overlays.

### Co-op breaks

Co-op mode lets one host own the schedule while guests mirror its next break,
active break, pause state, and permitted actions. Each computer still renders
its own native overlay with its own monitor and message settings.

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

The host remains authoritative for anything that coordinates the room: cadence,
break duration and kind, pause state, strict/skip/postpone rules, manual resume,
and notification enablement and lead times. A guest's skip, postpone, pause,
resume, reset, or manual-break command is sent to the host, and the resulting
host snapshot is mirrored back. If manual resume is enabled by the host, a key
press or click from any participant resumes the room after the countdown reaches
zero.

Presentation stays local: each participant keeps their own monitor selection,
display mode, opacity, pointer behavior, messages, completion sound, and tray
preference. Both systems should have normal network time synchronization enabled
because scheduled starts use absolute timestamps. If snapshots stop for 10
seconds, the guest starts a fresh local schedule; reconnecting adopts the host
again. Leave at any time with `hyprbreathe coop leave`.

The relay has no database, accounts, schedule engine, or desktop dependencies.
It retains only the latest snapshot while a host is connected. Room tokens are
sent in the WebSocket `Authorization` header rather than the request URL; the
invite keeps the token in its URL fragment. Anyone holding an invite can enter
that room, so share it as a secret and create a new room to rotate it. See
[Co-op deployment and protocol](docs/coop.md) for relay setup and operational
details.

### Pointer and keyboard input

`display.pointer_mode` controls where clicks go during a break:

- `block`: captures clicks across the full overlay. Controls remain clickable. This is the default.
- `controls`: captures clicks on the content panel and passes background clicks through.
- `none`: makes the full overlay click-through.

`display.keyboard_mode` accepts `none`, `on-demand`, or `exclusive`. The default is `on-demand`. Enabling `strict.inhibit_shortcuts` uses exclusive keyboard focus on the content surface while the overlay is visible. Press plain `s` to skip or plain `p` to postpone. A shortcut does nothing when that action is disabled, locked by strict mode, or out of postponements.

## Monitors

Run `hyprbreathe outputs` to list connected monitors and their stable identifiers:

```text
edid:<make>:<model>:<serial>
connector:<name>
```

Use one of these values for `display.mode`:

- `all`: show the full break on every monitor.
- `focused`: show it on the focused monitor.
- `cursor`: show it on the monitor containing the cursor.
- `primary`: use `display.primary_monitor`.
- `configured`: use `display.preferred_monitor`.
- `dim-all-content-one`: dim every monitor and put the message and controls on the monitor selected by `display.content_selector`.

For example:

```toml
[display]
mode = "configured"
preferred_monitor = "connector:DP-1"
fallback = ["focused", "cursor", "primary"]
pointer_mode = "block"
keyboard_mode = "on-demand"
opacity = 0.88
```

If a configured monitor is unavailable, `hyprbreathe` follows the entries in `display.fallback`.

## Commands

```text
hyprbreathe status [--json]
hyprbreathe pause [30m]
hyprbreathe resume
hyprbreathe reset
hyprbreathe skip
hyprbreathe postpone
hyprbreathe mini
hyprbreathe long
hyprbreathe rest
hyprbreathe toggle
hyprbreathe reload
hyprbreathe outputs [--json]
hyprbreathe doctor [--json]
hyprbreathe coop host --relay <ws://-or-wss://-url>
hyprbreathe coop join '<invite>'
hyprbreathe coop status [--json]
hyprbreathe coop leave
hyprbreathe settings
hyprbreathe example-config
```

`skip` and `postpone` follow the active strict-mode and postpone settings.

## Hyprland bindings

Any Hyprland binding can call the CLI. For Hyprland 0.55+ Lua configuration, mark break controls as both `dont_inhibit` and `submap_universal` so they remain available under standard shortcut inhibition and the Hyprland submap fallback. The scheduler still enforces strict mode and per-break action policy:

```lua
local hyprbreathe_flags = { dont_inhibit = true, submap_universal = true }
hl.bind("SUPER + SHIFT + B", hl.dsp.exec_cmd("hyprbreathe toggle"), hyprbreathe_flags)
hl.bind("SUPER + SHIFT + S", hl.dsp.exec_cmd("hyprbreathe skip"), hyprbreathe_flags)
hl.bind("SUPER + SHIFT + P", hl.dsp.exec_cmd("hyprbreathe postpone"), hyprbreathe_flags)
hl.bind("SUPER + SHIFT + M", hl.dsp.exec_cmd("hyprbreathe mini"), hyprbreathe_flags)
hl.bind("SUPER + SHIFT + L", hl.dsp.exec_cmd("hyprbreathe long"), hyprbreathe_flags)
hl.bind("SUPER + SHIFT + R", hl.dsp.exec_cmd("hyprbreathe rest"), hyprbreathe_flags)
```

An optional layer rule disables overlay animations:

```lua
hl.layer_rule({
  match = { namespace = "^hyprbreathe-overlay$" },
  no_anim = true,
})
```

Reload Hyprland after editing its configuration:

```bash
hyprctl reload
hyprctl configerrors
```

## Troubleshooting

Check the daemon, desktop integrations, and monitor detection:

```bash
hyprbreathe doctor
hyprbreathe outputs
systemctl --user status hyprbreathe.service
journalctl --user -u hyprbreathe.service -f
```

If the service cannot see the Wayland or Hyprland environment, import the session variables and restart it:

```bash
systemctl --user import-environment \
  WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE DBUS_SESSION_BUS_ADDRESS
systemctl --user restart hyprbreathe.service
```

## Development instance

Use the development launcher to run the working tree alongside an installed
`hyprbreathe` service:

```bash
# Terminal 1: build and start the isolated development daemon
./scripts/hyprbreathe-dev

# Terminal 2: control that daemon
./scripts/hyprbreathe-dev settings
./scripts/hyprbreathe-dev mini
./scripts/hyprbreathe-dev status --json
```

The launcher builds the debug binary and selects the `hyprbreathe-dev` instance. It
uses separate configuration, state, IPC, GTK, tray, overlay, notification, and
Hyprland identities, so the installed service does not need to be stopped or
restarted. Development settings are stored in
`~/.config/hyprbreathe-dev/config.toml`.

Production and development break overlays can still appear at the same time if
both schedules become due.

## Build from source

Install the build dependencies:

```bash
sudo pacman -S --needed base-devel gtk4 gtk4-layer-shell rust
cargo build --locked --release
```

Install the binary and user service:

```bash
install -Dm755 target/release/hyprbreathe ~/.local/bin/hyprbreathe
install -Dm644 packaging/systemd/hyprbreathe-local.service \
  ~/.config/systemd/user/hyprbreathe.service
install -Dm644 packaging/io.github.PedrobyJoao.hyprbreathe.settings.desktop \
  ~/.local/share/applications/io.github.PedrobyJoao.hyprbreathe.settings.desktop
install -Dm644 crates/platform-linux/assets/*.oga -t ~/.local/share/hyprbreathe
install -Dm644 THIRD_PARTY_NOTICES.md \
  ~/.local/share/licenses/hyprbreathe/THIRD_PARTY_NOTICES.md
install -Dm600 config.example.toml ~/.config/hyprbreathe/config.toml
systemctl --user daemon-reload
systemctl --user enable --now hyprbreathe.service
```
