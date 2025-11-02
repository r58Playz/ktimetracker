use tokio::sync::mpsc;

use anyhow::Result;
use log::{debug, error, info};

use crate::{
    kactivities::KActivitiesConnection, systemd::SystemdConnection, wayland::WaylandConnection,
};

pub enum DaemonEvent {
    KdeActivityChanged { activity: String },
    IdleStatusChanged { idle: bool },
    SleepingNow,
    WakingNow,
}

pub struct Daemon {
    event_tx: mpsc::UnboundedSender<DaemonEvent>,
    event_rx: mpsc::UnboundedReceiver<DaemonEvent>,
    idle_duration: u32,
}

impl Daemon {
    pub fn new(idle_duration: u32) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            event_tx,
            event_rx,
            idle_duration,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("starting daemon");

        let kactivities_conn = KActivitiesConnection::new(self.event_tx.clone()).await?;

        tokio::spawn({
            let wayland_tx = self.event_tx.clone();
            async move {
                if let Err(e) = WaylandConnection::daemon(wayland_tx, self.idle_duration).await {
                    error!("wayland listener failed: {e}");
                }
            }
        });

        tokio::spawn({
            let systemd_conn = SystemdConnection::new(self.event_tx.clone()).await?;
            async move {
                if let Err(e) = systemd_conn.daemon().await {
                    error!("systemd connection failed: {e}");
                }
            }
        });

        loop {
            match self.event_rx.recv().await {
                Some(DaemonEvent::KdeActivityChanged { activity }) => {
                    let activity_info = kactivities_conn
                        .query_activity_info(activity.clone())
                        .await?;
                    debug!(
                        "kde activity changed to {activity} ({})",
                        activity_info.name
                    );
                }
                Some(DaemonEvent::IdleStatusChanged { idle }) => {
                    debug!("idle status changed: {idle}");
                }
                Some(DaemonEvent::SleepingNow) => {
                    debug!("sleeping now");
                }
                Some(DaemonEvent::WakingNow) => {
                    debug!("waking now");
                }
                None => {
                    break;
                }
            }
        }

        Ok(())
    }
}
