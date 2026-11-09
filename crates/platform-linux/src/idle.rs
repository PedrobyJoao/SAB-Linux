use std::time::Duration;

use tokio::{sync::watch, time};

use crate::HyprlandClient;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DesktopState {
    pub displays_off: Option<bool>,
    pub locked: Option<bool>,
}

impl DesktopState {
    fn availability(self) -> (bool, bool) {
        (self.displays_off.is_some(), self.locked.is_some())
    }
}

/// Unknown power or lock state is not evidence of idle time.
pub async fn spawn_desktop_monitor() -> watch::Receiver<DesktopState> {
    let (sender, receiver) = watch::channel(DesktopState::default());
    let client = match HyprlandClient::from_env() {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "desktop tracking unavailable");
            return receiver;
        }
    };
    let initial = desktop_state(&client).await;
    report_availability(initial);
    sender.send_replace(initial);
    tokio::spawn(async move {
        let mut ticker = time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        let mut available = initial.availability();
        loop {
            tokio::select! {
                _ = sender.closed() => break,
                _ = ticker.tick() => {
                    let state = desktop_state(&client).await;
                    if state.availability() != available {
                        available = state.availability();
                        report_availability(state);
                    }
                    sender.send_if_modified(|previous| {
                        if *previous == state {
                            return false;
                        }
                        *previous = state;
                        true
                    });
                }
            }
        }
    });
    receiver
}

fn report_availability(state: DesktopState) {
    if state.displays_off.is_none() || state.locked.is_none() {
        tracing::warn!(
            power_available = state.displays_off.is_some(),
            lock_available = state.locked.is_some(),
            "desktop tracking partially or fully unavailable",
        );
    } else {
        tracing::info!("desktop power and lock tracking available");
    }
}

async fn desktop_state(client: &HyprlandClient) -> DesktopState {
    let timeout = Duration::from_secs(2);
    let (power, lock) = tokio::join!(
        time::timeout(timeout, client.displays_off()),
        time::timeout(timeout, client.locked()),
    );
    DesktopState {
        displays_off: power.ok().and_then(Result::ok),
        locked: lock.ok().and_then(Result::ok),
    }
}
