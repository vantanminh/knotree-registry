//! PostgreSQL boundary and schema migrations.

use std::time::Duration;

use sqlx::{PgPool, postgres::PgPoolOptions};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("database connection failed: {0}")]
    Connect(#[from] sqlx::Error),
    #[error("database migration failed: {0}")]
    Migration(#[source] sqlx::migrate::MigrateError),
}

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self, DatabaseError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), DatabaseError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(DatabaseError::Migration)
    }

    pub async fn ping(&self) -> Result<(), DatabaseError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(DatabaseError::Connect)
    }

    pub async fn load_snapshot(
        &self,
        key: &str,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload FROM runtime_snapshots WHERE snapshot_key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(DatabaseError::Connect)
    }

    pub async fn save_snapshot(
        &self,
        key: &str,
        payload: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO runtime_snapshots (snapshot_key, payload, updated_at) VALUES ($1, $2, now())
             ON CONFLICT (snapshot_key) DO UPDATE SET payload = EXCLUDED.payload, updated_at = now()",
        )
        .bind(key)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(DatabaseError::Connect)
    }
}
