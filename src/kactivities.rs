use std::{
    sync::mpsc::Sender,
    time::Duration
};

use anyhow::{Context, Result};
use dbus::{Message, blocking::Connection};
use log::info;

use crate::daemon::DaemonEvent;

const BUS_NAME: &str = "org.kde.ActivityManager";
const PATH: &str = "/ActivityManager/Activities";
const INTERFACE: &str = "org.kde.ActivityManager.Activities";

#[derive(Debug)]
pub struct OrgKdeActivityManagerActivitiesActivityChanged {
    pub activity: String,
}

impl dbus::arg::AppendAll for OrgKdeActivityManagerActivitiesActivityChanged {
    fn append(&self, i: &mut dbus::arg::IterAppend) {
        dbus::arg::RefArg::append(&self.activity, i);
    }
}

impl dbus::arg::ReadAll for OrgKdeActivityManagerActivitiesActivityChanged {
    fn read(i: &mut dbus::arg::Iter) -> Result<Self, dbus::arg::TypeMismatchError> {
        Ok(OrgKdeActivityManagerActivitiesActivityChanged {
            activity: i.read()?,
        })
    }
}

impl dbus::message::SignalArgs for OrgKdeActivityManagerActivitiesActivityChanged {
    const NAME: &'static str = "ActivityChanged";
    const INTERFACE: &'static str = "org.kde.ActivityManager.Activities";
}

#[derive(Debug)]
pub struct ActivityInfo {
    name: String,
    description: String,
    icon: String,
}

pub struct KActivitiesConnection {
    conn: Connection,
}
impl KActivitiesConnection {
    pub fn new() -> Result<Self> {
        let conn = Connection::new_session().context("failed to connect to d-bus session bus")?;
        Ok(Self { conn })
    }

    pub fn listen_for_activity_change(&self, sender: Sender<DaemonEvent>) -> Result<()> {
        self.conn
            .with_proxy(BUS_NAME, PATH, Duration::from_secs(1))
            .match_signal(Box::new(
                move |message: OrgKdeActivityManagerActivitiesActivityChanged,
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

        let (icon,): (String,) = proxy
            .method_call(INTERFACE, "ActivityIcon", (activity,))
            .context("failed to get activity icon")?;

        Ok(ActivityInfo {
            name,
            description,
            icon,
        })
    }
}
