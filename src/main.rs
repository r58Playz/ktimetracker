use clap::Parser;
use log::LevelFilter;
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, net::UnixStream};

use crate::daemon::Daemon;

mod daemon;
mod db;
mod kactivities;
mod systemd;
mod wayland;

#[derive(Parser, Debug, Serialize, Deserialize)]
pub enum Action {
	/// Print summary of time spent
	Summary {
		start_time: Option<String>,
		end_time: Option<String>,
	},
	/// Print current session
	Current,
}

#[derive(Debug, Parser)]
enum Cli {
	#[clap(flatten)]
	Action(Action),
	/// Run daemon
	Daemon {
		#[arg(long, default_value = "~/.local/share/ktimetracker.db3")]
		database_path: String,
		#[arg(long, default_value_t = 5000)]
		idle_timeout: u32,
	},
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	env_logger::builder()
		.filter_level(LevelFilter::Off)
		.filter_module("ktimetracker", LevelFilter::Debug)
		.parse_default_env()
		.init();

	let args = Cli::parse();

	match args {
		Cli::Daemon {
			database_path,
			idle_timeout,
		} => {
			let daemon = Daemon::new(idle_timeout);
			daemon.run(&database_path).await?;
			Ok(())
		}
		Cli::Action(action) => {
			let (mut rx, mut tx) = UnixStream::connect("\0dev.r58playz.ktimetracker")
				.await?
				.into_split();
			let action_str = serde_json::to_string(&action)?;
			tx.write_all(action_str.as_bytes()).await?;
			tx.shutdown().await?;

			let mut stdout = tokio::io::stdout();
			tokio::io::copy(&mut rx, &mut stdout).await?;

			Ok(())
		}
	}
}
