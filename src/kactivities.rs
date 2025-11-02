use std::{sync::mpsc::Sender, time::Duration};

use anyhow::{Context, Result};
use dbus::{Message, blocking::Connection};

use crate::daemon::DaemonEvent;

const BUS_NAME: &str = "org.kde.ActivityManager";
const PATH: &str = "/ActivityManager/Activities";
const INTERFACE: &str = "org.kde.ActivityManager.Activities";

#[derive(Debug)]
pub struct OrgKdeActivityManagerActivitiesCurrentActivityChanged {
    pub activity: String,
}

impl dbus::arg::AppendAll for OrgKdeActivityManagerActivitiesCurrentActivityChanged {
    fn append(&self, i: &mut dbus::arg::IterAppend) {
        dbus::arg::RefArg::append(&self.activity, i);
    }
}

impl dbus::arg::ReadAll for OrgKdeActivityManagerActivitiesCurrentActivityChanged {
    fn read(i: &mut dbus::arg::Iter) -> Result<Self, dbus::arg::TypeMismatchError> {
        Ok(OrgKdeActivityManagerActivitiesCurrentActivityChanged {
            activity: i.read()?,
        })
    }
}

impl dbus::message::SignalArgs for OrgKdeActivityManagerActivitiesCurrentActivityChanged {
    const NAME: &'static str = "CurrentActivityChanged";
    const INTERFACE: &'static str = "org.kde.ActivityManager.Activities";
}

#[derive(Debug)]
pub struct ActivityInfo {
    pub name: String,
    pub description: String,
}

pub struct KActivitiesListener {
    conn: Connection,
}
impl KActivitiesListener {
    pub fn new() -> Result<Self> {
        let conn = Connection::new_session().context("failed to connect to d-bus session bus")?;
        Ok(Self { conn })
    }

    pub fn listen_for_activity_change(&self, sender: Sender<DaemonEvent>) -> Result<()> {
        self.conn
            .with_proxy(BUS_NAME, PATH, Duration::from_secs(1))
            .match_signal(Box::new(
                move |message: OrgKdeActivityManagerActivitiesCurrentActivityChanged,
                      _: &Connection,
                      _: &Message| {
                    sender
                        .send(DaemonEvent::KdeActivityChanged {
                            activity: message.activity,
                        })
                        .is_ok()
                },
            ))
            .map(|_| ())
            .context("failed to match signal")?;
        Ok(())
    }

    pub fn daemon(self) -> Result<()> {
        loop {
            self.conn
                .process(Duration::from_secs(60))
                .context("failed to process incoming d-bus messages")?;
        }
    }
}

pub struct KActivitiesConnection {
    conn: Connection,
}
impl KActivitiesConnection {
    pub fn new() -> Result<Self> {
        let conn = Connection::new_session().context("failed to connect to d-bus session bus")?;
        Ok(Self { conn })
    }

    pub fn query_current_activity(&self) -> Result<String> {
        let proxy = self.conn.with_proxy(BUS_NAME, PATH, Duration::from_secs(1));

        let (activity,): (String,) = proxy
            .method_call(INTERFACE, "CurrentActivity", ())
            .context("failed to get current activity")?;

        Ok(activity)
    }

    pub fn query_activity_info(&self, activity: &str) -> Result<ActivityInfo> {
        let proxy = self.conn.with_proxy(BUS_NAME, PATH, Duration::from_secs(1));

        let (name,): (String,) = proxy
            .method_call(INTERFACE, "ActivityName", (activity,))
            .context("failed to get activity name")?;

        let (description,): (String,) = proxy
            .method_call(INTERFACE, "ActivityDescription", (activity,))
            .context("failed to get activity description")?;

        Ok(ActivityInfo {
            name,
            description,
        })
    }
}
