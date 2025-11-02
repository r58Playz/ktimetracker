use clap::{Parser, Subcommand};
use log::LevelFilter;

mod daemon;
mod wayland;
mod kactivities;
mod systemd;
mod db;

#[derive(Debug, Parser)]
/// KDE Activities and Wayland idle protocol based time tracking tool
struct Args {
    #[command(subcommand)]
    command: Cli,
}

#[derive(Debug, Subcommand)]
enum Cli {
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

    let args = Args::parse();

    match args.command {
        Cli::Daemon { database_path, idle_timeout } => {
            let daemon = daemon::Daemon::new(idle_timeout);
            daemon.run(&database_path).await?;
            Ok(())
        }
    }
}
