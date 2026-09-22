use std::{env, net::SocketAddr, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Local,
    Memory,
    R2,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub public_url: String,
    pub database_url: Option<String>,
    pub database_max_connections: u32,
    pub require_database: bool,
    pub storage_backend: StorageBackend,
    pub storage_root: PathBuf,
    pub request_body_limit_bytes: usize,
    pub upload_chunk_limit_bytes: usize,
    pub token_issuer: String,
    pub token_service: String,
    pub token_ttl_seconds: u64,
    pub bootstrap_admin_username: Option<String>,
    pub bootstrap_admin_password: Option<String>,
    pub cookie_secure: bool,
    pub r2_endpoint: Option<String>,
    pub r2_bucket: Option<String>,
    pub r2_access_key_id: Option<String>,
    pub r2_secret_access_key: Option<String>,
    pub r2_region: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid BIND_ADDR: {0}")]
    BindAddr(#[from] std::net::AddrParseError),
    #[error("invalid {0}: {1}")]
    Number(&'static str, String),
    #[error("unsupported STORAGE_BACKEND '{0}'")]
    StorageBackend(String),
    #[error("PUBLIC_REGISTRY_URL must be an absolute http(s) URL")]
    PublicUrl,
    #[error("DATABASE_URL is required when REQUIRE_DATABASE=true")]
    MissingDatabase,
    #[error(
        "R2 storage requires R2_ENDPOINT, R2_BUCKET, R2_ACCESS_KEY_ID and R2_SECRET_ACCESS_KEY"
    )]
    R2Unavailable,
    #[error("BOOTSTRAP_ADMIN_USERNAME and BOOTSTRAP_ADMIN_PASSWORD must be set together")]
    BootstrapCredentialsIncomplete,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_addr = env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
            .parse()?;
        let public_url =
            env::var("PUBLIC_REGISTRY_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned());
        let database_url = env::var("DATABASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let database_max_connections = parse_u32("DATABASE_MAX_CONNECTIONS", 10)?;
        let require_database = parse_bool("REQUIRE_DATABASE", false)?;
        let storage_backend = match env::var("STORAGE_BACKEND")
            .unwrap_or_else(|_| "local".to_owned())
            .to_ascii_lowercase()
            .as_str()
        {
            "local" => StorageBackend::Local,
            "memory" => StorageBackend::Memory,
            "r2" => StorageBackend::R2,
            value => return Err(ConfigError::StorageBackend(value.to_owned())),
        };
        let storage_root =
            PathBuf::from(env::var("STORAGE_ROOT").unwrap_or_else(|_| "./data/objects".to_owned()));
        let request_body_limit_bytes = parse_usize("REQUEST_BODY_LIMIT_BYTES", 4 * 1024 * 1024)?;
        let upload_chunk_limit_bytes = parse_usize("UPLOAD_CHUNK_LIMIT_BYTES", 64 * 1024 * 1024)?;
        let token_issuer =
            env::var("TOKEN_ISSUER").unwrap_or_else(|_| "knotree-registry".to_owned());
        let token_service =
            env::var("TOKEN_SERVICE").unwrap_or_else(|_| "knotree-registry".to_owned());
        let token_ttl_seconds = parse_u64("REGISTRY_TOKEN_TTL_SECONDS", 300)?;
        let bootstrap_admin_username = env::var("BOOTSTRAP_ADMIN_USERNAME")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let bootstrap_admin_password = env::var("BOOTSTRAP_ADMIN_PASSWORD")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let cookie_secure = parse_bool("COOKIE_SECURE", true)?;
        let r2_endpoint = env::var("R2_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let r2_bucket = env::var("R2_BUCKET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let r2_access_key_id = env::var("R2_ACCESS_KEY_ID")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let r2_secret_access_key = env::var("R2_SECRET_ACCESS_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let r2_region = env::var("R2_REGION").unwrap_or_else(|_| "auto".to_owned());
        let config = Self {
            bind_addr,
            public_url,
            database_url,
            database_max_connections,
            require_database,
            storage_backend,
            storage_root,
            request_body_limit_bytes,
            upload_chunk_limit_bytes,
            token_issuer,
            token_service,
            token_ttl_seconds,
            bootstrap_admin_username,
            bootstrap_admin_password,
            cookie_secure,
            r2_endpoint,
            r2_bucket,
            r2_access_key_id,
            r2_secret_access_key,
            r2_region,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(self.public_url.starts_with("http://") || self.public_url.starts_with("https://")) {
            return Err(ConfigError::PublicUrl);
        }
        if self.require_database && self.database_url.is_none() {
            return Err(ConfigError::MissingDatabase);
        }
        if self.storage_backend == StorageBackend::R2
            && (self.r2_endpoint.is_none()
                || self.r2_bucket.is_none()
                || self.r2_access_key_id.is_none()
                || self.r2_secret_access_key.is_none())
        {
            return Err(ConfigError::R2Unavailable);
        }
        if self.token_ttl_seconds < 60 || self.token_ttl_seconds > 3600 {
            return Err(ConfigError::Number(
                "REGISTRY_TOKEN_TTL_SECONDS",
                self.token_ttl_seconds.to_string(),
            ));
        }
        if self.bootstrap_admin_username.is_some() != self.bootstrap_admin_password.is_some() {
            return Err(ConfigError::BootstrapCredentialsIncomplete);
        }
        Ok(())
    }
}

fn parse_u32(name: &'static str, default: u32) -> Result<u32, ConfigError> {
    env::var(name).map_or(Ok(default), |value| {
        value.parse().map_err(|_| ConfigError::Number(name, value))
    })
}

fn parse_usize(name: &'static str, default: usize) -> Result<usize, ConfigError> {
    env::var(name).map_or(Ok(default), |value| {
        value.parse().map_err(|_| ConfigError::Number(name, value))
    })
}

fn parse_u64(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    env::var(name).map_or(Ok(default), |value| {
        value.parse().map_err(|_| ConfigError::Number(name, value))
    })
}

fn parse_bool(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    env::var(name).map_or(Ok(default), |value| {
        match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Ok(true),
            "0" | "false" | "no" => Ok(false),
            _ => Err(ConfigError::Number(name, value)),
        }
    })
}
