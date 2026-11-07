hyprbreathe is an opinitiated version of breakd with the aim
of being way more strict and have reminders with a mindfulness
theme.

# Repository map

## Application

- `src/main.rs`: CLI commands and application entry point.
- `crates/core/src/lib.rs`: shared configuration, commands,
  responses, break types, and overlay specifications.
- `crates/scheduler/src/lib.rs`: scheduling state machine,
  break policies, recovery, snapshots, and emitted effects.
- `crates/daemon/src/lib.rs`: runtime event loop, integration
  wiring, command handling, and overlay supervision.
- `crates/ipc/src/lib.rs`: Unix-socket client/server protocol.

## Configuration and UI

- `crates/config/src/lib.rs`: defaults, TOML loading/saving,
  validation, runtime identities, and config/state/runtime paths.
- `crates/config/src/messages.rs`: built-in break messages.
- `config.example.toml`: example user configuration.
- `crates/wayland-overlay/src/lib.rs`: GTK layer-shell break
  overlays, monitor selection, and break controls.
- `crates/settings/src/lib.rs`: GTK settings window.
- `crates/tray/src/lib.rs`: system tray menu and actions.

## Linux integrations

Files below are in `crates/platform-linux/src/`:

- `hyprland.rs`: monitors, fullscreen state, and keybindings.
- `idle.rs`: Wayland idle monitoring.
- `logind.rs`: system sleep and session lock events.
- `clock.rs`: Linux clocks and boot identity.
- `state_store.rs`: scheduler snapshot persistence.
- `notifications.rs`: desktop notifications.
- `event_sound.rs`: completion sound playback.
- `wayland_probe.rs`: Wayland protocol discovery.

Sound assets: `crates/platform-linux/assets/`.

## Co-op breaks

- `crates/coop/src/lib.rs`: shared protocol, invites, and policy.
- `crates/daemon/src/coop.rs`: daemon-side relay connection
  and synchronized break handling.
- `crates/relay/src/main.rs`: WebSocket room relay server.
- `docs/coop.md`: co-op usage and deployment documentation.

## Development and distribution

- Tests: inline `#[cfg(test)]` modules in Rust source files.
- `scripts/hyprbreathe-dev`: runs a separate development instance.
- `.github/workflows/ci.yml`: formatting, lint, test, and build
  checks for Rust and Nix.
- `packaging/nix/`: application/relay packages and NixOS module.
- `packaging/systemd/`: user service units.
- `packaging/arch/PKGBUILD`: Arch Linux packaging.
- `packaging/arch/.SRCINFO`: Arch package metadata.
- `packaging/*.desktop`: desktop launcher.
- `scripts/`: source archives, Arch builds, AUR publishing,
  and release lockfile synchronization.
- `.github/workflows/release-please.yml`: release automation.
- `release-please-config.json`: release/version configuration.
- `.release-please-manifest.json`: tracked release versions.
