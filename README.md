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

The settings window covers scheduling, break actions, strict mode,
display behavior, collaboration, idle recovery, and tray visibility.
It validates changes before saving and reloads the running daemon.
Advanced monitor, recovery, message, and logging options remain
available in TOML:

```bash
mkdir -p ~/.config/hyprbreathe
hyprbreathe example-config > ~/.config/hyprbreathe/config.toml
$EDITOR ~/.config/hyprbreathe/config.toml
hyprbreathe reload
```

The default screen-time intervals are 10 minutes for a 20-second Mini,
30 minutes for a 5-minute Long, and 2 hours for a 30-minute Rest.
Durations accept values such as `20s`, `7m 30s`, `1h`, and `1h 15m`.

Most changes, including `[idle]`, take effect after
`hyprbreathe reload`. Restart the service after changing `[logging]`:

```bash
systemctl --user restart hyprbreathe.service
```

### Screen time and idle recovery

Mini, Long, and Rest each have an accumulated screen-time counter.
While a display is on and the session is unlocked, each counter
increases one second per second. Reading and watching movies count
as screen time, whereas keyboard and mouse inactivity do not imply idle.

### Strict mode

Set `strict.mode` to one of these values:

- `off`: postponement is available immediately.
- `delay`: postponement is available immediately, while
  shortcut inhibition can still be enabled.
- `entire`: postponement is locked for the whole break
  (`allow_postpone_during_lockout` still permits postponing).

Set `strict.inhibit_shortcuts = true` to request standard Wayland shortcut inhibition while the overlay is active. With `hyprland.submap_fallback = true`, the daemon also enters a temporary Hyprland `hyprbreathe` submap because layer surfaces do not reliably suppress compositor bindings on every Hyprland version. The submap is registered at runtime, checked throughout the break, and reset when the break or daemon exits.

This blocks normal Hyprland bindings, including the `SUPER+1..9` workspace bindings. It does not change or unbind them. If the daemon is forcibly killed, its systemd restart resets a stale `hyprbreathe` submap. The manual recovery command is `hyprctl dispatch submap reset`.

### Postponement

Mini, long, and rest breaks have independent postponement settings:

```toml
[postpone.mini]
enabled = false
duration = "2m"

[postpone.long]
enabled = true
duration = "10m"

[postpone.rest]
enabled = true
duration = "10m"
```

Each break can be postponed at most once per cycle. The **Postpone once** button is
omitted when postponement is disabled or already used.
The next break cycle allows postponement
again.

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

The tray shows the current schedule status and provides manual break,
postpone, resume-work, and reload actions. It needs a
StatusNotifierItem host, such as Waybar's `tray` module. Disabling
the tray does not affect the scheduler or overlays.

### Pointer and keyboard input

`display.pointer_mode` controls where clicks go during a break:

- `block`: captures clicks across the full overlay. Controls remain clickable. This is the default.
- `controls`: captures clicks on the content panel and passes background clicks through.
- `none`: makes the full overlay click-through.

`display.keyboard_mode` accepts `none`, `on-demand`, or `exclusive`.
The default is `on-demand`. Enabling `strict.inhibit_shortcuts` uses
exclusive keyboard focus on the content surface while the overlay is
visible. Press plain `p` to postpone. The shortcut does nothing when
postponement is disabled, locked by strict mode, or out of postponements.

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
```

If a configured monitor is unavailable, `hyprbreathe` follows the entries in `display.fallback`.

## Commands

```text
hyprbreathe status [--json]
hyprbreathe postpone
hyprbreathe mini
hyprbreathe long
hyprbreathe rest
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

`postpone` follows the active strict-mode and postpone settings.

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
