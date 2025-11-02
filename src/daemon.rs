use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::UnixListener,
	sync::mpsc,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate, TimeZone};
use log::{debug, error, info, trace};
use std::sync::Arc;
use tokio::{signal, task::JoinHandle};

use crate::{
	Action, db::Database, kactivities::KActivitiesConnection, systemd::SystemdConnection,
	wayland::WaylandConnection,
};
use serde_json;

fn format_duration(duration: chrono::Duration) -> String {
	let mut parts = Vec::new();
	let hours = duration.num_hours();
	if hours > 0 {
		parts.push(format!("{}h", hours));
	}
	let minutes = duration.num_minutes() % 60;
	if minutes > 0 {
		parts.push(format!("{}m", minutes));
	}
	let seconds = duration.num_seconds() % 60;
	if seconds > 0 {
		parts.push(format!("{}s", seconds));
	}
	if parts.is_empty() {
		return "0s".to_string();
	}
	parts.join(" ")
}

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

macro_rules! swrite {
	($stream:expr, $arg:ident) => {
		$stream.write_all($arg.as_bytes()).await
	};
	($stream:expr, $($arg:tt)*) => {
		$stream.write_all(format!($($arg)*).as_bytes()).await
	};
}

fn parse_datetime(s: String) -> anyhow::Result<DateTime<Local>> {
	// Try different formats
	if let Ok(dt) = DateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S") {
		return Ok(dt.with_timezone(&Local));
	}
	if let Ok(dt) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
		return Ok(Local
			.from_local_datetime(&dt.and_hms_opt(0, 0, 0).unwrap())
			.unwrap());
	}
	if let Ok(dt) = NaiveDate::parse_from_str(&s, "%d/%m/%Y") {
		return Ok(Local
			.from_local_datetime(&dt.and_hms_opt(0, 0, 0).unwrap())
			.unwrap());
	}
	Err(anyhow::anyhow!("Invalid date format"))
}

async fn handle_unix_client(
	stream: &mut tokio::net::UnixStream,
	db: Arc<Database>,
	kactivities_conn: KActivitiesConnection,
) -> Result<()> {
	let mut buf = Vec::new();
	stream.read_to_end(&mut buf).await?;
	let action: Action = serde_json::from_slice(&buf).context("Failed to deserialize action")?;

	match action {
		Action::Summary {
			start_time,
			end_time,
		} => {
			let start = start_time
				.map(|s| parse_datetime(s))
				.transpose()
				.context("Failed to parse start_time")?;
			let end = end_time
				.map(|s| parse_datetime(s))
				.transpose()
				.context("Failed to parse end_time")?;

			let summary = db.get_summary(start, end).await?;

			let mut max_activity_len = "Activity".len();
			let mut max_duration_len = "Duration".len();

			let mut resolved_summary = Vec::new();
			for (activity_uuid, duration) in summary {
				let activity_info = kactivities_conn
					.query_activity_info(activity_uuid.clone())
					.await?;
				let activity_name = if activity_info.name.is_empty() {
					activity_uuid
				} else {
					activity_info.name
				};
				resolved_summary.push((activity_name, format_duration(duration)));
			}

			for (activity, duration) in &resolved_summary {
				max_activity_len = max_activity_len.max(activity.len());
				max_duration_len = max_duration_len.max(duration.len());
			}

			let separator = format!("{:->max_activity_len$}-+{:->max_duration_len$}\n", "", "");

			swrite!(stream, separator)?;
			swrite!(
				stream,
				"{:<max_activity_len$} | {:<max_duration_len$}\n",
				"Activity",
				"Duration"
			)?;
			swrite!(stream, separator)?;

			for (activity, duration) in resolved_summary {
				swrite!(
					stream,
					"{:<max_activity_len$} | {:<max_duration_len$}\n",
					activity,
					duration
				)?;
			}
			swrite!(stream, separator)?;
		}
		Action::Current => {
			let current_uuid = db.get_current_activity().await?;
			let elapsed_time = db.get_current_activity_elapsed_time().await?;

			let activity_info = kactivities_conn
				.query_activity_info(current_uuid.clone())
				.await?;
			let (name, description) = if activity_info.name.is_empty() {
				(current_uuid, String::new())
			} else {
				(activity_info.name, activity_info.description)
			};

			swrite!(
				stream,
				"Current Activity: {}\nDescription: {}\nElapsed Time: {}\n",
				name,
				description,
				elapsed_time.map_or("N/A".to_string(), format_duration)
			)?;
		}
	}
	Ok(())
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

		let listener = UnixListener::bind("\0dev.r58playz.ktimetracker")?;
		let mut unix_socket_handle: JoinHandle<Result<()>> = tokio::spawn({
			let db = db.clone();
			let kactivities_conn = kactivities_conn.clone();
			async move {
				loop {
					let (mut stream, _addr) = listener.accept().await?;
					let db = db.clone();
					let kactivities_conn = kactivities_conn.clone();
					tokio::spawn(async move {
						if let Err(e) = handle_unix_client(&mut stream, db, kactivities_conn).await
						{
							error!("error handling unix client: {e}");
							let _ = stream.write_all(format!("Error: {e}\n").as_bytes()).await;
						}
					});
				}
			}
		});

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
				res = &mut unix_socket_handle => {
					error!("unix socket task exited with: {res:?}");
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
