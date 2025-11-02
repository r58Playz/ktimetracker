use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::sync::mpsc;
use zbus::{Connection, proxy};

use crate::daemon::DaemonEvent;

#[proxy(
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1",
    interface = "org.freedesktop.login1.Manager"
)]
trait SystemdLogin1 {
    #[zbus(signal)]
	fn prepare_for_sleep(&self, sleeping: bool) -> zbus::Result<()>;
}

pub struct SystemdConnection {
    conn: Connection,
    daemon: mpsc::UnboundedSender<DaemonEvent>,
}
impl SystemdConnection {
    pub async fn new(daemon: mpsc::UnboundedSender<DaemonEvent>) -> Result<Self> {
        let conn = Connection::system()
            .await
            .context("failed to connect to d-bus system bus")?;

        Ok(Self { conn, daemon })
    }

    pub async fn daemon(self) -> Result<()> {
		let proxy = SystemdLogin1Proxy::new(&self.conn).await.context("failed to bind to systemd login1")?;

		let mut sleep = proxy.receive_prepare_for_sleep().await.context("failed to bind to prepare for sleep signal")?;

		while let Some(signal) = sleep.next().await {
			let sleeping = signal.args().context("failed to parse message")?.sleeping;

			if sleeping {
				self.daemon.send(DaemonEvent::SleepingNow).context("failed to send message to daemon")?;
			} else {
				self.daemon.send(DaemonEvent::WakingNow).context("failed to send message to daemon")?;
			}
		}

		Ok(())
	}
}
