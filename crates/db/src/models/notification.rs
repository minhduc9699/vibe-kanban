use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, SqlitePool, Type};
use strum_macros::{Display, EnumString};
use thiserror::Error;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum NotificationError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Notification not found")]
    NotFound,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display)]
#[sqlx(type_name = "notification_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum NotificationType {
    TaskComplete,
    ApprovalNeeded,
    Question,
    Error,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct Notification {
    pub id: Uuid,
    pub session_id: Uuid,
    pub notification_type: NotificationType,
    pub title: String,
    pub message: String,
    #[ts(type = "unknown | null")]
    pub payload: Option<Value>,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, TS)]
pub struct CreateNotification {
    pub session_id: Uuid,
    pub notification_type: NotificationType,
    pub title: String,
    pub message: String,
    #[ts(type = "unknown | null")]
    pub payload: Option<Value>,
}

impl Notification {
    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            Notification,
            r#"SELECT
                id AS "id!: Uuid",
                session_id AS "session_id!: Uuid",
                notification_type AS "notification_type!: NotificationType",
                title,
                message,
                payload AS "payload: Value",
                read_at AS "read_at: DateTime<Utc>",
                created_at AS "created_at!: DateTime<Utc>"
            FROM notifications
            WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_session_id(
        pool: &SqlitePool,
        session_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            Notification,
            r#"SELECT
                id AS "id!: Uuid",
                session_id AS "session_id!: Uuid",
                notification_type AS "notification_type!: NotificationType",
                title,
                message,
                payload AS "payload: Value",
                read_at AS "read_at: DateTime<Utc>",
                created_at AS "created_at!: DateTime<Utc>"
            FROM notifications
            WHERE session_id = $1
            ORDER BY created_at DESC"#,
            session_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_unread_by_session(
        pool: &SqlitePool,
        session_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            Notification,
            r#"SELECT
                id AS "id!: Uuid",
                session_id AS "session_id!: Uuid",
                notification_type AS "notification_type!: NotificationType",
                title,
                message,
                payload AS "payload: Value",
                read_at AS "read_at: DateTime<Utc>",
                created_at AS "created_at!: DateTime<Utc>"
            FROM notifications
            WHERE session_id = $1 AND read_at IS NULL
            ORDER BY created_at DESC"#,
            session_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn count_unread_by_session(
        pool: &SqlitePool,
        session_id: Uuid,
    ) -> Result<i64, sqlx::Error> {
        let count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!: i64"
            FROM notifications
            WHERE session_id = $1 AND read_at IS NULL"#,
            session_id
        )
        .fetch_one(pool)
        .await?;
        Ok(count)
    }

    pub async fn create(
        pool: &SqlitePool,
        data: &CreateNotification,
        id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        let payload_json = data.payload.as_ref().map(|p| serde_json::to_string(p).ok()).flatten();

        sqlx::query_as!(
            Notification,
            r#"INSERT INTO notifications (id, session_id, notification_type, title, message, payload)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING
                id AS "id!: Uuid",
                session_id AS "session_id!: Uuid",
                notification_type AS "notification_type!: NotificationType",
                title,
                message,
                payload AS "payload: Value",
                read_at AS "read_at: DateTime<Utc>",
                created_at AS "created_at!: DateTime<Utc>""#,
            id,
            data.session_id,
            data.notification_type,
            data.title,
            data.message,
            payload_json
        )
        .fetch_one(pool)
        .await
    }

    pub async fn mark_read(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        sqlx::query!(
            "UPDATE notifications SET read_at = $2 WHERE id = $1",
            id,
            now
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_all_read_for_session(
        pool: &SqlitePool,
        session_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let now = Utc::now();
        let result = sqlx::query!(
            "UPDATE notifications SET read_at = $2 WHERE session_id = $1 AND read_at IS NULL",
            session_id,
            now
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!("DELETE FROM notifications WHERE id = $1", id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}
