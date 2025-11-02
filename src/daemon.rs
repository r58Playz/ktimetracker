use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use anyhow::Result;
use log::{debug, error, info};

use crate::{kactivities::KActivitiesConnection, wayland::WaylandConnection};

pub enum DaemonEvent {
    KdeActivityChanged { activity: String },
    IdleStatusChanged { idle: bool },
}

pub struct Daemon {
    event_tx: Sender<DaemonEvent>,
    event_rx: Receiver<DaemonEvent>,
    idle_duration: u32,
}

impl Daemon {
    pub fn new(idle_duration: u32) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self { event_tx, event_rx, idle_duration }
    }

    pub fn run(self) -> Result<()> {
        info!("starting daemon");

        let kactivities_conn = KActivitiesConnection::new()?;
        kactivities_conn.listen_for_activity_change(self.event_tx.clone())?;

        let wayland_tx = self.event_tx.clone();
        thread::spawn(move || {
            if let Err(e) = WaylandConnection::daemon(wayland_tx, self.idle_duration) {
                error!("wayland listener failed: {e}");
            }
        });

        loop {
            match self.event_rx.recv() {
                Ok(event) => match event {
                    DaemonEvent::KdeActivityChanged { activity } => {
                        debug!("kde activity changed to {activity}");
                    }
                    DaemonEvent::IdleStatusChanged { idle } => {
                        debug!("idle status changed: {idle}");
                    }
                },
                Err(e) => {
                    error!("failed to receive daemon event: {e}");
                    break;
                }
            }
        }

        Ok(())
    }
}

