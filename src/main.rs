use clap::Parser;
use log::LevelFilter;

mod daemon;
mod wayland;
mod kactivities;
mod systemd;

#[derive(Debug, Parser)]
/// KDE Activities and Wayland idle protocol based time tracking tool
enum Cli {
    /// Run daemon
    Daemon {},
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(LevelFilter::Trace)
        .parse_default_env()
        .init();

    let args = Cli::parse();

	match args {
		Cli::Daemon {  } => {
            let daemon = daemon::Daemon::new(5000);
            daemon.run().await?;
			Ok(())
		}
	}
}
