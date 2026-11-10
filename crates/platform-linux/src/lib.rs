mod clock;
mod event_sound;
mod hyprland;
mod idle;
mod logind;
mod notifications;
mod state_store;
mod wayland_probe;

pub use clock::{ClockError, LinuxClock};
pub use event_sound::{EventSoundClient, EventSoundError};
pub use hyprland::{HyprlandClient, HyprlandError};
pub use idle::{DesktopState, spawn_desktop_monitor};
pub use logind::{PowerEvent, request_poweroff, spawn_logind_monitor};
pub use notifications::NotificationClient;
pub use state_store::{StateStore, StateStoreError};
pub use wayland_probe::{WaylandGlobal, WaylandProbeError, probe_wayland_globals};
