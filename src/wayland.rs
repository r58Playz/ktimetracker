use std::{ffi::CString, sync::mpsc::Sender};

use anyhow::{Context, Result};
use log::info;
use wayrs_client::{Connection, IoMode, protocol::WlSeat};
use wayrs_protocols::ext_idle_notify_v1::{ExtIdleNotifierV1, ext_idle_notification_v1::Event};
use wayrs_utils::seats::{SeatHandler, Seats};

use crate::daemon::DaemonEvent;

pub struct WaylandConnection {
    seats: Seats,
    seat_names: Vec<(CString, WlSeat)>,
    sender: Sender<DaemonEvent>,
}
impl WaylandConnection {
    pub fn daemon(sender: Sender<DaemonEvent>, idle_timeout: u32) -> Result<()> {
        let mut conn = Connection::connect().context("failed to connect to wayland server")?;
        let mut this = Self {
            seat_names: Vec::new(),
            seats: Seats::new(&mut conn),
            sender,
        };

        // receive seats
        conn.blocking_roundtrip().context("roundtrip failed")?;
        conn.dispatch_events(&mut this);

        // receive seat names
        conn.blocking_roundtrip().context("roundtrip failed")?;
        conn.dispatch_events(&mut this);

        let idle = conn
            .bind_singleton::<ExtIdleNotifierV1>(2..=2)
            .context("failed to bind to ext_idle_notify_v1 version 2")?;

        let (seat_name, seat) = this.seat_names.first().context("no wayland seats found")?;

        info!("chose wayland seat {seat_name:?} for idle notifications");
        idle.get_input_idle_notification_with_cb(&mut conn, idle_timeout, *seat, |ctx| {
            ctx.state.idle_event(ctx.event);
        });

		loop {
			conn.flush(IoMode::Blocking).context("failed to flush wayland connection")?;
			conn.recv_events(IoMode::Blocking).context("failed to recv wayland events")?;
			conn.dispatch_events(&mut this);
		}
    }

    fn idle_event(&mut self, event: Event) {
        match event {
            Event::Idled => {
                let _ = self
                    .sender
                    .send(DaemonEvent::IdleStatusChanged { idle: true });
            }
            Event::Resumed => {
                let _ = self
                    .sender
                    .send(DaemonEvent::IdleStatusChanged { idle: false });
            }
            _ => {}
        }
    }
}

impl SeatHandler for WaylandConnection {
    fn get_seats(&mut self) -> &mut Seats {
        &mut self.seats
    }
    fn seat_name(&mut self, _: &mut Connection<Self>, seat: WlSeat, name: std::ffi::CString) {
        self.seat_names.push((name, seat));
    }
}
