use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool, Type};
use strum_macros::{Display, EnumString};
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ScheduledTaskError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Scheduled task not found")]
    NotFound,
}

#[derive(
    Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display, Default,
)]
#[sqlx(type_name = "scheduled_task_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ScheduledTaskStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct ScheduledTask {
    pub id: Uuid,
    pub task_id: Uuid,
    pub session_id: Option<Uuid>,
    pub execute_at: DateTime<Utc>,
    pub status: ScheduledTaskStatus,
    pub locked_until: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, TS)]
pub struct CreateScheduledTask {
    pub task_id: Uuid,
    pub session_id: Option<Uuid>,
    pub execute_at: DateTime<Utc>,
}

impl ScheduledTask {
    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            ScheduledTask,
            r#"SELECT
                id AS "id!: Uuid",
                task_id AS "task_id!: Uuid",
                session_id AS "session_id: Uuid",
                execute_at AS "execute_at!: DateTime<Utc>",
                status AS "status!: ScheduledTaskStatus",
                locked_until AS "locked_until: DateTime<Utc>",
                error_message,
                created_at AS "created_at!: DateTime<Utc>",
                updated_at AS "updated_at!: DateTime<Utc>"
            FROM scheduled_tasks
            WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_task_id(
        pool: &SqlitePool,
        task_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            ScheduledTask,
            r#"SELECT
                id AS "id!: Uuid",
                task_id AS "task_id!: Uuid",
                session_id AS "session_id: Uuid",
                execute_at AS "execute_at!: DateTime<Utc>",
                status AS "status!: ScheduledTaskStatus",
                locked_until AS "locked_until: DateTime<Utc>",
                error_message,
                created_at AS "created_at!: DateTime<Utc>",
                updated_at AS "updated_at!: DateTime<Utc>"
            FROM scheduled_tasks
            WHERE task_id = $1
            ORDER BY execute_at ASC"#,
            task_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_pending(pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            ScheduledTask,
            r#"SELECT
                id AS "id!: Uuid",
                task_id AS "task_id!: Uuid",
                session_id AS "session_id: Uuid",
                execute_at AS "execute_at!: DateTime<Utc>",
                status AS "status!: ScheduledTaskStatus",
                locked_until AS "locked_until: DateTime<Utc>",
                error_message,
                created_at AS "created_at!: DateTime<Utc>",
                updated_at AS "updated_at!: DateTime<Utc>"
            FROM scheduled_tasks
            WHERE status = 'pending'
            ORDER BY execute_at ASC"#
        )
        .fetch_all(pool)
        .await
    }

    /// Atomically claim the next pending task that is due for execution.
    /// Uses locked_until for distributed locking to prevent multiple workers
    /// from claiming the same task.
    pub async fn claim_next(
        pool: &SqlitePool,
        lock_duration_secs: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        let now = Utc::now();
        let locked_until = now + Duration::seconds(lock_duration_secs);

        // Atomic UPDATE ... RETURNING to claim the next eligible task
        sqlx::query_as!(
            ScheduledTask,
            r#"UPDATE scheduled_tasks
            SET status = 'running',
                locked_until = $1,
                updated_at = datetime('now')
            WHERE id = (
                SELECT id FROM scheduled_tasks
                WHERE status = 'pending'
                  AND execute_at <= $2
                  AND (locked_until IS NULL OR locked_until < $2)
                ORDER BY execute_at ASC
                LIMIT 1
            )
            RETURNING
                id AS "id!: Uuid",
                task_id AS "task_id!: Uuid",
                session_id AS "session_id: Uuid",
                execute_at AS "execute_at!: DateTime<Utc>",
                status AS "status!: ScheduledTaskStatus",
                locked_until AS "locked_until: DateTime<Utc>",
                error_message,
                created_at AS "created_at!: DateTime<Utc>",
                updated_at AS "updated_at!: DateTime<Utc>""#,
            locked_until,
            now
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn create(
        pool: &SqlitePool,
        data: &CreateScheduledTask,
        id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(
            ScheduledTask,
            r#"INSERT INTO scheduled_tasks (id, task_id, session_id, execute_at)
            VALUES ($1, $2, $3, $4)
            RETURNING
                id AS "id!: Uuid",
                task_id AS "task_id!: Uuid",
                session_id AS "session_id: Uuid",
                execute_at AS "execute_at!: DateTime<Utc>",
                status AS "status!: ScheduledTaskStatus",
                locked_until AS "locked_until: DateTime<Utc>",
                error_message,
                created_at AS "created_at!: DateTime<Utc>",
                updated_at AS "updated_at!: DateTime<Utc>""#,
            id,
            data.task_id,
            data.session_id,
            data.execute_at
        )
        .fetch_one(pool)
        .await
    }

    pub async fn update_status(
        pool: &SqlitePool,
        id: Uuid,
        status: ScheduledTaskStatus,
        error_message: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"UPDATE scheduled_tasks
            SET status = $2, error_message = $3, updated_at = datetime('now')
            WHERE id = $1"#,
            id,
            status,
            error_message
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_completed(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
        Self::update_status(pool, id, ScheduledTaskStatus::Completed, None).await
    }

    pub async fn mark_failed(
        pool: &SqlitePool,
        id: Uuid,
        error_message: &str,
    ) -> Result<(), sqlx::Error> {
        Self::update_status(pool, id, ScheduledTaskStatus::Failed, Some(error_message)).await
    }

    pub async fn cancel(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
        Self::update_status(pool, id, ScheduledTaskStatus::Cancelled, None).await
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!("DELETE FROM scheduled_tasks WHERE id = $1", id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}
