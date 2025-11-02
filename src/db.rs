use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
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

    pub async fn get_time_summary(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<HashMap<String, Duration>> {
        let mut time_spent: HashMap<String, Duration> = HashMap::new();

        let activities: Vec<Activity> = sqlx::query_as(
            r#"
            SELECT id, name, start_time, end_time
            FROM activities
            WHERE start_time < ? AND (end_time IS NULL OR end_time > ?);
            "#,
        )
        .bind(end_time.timestamp())
        .bind(start_time.timestamp())
        .fetch_all(&self.pool)
        .await?;

        for activity in activities {
            let activity_start = DateTime::from_timestamp(activity.start_time, 0).unwrap_or(Utc::now());
            let activity_end = match activity.end_time {
                Some(ts) => DateTime::from_timestamp(ts, 0).unwrap_or(Utc::now()),
                None => end_time,
            };

            let effective_start = std::cmp::max(start_time, activity_start);
            let effective_end = std::cmp::min(end_time, activity_end);

            let duration = effective_end - effective_start;

            if duration > Duration::zero() {
                *time_spent.entry(activity.name).or_insert(Duration::zero()) += duration;
            }
        }

        Ok(time_spent)
    }
}