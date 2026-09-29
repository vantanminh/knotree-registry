use std::{env, net::SocketAddr, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Local,
    Memory,
    R2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnvironment {
    Development,
    Staging,
    Production,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    Proxy,
    Edge,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub sso: Option<crate::sso::SsoConfig>,
    pub environment: AppEnvironment,
    pub bind_addr: SocketAddr,
    pub public_url: String,
    pub database_url: Option<String>,
    pub database_max_connections: u32,
    pub require_database: bool,
    pub storage_backend: StorageBackend,
    pub storage_root: PathBuf,
    pub static_root: Option<PathBuf>,
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
    pub pull_mode: PullMode,
    pub edge_download_url: Option<String>,
    pub edge_download_secret: Option<String>,
    pub control_plane_origins: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid Accounts SSO configuration: {0}")]
    Sso(&'static str),
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
    #[error("PULL_MODE must be 'proxy' or 'edge'")]
    PullMode(String),
    #[error("edge pull mode requires EDGE_DOWNLOAD_URL and EDGE_DOWNLOAD_SECRET")]
    EdgeUnavailable,
    #[error("BOOTSTRAP_ADMIN_USERNAME and BOOTSTRAP_ADMIN_PASSWORD must be set together")]
    BootstrapCredentialsIncomplete,
    #[error("invalid APP_ENV '{0}', expected development, staging, or production")]
    Environment(String),
    #[error("production requires PUBLIC_REGISTRY_URL to use https")]
    ProductionPublicUrl,
    #[error("production requires REQUIRE_DATABASE=true and DATABASE_URL")]
    ProductionDatabase,
    #[error("production requires COOKIE_SECURE=true")]
    ProductionCookie,
    #[error("production cannot use STORAGE_BACKEND=memory")]
    ProductionStorage,
    #[error("production requires STATIC_ROOT to contain the built frontend")]
    ProductionStaticRoot,
    #[error("production requires bootstrap admin credentials on first startup")]
    BootstrapAdminRequired,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let environment = match env::var("APP_ENV")
            .unwrap_or_else(|_| "development".to_owned())
            .to_ascii_lowercase()
            .as_str()
        {
            "development" | "dev" => AppEnvironment::Development,
            "staging" | "stage" => AppEnvironment::Staging,
            "production" | "prod" => AppEnvironment::Production,
            value => return Err(ConfigError::Environment(value.to_owned())),
        };
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
        let static_root = env::var("STATIC_ROOT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
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
        let pull_mode = match env::var("PULL_MODE")
            .unwrap_or_else(|_| "proxy".to_owned())
            .to_ascii_lowercase()
            .as_str()
        {
            "proxy" => PullMode::Proxy,
            "edge" => PullMode::Edge,
            value => return Err(ConfigError::PullMode(value.to_owned())),
        };
        let edge_download_url = env::var("EDGE_DOWNLOAD_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let edge_download_secret = env::var("EDGE_DOWNLOAD_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let control_plane_origins = env::var("CONTROL_PLANE_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        let config = Self {
            sso: crate::sso::SsoConfig::from_env(environment)?,
            environment,
            bind_addr,
            public_url,
            database_url,
            database_max_connections,
            require_database,
            storage_backend,
            storage_root,
            static_root,
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
            pull_mode,
            edge_download_url,
            edge_download_secret,
            control_plane_origins,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if let Some(sso) = &self.sso {
            sso.validate(self.environment == AppEnvironment::Production)?;
        }
        let Ok(public_url) = url::Url::parse(&self.public_url) else {
            return Err(ConfigError::PublicUrl);
        };
        if !matches!(public_url.scheme(), "http" | "https") || public_url.host_str().is_none() {
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
        if self.pull_mode == PullMode::Edge
            && (self.edge_download_url.is_none() || self.edge_download_secret.is_none())
        {
            return Err(ConfigError::EdgeUnavailable);
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
        if self.database_max_connections == 0 {
            return Err(ConfigError::Number(
                "DATABASE_MAX_CONNECTIONS",
                self.database_max_connections.to_string(),
            ));
        }
        if self.request_body_limit_bytes == 0 {
            return Err(ConfigError::Number(
                "REQUEST_BODY_LIMIT_BYTES",
                self.request_body_limit_bytes.to_string(),
            ));
        }
        if self.upload_chunk_limit_bytes == 0 {
            return Err(ConfigError::Number(
                "UPLOAD_CHUNK_LIMIT_BYTES",
                self.upload_chunk_limit_bytes.to_string(),
            ));
        }
        if self.environment == AppEnvironment::Production {
            if public_url.scheme() != "https" {
                return Err(ConfigError::ProductionPublicUrl);
            }
            if !self.require_database || self.database_url.is_none() {
                return Err(ConfigError::ProductionDatabase);
            }
            if !self.cookie_secure {
                return Err(ConfigError::ProductionCookie);
            }
            if self.storage_backend == StorageBackend::Memory {
                return Err(ConfigError::ProductionStorage);
            }
            if self
                .static_root
                .as_ref()
                .is_none_or(|path| !path.join("index.html").is_file())
            {
                return Err(ConfigError::ProductionStaticRoot);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AppConfig {
        AppConfig {
            sso: None,
            environment: AppEnvironment::Development,
            bind_addr: "127.0.0.1:8080".parse().expect("address"),
            public_url: "http://localhost:8080".to_owned(),
            database_url: None,
            database_max_connections: 10,
            require_database: false,
            storage_backend: StorageBackend::Memory,
            storage_root: PathBuf::from("./data/objects"),
            static_root: None,
            request_body_limit_bytes: 4 * 1024 * 1024,
            upload_chunk_limit_bytes: 64 * 1024 * 1024,
            token_issuer: "knotree-registry".to_owned(),
            token_service: "knotree-registry".to_owned(),
            token_ttl_seconds: 300,
            bootstrap_admin_username: None,
            bootstrap_admin_password: None,
            cookie_secure: true,
            r2_endpoint: None,
            r2_bucket: None,
            r2_access_key_id: None,
            r2_secret_access_key: None,
            r2_region: "auto".to_owned(),
            pull_mode: PullMode::Proxy,
            edge_download_url: None,
            edge_download_secret: None,
            control_plane_origins: Vec::new(),
        }
    }

    #[test]
    fn production_rejects_insecure_public_url_before_startup() {
        let mut config = config();
        config.environment = AppEnvironment::Production;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ProductionPublicUrl)
        ));
    }

    #[test]
    fn production_rejects_memory_storage() {
        let mut config = config();
        config.environment = AppEnvironment::Production;
        config.public_url = "https://registry.example.com".to_owned();
        config.database_url = Some("postgres://registry:secret@db/registry".to_owned());
        config.require_database = true;
        config.static_root = Some(PathBuf::from("."));
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ProductionStorage)
        ));
    }
}
