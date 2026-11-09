use std::time::Duration;

use futures_util::StreamExt;
use tokio::{sync::mpsc, time};
use zbus::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    PreparingForSleep,
    Resumed,
}

#[zbus::proxy(
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1",
    interface = "org.freedesktop.login1.Manager"
)]
trait LoginManager {
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

pub fn spawn_logind_monitor(sender: mpsc::Sender<PowerEvent>) {
    tokio::spawn(async move {
        loop {
            let result = monitor_sleep(&sender).await;
            if sender.is_closed() {
                break;
            }
            if let Err(error) = result {
                tracing::warn!(%error, "sleep tracking unavailable");
            }
            if sender.send(PowerEvent::Resumed).await.is_err() {
                break;
            }
            time::sleep(Duration::from_secs(5)).await;
        }
    });
}

async fn monitor_sleep(sender: &mpsc::Sender<PowerEvent>) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let manager = LoginManagerProxy::new(&connection).await?;
    let mut events = manager.receive_prepare_for_sleep().await?;
    tracing::info!("sleep tracking available");
    // A live daemon is awake; discard an unverified old sleep flag.
    if sender.send(PowerEvent::Resumed).await.is_err() {
        return Ok(());
    }
    while let Some(signal) = events.next().await {
        let event = match signal.args() {
            Ok(args) if args.start => PowerEvent::PreparingForSleep,
            Ok(_) => PowerEvent::Resumed,
            Err(error) => {
                tracing::warn!(%error, "invalid sleep signal");
                continue;
            }
        };
        if sender.send(event).await.is_err() {
            return Ok(());
        }
    }
    Ok(())
}
