use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::{
    select,
    sync::{mpsc, oneshot},
};
use zbus::{Connection, proxy};
use log::error;

use crate::daemon::DaemonEvent;

#[proxy(
    default_service = "org.kde.ActivityManager",
    default_path = "/ActivityManager/Activities",
    interface = "org.kde.ActivityManager.Activities"
)]
trait KdeActivityManager {
    #[zbus(signal)]
    fn current_activity_changed(&self, activity: String) -> zbus::Result<()>;

    fn current_activity(&self) -> zbus::Result<String>;

    fn activity_name(&self, activity: &str) -> zbus::Result<String>;
    fn activity_description(&self, activity: &str) -> zbus::Result<String>;
}

enum KActivitiesMessage {
    CurrentActivity(oneshot::Sender<Result<String>>),
    ActivityInfo(String, oneshot::Sender<Result<ActivityInfo>>),
    ActivityChanged(String),
}

#[derive(Debug)]
pub struct ActivityInfo {
    pub name: String,
    pub description: String,
}

pub struct KActivitiesConnection {
    actor: mpsc::UnboundedSender<KActivitiesMessage>,
}

impl Clone for KActivitiesConnection {
    fn clone(&self) -> Self {
        Self { actor: self.actor.clone() }
    }
}

impl KActivitiesConnection {
    pub async fn new(daemon: mpsc::UnboundedSender<DaemonEvent>) -> Result<Self> {
        let conn = Connection::session()
            .await
            .context("failed to connect to d-bus session bus")?;

        let (actor, actor_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
			if let Err(e) = Self::daemon(conn, actor_rx, daemon).await {
				error!("kde activities connection failed: {e}");
			}
		});

        Ok(Self { actor })
    }

    pub async fn query_current_activity(&self) -> Result<String> {
        let (tx, rx) = oneshot::channel();

        self.actor
            .send(KActivitiesMessage::CurrentActivity(tx))
            .context("failed to send request to actor")?;

        rx.await
            .context("failed to get result from actor")
            .flatten()
    }

    pub async fn query_activity_info(&self, activity: String) -> Result<ActivityInfo> {
        let (tx, rx) = oneshot::channel();

        self.actor
            .send(KActivitiesMessage::ActivityInfo(activity, tx))
            .context("failed to send request to actor")?;

        rx.await
            .context("failed to get result from actor")
            .flatten()
    }

    async fn daemon(
        conn: Connection,
        mut rx: mpsc::UnboundedReceiver<KActivitiesMessage>,
        daemon: mpsc::UnboundedSender<DaemonEvent>,
    ) -> Result<()> {
        let proxy = KdeActivityManagerProxy::new(&conn)
            .await
            .context("failed to bind to kde activity manager")?;
        let mut activity_signal = proxy
            .receive_current_activity_changed()
            .await
            .context("failed to bind to current activity changed signal")?;

        loop {
            match select! {
                x = activity_signal.next() => {
                    x.map(|x| anyhow::Ok(KActivitiesMessage::ActivityChanged(x.args().context("failed to parse signal")?.activity))).transpose()?
                },
                x = rx.recv() => x
            } {
                Some(KActivitiesMessage::ActivityChanged(activity)) => {
                    daemon
                        .send(DaemonEvent::KdeActivityChanged { activity })
                        .context("failed to send activity changed to daemon")?;
                }
                Some(KActivitiesMessage::CurrentActivity(tx)) => {
                    let _ = tx.send(
                        proxy
                            .current_activity()
                            .await
                            .context("failed to get current activity"),
                    );
                }
                Some(KActivitiesMessage::ActivityInfo(activity, tx)) => {
                    let ret = async {
                        let name = proxy
                            .activity_name(&activity)
                            .await
                            .context("failed to get activity name")?;
                        let description = proxy
                            .activity_description(&activity)
                            .await
                            .context("failed to get activity description")?;

                        anyhow::Ok(ActivityInfo { name, description })
                    }
                    .await;

                    let _ = tx.send(ret);
                }
                None => break Ok(()),
            }
        }
    }
}
