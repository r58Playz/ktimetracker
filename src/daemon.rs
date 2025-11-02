use tokio::sync::mpsc;

use anyhow::Result;
use log::{debug, error, info, trace};
use std::sync::Arc;
use tokio::signal;

use crate::{
    db::Database, kactivities::KActivitiesConnection, systemd::SystemdConnection,
    wayland::WaylandConnection,
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

    pub async fn run(mut self, database_path: &str) -> Result<()> {
        info!("starting daemon");

        let db = Arc::new(Database::new(database_path).await?);
        let kactivities_conn = KActivitiesConnection::new(self.event_tx.clone()).await?;

        let mut signal_handle = tokio::spawn({
            let db_clone = db.clone();
            async move {
                let mut sigterm =
                    signal::unix::signal(signal::unix::SignalKind::terminate()).unwrap();
                let mut sigint =
                    signal::unix::signal(signal::unix::SignalKind::interrupt()).unwrap();
                tokio::select! {
                    _ = sigterm.recv() => {},
                    _ = sigint.recv() => {},
                };
                trace!("got signal, saving state");
                db_clone.end_current_activity().await
            }
        });

        let initial_activity = kactivities_conn.query_current_activity().await?;
        db.switch_activity(&initial_activity).await?;
        trace!("kde activity changed to {initial_activity}");

        let mut wayland_handle = tokio::spawn(WaylandConnection::daemon(
            self.event_tx.clone(),
            self.idle_duration,
        ));

        let mut systemd_handle = tokio::spawn(
            SystemdConnection::new(self.event_tx.clone())
                .await?
                .daemon(),
        );

        loop {
            tokio::select! {
                res = &mut signal_handle => {
                    debug!("terminating due to signal, save result {res:?}");
                    break;
                },
                res = &mut wayland_handle => {
                    error!("wayland task exited with: {res:?}");
                    break;
                },
                res = &mut systemd_handle => {
                    error!("systemd task exited with: {res:?}");
                    break;
                },
                event = self.event_rx.recv() => {
                    match event {
                        Some(DaemonEvent::KdeActivityChanged { activity }) => {
                            trace!("activity changed to {activity}");
                            db.switch_activity(&activity).await?;
                        }
                        Some(DaemonEvent::IdleStatusChanged { idle }) => {
                            if idle {
                                trace!("ending current activity: now idle");
                                db.end_current_activity().await?;
                            } else {
                                let activity = kactivities_conn.query_current_activity().await?;
                                trace!("starting activity {activity}: no longer idle");
                                db.switch_activity(&activity).await?;
                            }
                        }
                        Some(DaemonEvent::SleepingNow) => {
                            trace!("ending current activity: now going to sleep");
                            db.end_current_activity().await?;
                        }
                        Some(DaemonEvent::WakingNow) => {
                            let activity = kactivities_conn.query_current_activity().await?;
                            trace!("stating activity {activity}: no longer asleep");
                            db.switch_activity(&activity).await?;
                        }
                        None => {
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
