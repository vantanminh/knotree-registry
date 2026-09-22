//! PostgreSQL boundary and schema migrations.

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
}
