use anyhow::Result;
use chrono::{DateTime, Duration, Utc, Local};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::FromRow;
use std::collections::HashMap;


pub struct Database {
    pool: SqlitePool,
}

#[derive(Debug, FromRow)]
struct Activity {
    #[allow(dead_code)]
    id: i64,
    name: String,
    start_time: i64,
    end_time: Option<i64>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        let db = Database { pool };
        db.setup().await?;
        Ok(db)
    }

    async fn setup(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS activities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                end_time INTEGER
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn end_current_activity(&self) -> Result<()> {
        let timestamp = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE activities
            SET end_time = ?
            WHERE end_time IS NULL;
            "#,
        )
        .bind(timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn switch_activity(&self, new_activity: &str) -> Result<()> {
        self.end_current_activity().await?;

        let timestamp = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO activities (name, start_time)
            VALUES (?, ?);
            "#,
        )
        .bind(new_activity)
        .bind(timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_current_activity(&self) -> Result<String> {
        let activity: Option<Activity> = sqlx::query_as(
            r#"
            SELECT id, name, start_time, end_time
            FROM activities
            WHERE end_time IS NULL
            ORDER BY start_time DESC
            LIMIT 1;
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(activity.map(|a| a.name).unwrap_or_else(|| "No current activity".to_string()))
    }

    pub async fn get_summary(
        &self,
        start_time: Option<DateTime<Local>>,
        end_time: Option<DateTime<Local>>,
    ) -> Result<HashMap<String, String>> {
        let mut time_spent: HashMap<String, Duration> = HashMap::new();

        let start_time_utc = start_time.map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(Utc::now);
        let end_time_utc = end_time.map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(Utc::now);

        let activities: Vec<Activity> = sqlx::query_as(
            r#"
            SELECT id, name, start_time, end_time
            FROM activities
            WHERE start_time < ? AND (end_time IS NULL OR end_time > ?);
            "#,
        )
        .bind(end_time_utc.timestamp())
        .bind(start_time_utc.timestamp())
        .fetch_all(&self.pool)
        .await?;

        for activity in activities {
            let activity_start = DateTime::from_timestamp(activity.start_time, 0).unwrap().with_timezone(&Utc);
            let activity_end = match activity.end_time {
                Some(ts) => DateTime::from_timestamp(ts, 0).unwrap().with_timezone(&Utc),
                None => end_time_utc,
            };

            let effective_start = std::cmp::max(start_time_utc, activity_start);
            let effective_end = std::cmp::min(end_time_utc, activity_end);

            let duration = effective_end - effective_start;

            if duration > Duration::zero() {
                *time_spent.entry(activity.name).or_insert(Duration::zero()) += duration;
            }
        }

        Ok(time_spent.into_iter().map(|(k, v)| (k, format_duration(v))).collect())
    }
}

fn format_duration(duration: Duration) -> String {
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