use clap::Parser;
use log::LevelFilter;
use serde::{Deserialize, Serialize};
use std::os::{linux::net::SocketAddrExt, unix::net::SocketAddr};
use tokio::io::AsyncWriteExt;

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
		.filter_level(LevelFilter::Trace)
		.parse_default_env()
		.init();

	let args = Cli::parse();

	match args {
		Cli::Daemon {
			database_path,
			idle_timeout,
		} => {
			let daemon = daemon::Daemon::new(idle_timeout);
			daemon.run(&database_path).await?;
			Ok(())
		}
		Cli::Action(action) => {
			let mut stream = tokio::net::UnixStream::connect(&SocketAddr::from_abstract_name(
				"dev.r58playz.ktimetracker",
			)?)
			.await?;
			let action_str = serde_json::to_string(&action)?;
			stream.write_all(action_str.as_bytes()).await?;

			let mut stdout = tokio::io::stdout();
			tokio::io::copy(&mut stream, &mut stdout).await?;

			Ok(())
		}
	}
}
