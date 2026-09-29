use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use registry_core::{Action, RepositoryName, RepositoryScope};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    jwt::{AccessEntry, JwtIssuer, RegistryClaims, now_seconds},
    secret::{SecretVerifier, issue_secret, raw_digest},
    totp,
};

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("two-factor authentication is required")]
    TwoFactorRequired,
    #[error("invalid two-factor authentication code")]
    InvalidTwoFactorCode,
    #[error("two-factor authentication is already enabled")]
    TwoFactorAlreadyEnabled,
    #[error("two-factor authentication is not enabled")]
    TwoFactorNotEnabled,
    #[error("two-factor setup has not been started")]
    TwoFactorSetupMissing,
    #[error("invalid session")]
    InvalidSession,
    #[error("bootstrap admin already exists")]
    AlreadyBootstrapped,
    #[error("username must be 3-64 lowercase-safe characters")]
    InvalidUsername,
    #[error("password must be at least 12 characters")]
    WeakPassword,
    #[error("password hashing failed")]
    PasswordHash,
    #[error("invalid password hash")]
    PasswordFormat,
    #[error("credential is revoked")]
    CredentialRevoked,
    #[error("credential is expired")]
    CredentialExpired,
    #[error("credential has no access to the requested scope")]
    NoAccess,
    #[error("invalid repository scope")]
    InvalidScope,
    #[error("token service does not match")]
    InvalidService,
    #[error("bearer token is invalid")]
    Bearer,
    #[error("persistent authentication state is invalid")]
    PersistentState,
    #[error("persistent authentication state could not be serialized")]
    PersistentStateSerialization,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserSummary {
    pub id: Uuid,
    pub username: String,
    pub is_admin: bool,
}

impl UserSummary {
    pub fn can_access_repository(&self, repository: &RepositoryName) -> bool {
        self.is_admin
            || repository
                .as_str()
                .starts_with(&format!("{}/", self.username))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub token: String,
    pub user: UserSummary,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialCreated {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub secret: String,
    pub scopes: Vec<RepositoryScope>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialSummary {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<RepositoryScope>,
    pub expires_at: Option<u64>,
    pub last_used_at: Option<u64>,
    pub revoked_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MintedToken {
    pub token: String,
    pub expires_in: u64,
    pub issued_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TotpSetup {
    pub secret: String,
    pub otpauth_uri: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TotpStatus {
    pub enabled: bool,
}

#[derive(Clone)]
pub struct AuthService {
    state: std::sync::Arc<tokio::sync::RwLock<AuthState>>,
    issuer: JwtIssuer,
    issuer_name: String,
    service: String,
    token_ttl: Duration,
}

struct AuthState {
    users: HashMap<String, UserRecord>,
    credentials: HashMap<Uuid, CredentialRecord>,
    credential_prefixes: HashMap<String, Uuid>,
    sessions: HashMap<[u8; 32], SessionRecord>,
    login_attempts: HashMap<String, LoginAttempt>,
}

#[derive(Clone, Serialize, Deserialize)]
struct UserRecord {
    id: Uuid,
    username: String,
    password_hash: String,
    is_admin: bool,
    #[serde(default)]
    federated_identity: Option<(String, String)>,
    #[serde(default)]
    totp_secret: Option<String>,
    #[serde(default)]
    totp_pending_secret: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CredentialRecord {
    id: Uuid,
    user_id: Uuid,
    name: String,
    prefix: String,
    verifier: SecretVerifier,
    scopes: Vec<RepositoryScope>,
    expires_at: Option<u64>,
    revoked_at: Option<u64>,
    last_used_at: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SessionRecord {
    user_id: Uuid,
    expires_at: u64,
    revoked: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct LoginAttempt {
    failures: u32,
    blocked_until: u64,
}

#[derive(Serialize, Deserialize)]
struct AuthSnapshot {
    version: u32,
    issuer: crate::jwt::JwtSnapshot,
    users: Vec<UserRecord>,
    credentials: Vec<CredentialRecord>,
    sessions: Vec<SessionSnapshot>,
    login_attempts: HashMap<String, LoginAttempt>,
}

#[derive(Serialize, Deserialize)]
struct SessionSnapshot {
    digest: Vec<u8>,
    record: SessionRecord,
}

impl AuthService {
    pub fn new(
        issuer: impl Into<String>,
        service: impl Into<String>,
        token_ttl_seconds: u64,
    ) -> Self {
        let issuer_name = issuer.into();
        Self {
            state: std::sync::Arc::new(tokio::sync::RwLock::new(AuthState {
                users: HashMap::new(),
                credentials: HashMap::new(),
                credential_prefixes: HashMap::new(),
                sessions: HashMap::new(),
                login_attempts: HashMap::new(),
            })),
            issuer: JwtIssuer::new(&issuer_name),
            issuer_name,
            service: service.into(),
            token_ttl: Duration::from_secs(token_ttl_seconds.clamp(60, 3600)),
        }
    }

    pub fn from_snapshot(
        issuer: impl Into<String>,
        service: impl Into<String>,
        token_ttl_seconds: u64,
        value: serde_json::Value,
    ) -> Result<Self, AuthError> {
        let issuer_name = issuer.into();
        let snapshot: AuthSnapshot =
            serde_json::from_value(value).map_err(|_| AuthError::PersistentState)?;
        if snapshot.version != 1 || snapshot.issuer.issuer != issuer_name {
            return Err(AuthError::PersistentState);
        }
        let issuer =
            JwtIssuer::from_snapshot(snapshot.issuer).map_err(|_| AuthError::PersistentState)?;
        let mut users = HashMap::with_capacity(snapshot.users.len());
        for user in snapshot.users {
            if users.insert(user.username.clone(), user).is_some() {
                return Err(AuthError::PersistentState);
            }
        }
        let mut credentials = HashMap::with_capacity(snapshot.credentials.len());
        let mut credential_prefixes = HashMap::with_capacity(snapshot.credentials.len());
        for credential in snapshot.credentials {
            if !users.values().any(|user| user.id == credential.user_id)
                || credential_prefixes
                    .insert(credential.prefix.clone(), credential.id)
                    .is_some()
                || credentials.insert(credential.id, credential).is_some()
            {
                return Err(AuthError::PersistentState);
            }
        }
        let mut sessions = HashMap::with_capacity(snapshot.sessions.len());
        for session in snapshot.sessions {
            let digest: [u8; 32] = session
                .digest
                .try_into()
                .map_err(|_| AuthError::PersistentState)?;
            if sessions.insert(digest, session.record).is_some() {
                return Err(AuthError::PersistentState);
            }
        }
        Ok(Self {
            state: std::sync::Arc::new(tokio::sync::RwLock::new(AuthState {
                users,
                credentials,
                credential_prefixes,
                sessions,
                login_attempts: snapshot.login_attempts,
            })),
            issuer,
            issuer_name,
            service: service.into(),
            token_ttl: Duration::from_secs(token_ttl_seconds.clamp(60, 3600)),
        })
    }

    pub async fn has_users(&self) -> bool {
        !self.state.read().await.users.is_empty()
    }

    pub async fn snapshot(&self) -> Result<serde_json::Value, AuthError> {
        let state = self.state.read().await;
        let snapshot = AuthSnapshot {
            version: 1,
            issuer: self.issuer.snapshot(),
            users: state.users.values().cloned().collect(),
            credentials: state.credentials.values().cloned().collect(),
            sessions: state
                .sessions
                .iter()
                .map(|(digest, record)| SessionSnapshot {
                    digest: digest.to_vec(),
                    record: record.clone(),
                })
                .collect(),
            login_attempts: state.login_attempts.clone(),
        };
        serde_json::to_value(snapshot).map_err(|_| AuthError::PersistentStateSerialization)
    }

    pub async fn bootstrap_admin(
        &self,
        username: &str,
        password: &str,
    ) -> Result<UserSummary, AuthError> {
        validate_username(username)?;
        let password_hash = hash_password(password)?;
        let mut state = self.state.write().await;
        if !state.users.is_empty() {
            return Err(AuthError::AlreadyBootstrapped);
        }
        let user = UserRecord {
            id: Uuid::new_v4(),
            username: username.to_owned(),
            password_hash,
            is_admin: true,
            federated_identity: None,
            totp_secret: None,
            totp_pending_secret: None,
        };
        let summary = summary(&user);
        state.users.insert(user.username.clone(), user);
        Ok(summary)
    }

    /// Only call after verifying the central provider's live userinfo response.
    /// External identities receive an isolated repository namespace and no admin role.
    pub async fn login_federated(&self, issuer: &str, subject: &str) -> Result<Session, AuthError> {
        if issuer.is_empty()
            || issuer.len() > 2048
            || subject.is_empty()
            || subject.len() > 255
            || issuer.chars().chain(subject.chars()).any(char::is_control)
        {
            return Err(AuthError::InvalidCredentials);
        }
        let identity = (issuer.to_owned(), subject.to_owned());
        let identity_key = serde_json::to_string(&identity)
            .map_err(|_| AuthError::PersistentStateSerialization)?;
        // Digest-derived namespace is stable across restarts and distinct providers.
        let digest: String = raw_digest(&identity_key)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let username = format!("kt-{digest}");
        let username = username[..64].to_owned();
        let mut state = self.state.write().await;
        let user = if let Some(user) = state.users.get(&username) {
            if user.federated_identity.as_ref() != Some(&identity) || user.is_admin {
                return Err(AuthError::InvalidCredentials);
            }
            user.clone()
        } else {
            let user = UserRecord {
                id: Uuid::new_v4(),
                username: username.clone(),
                password_hash: "!sso-only".into(),
                is_admin: false,
                federated_identity: Some(identity),
                totp_secret: None,
                totp_pending_secret: None,
            };
            state.users.insert(username, user.clone());
            user
        };
        Ok(issue_session(&mut state, &user))
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<Session, AuthError> {
        self.login_with_totp(username, password, None).await
    }

    pub async fn login_with_totp(
        &self,
        username: &str,
        password: &str,
        code: Option<&str>,
    ) -> Result<Session, AuthError> {
        let now = now_seconds();
        let user = {
            let state = self.state.read().await;
            if state
                .login_attempts
                .get(username)
                .is_some_and(|attempt| attempt.blocked_until > now)
            {
                return Err(AuthError::InvalidCredentials);
            }
            state.users.get(username).cloned()
        };
        let Some(user) = user else {
            return Err(self.failed_login(username).await);
        };
        if verify_password(&user.password_hash, password).is_err() {
            return Err(self.failed_login(username).await);
        }
        if let Some(secret) = user.totp_secret.as_deref() {
            let Some(code) = code else {
                return Err(AuthError::TwoFactorRequired);
            };
            if !totp::verify(secret, code, now) {
                let _ = self.failed_login(username).await;
                return Err(AuthError::InvalidTwoFactorCode);
            }
        }
        let mut state = self.state.write().await;
        state.login_attempts.remove(username);
        Ok(issue_session(&mut state, &user))
    }

    pub async fn change_password(
        &self,
        session_token: &str,
        current_password: &str,
        new_password: &str,
    ) -> Result<Session, AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        let user = self
            .state
            .read()
            .await
            .users
            .values()
            .find(|user| user.id == user_id)
            .cloned()
            .ok_or(AuthError::InvalidSession)?;
        verify_password(&user.password_hash, current_password)?;
        let password_hash = hash_password(new_password)?;
        let mut state = self.state.write().await;
        let updated = state
            .users
            .get_mut(&user.username)
            .ok_or(AuthError::InvalidSession)?;
        updated.password_hash = password_hash;
        for session in state.sessions.values_mut() {
            if session.user_id == user_id {
                session.revoked = true;
            }
        }
        let updated = state
            .users
            .get(&user.username)
            .cloned()
            .ok_or(AuthError::InvalidSession)?;
        Ok(issue_session(&mut state, &updated))
    }

    pub async fn totp_status(&self, session_token: &str) -> Result<TotpStatus, AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        let state = self.state.read().await;
        let user = state
            .users
            .values()
            .find(|user| user.id == user_id)
            .ok_or(AuthError::InvalidSession)?;
        Ok(TotpStatus {
            enabled: user.totp_secret.is_some(),
        })
    }

    pub async fn begin_totp_setup(
        &self,
        session_token: &str,
        password: &str,
    ) -> Result<TotpSetup, AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        let user = self
            .state
            .read()
            .await
            .users
            .values()
            .find(|user| user.id == user_id)
            .cloned()
            .ok_or(AuthError::InvalidSession)?;
        verify_password(&user.password_hash, password)?;
        let mut state = self.state.write().await;
        let user = state
            .users
            .values()
            .find(|user| user.id == user_id)
            .cloned()
            .ok_or(AuthError::InvalidSession)?;
        if user.totp_secret.is_some() {
            return Err(AuthError::TwoFactorAlreadyEnabled);
        }
        let secret = totp::generate_secret();
        let user = state
            .users
            .get_mut(&user.username)
            .ok_or(AuthError::InvalidSession)?;
        user.totp_pending_secret = Some(secret.clone());
        Ok(TotpSetup {
            otpauth_uri: totp::otpauth_uri(&self.issuer_name, &user.username, &secret),
            secret,
        })
    }

    pub async fn confirm_totp_setup(
        &self,
        session_token: &str,
        code: &str,
    ) -> Result<TotpStatus, AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        let digest = raw_digest(session_token);
        let mut state = self.state.write().await;
        let user = state
            .users
            .values()
            .find(|user| user.id == user_id)
            .cloned()
            .ok_or(AuthError::InvalidSession)?;
        let secret = user
            .totp_pending_secret
            .as_deref()
            .ok_or(AuthError::TwoFactorSetupMissing)?;
        if !totp::verify(secret, code, now_seconds()) {
            return Err(AuthError::InvalidTwoFactorCode);
        }
        let user = state
            .users
            .get_mut(&user.username)
            .ok_or(AuthError::InvalidSession)?;
        user.totp_secret = Some(secret.to_owned());
        user.totp_pending_secret = None;
        for (session_digest, session) in &mut state.sessions {
            if session.user_id == user_id && session_digest != &digest {
                session.revoked = true;
            }
        }
        Ok(TotpStatus { enabled: true })
    }

    pub async fn disable_totp(
        &self,
        session_token: &str,
        password: &str,
        code: &str,
    ) -> Result<TotpStatus, AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        let user = self
            .state
            .read()
            .await
            .users
            .values()
            .find(|user| user.id == user_id)
            .cloned()
            .ok_or(AuthError::InvalidSession)?;
        verify_password(&user.password_hash, password)?;
        let secret = user
            .totp_secret
            .as_deref()
            .ok_or(AuthError::TwoFactorNotEnabled)?;
        if !totp::verify(secret, code, now_seconds()) {
            return Err(AuthError::InvalidTwoFactorCode);
        }
        let mut state = self.state.write().await;
        let user = state
            .users
            .values()
            .find(|user| user.id == user_id)
            .cloned()
            .ok_or(AuthError::InvalidSession)?;
        if user.totp_secret.is_none() {
            return Err(AuthError::TwoFactorNotEnabled);
        }
        let user = state
            .users
            .get_mut(&user.username)
            .ok_or(AuthError::InvalidSession)?;
        user.totp_secret = None;
        user.totp_pending_secret = None;
        Ok(TotpStatus { enabled: false })
    }

    pub async fn logout(&self, session_token: &str) -> Result<(), AuthError> {
        let digest = raw_digest(session_token);
        let mut state = self.state.write().await;
        let session = state
            .sessions
            .get_mut(&digest)
            .ok_or(AuthError::InvalidSession)?;
        session.revoked = true;
        Ok(())
    }

    pub async fn session_user(&self, session_token: &str) -> Result<UserSummary, AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        let state = self.state.read().await;
        state
            .users
            .values()
            .find(|user| user.id == user_id)
            .map(summary)
            .ok_or(AuthError::InvalidSession)
    }

    pub async fn create_credential_for_session(
        &self,
        session_token: &str,
        name: String,
        scopes: Vec<RepositoryScope>,
        expires_at: Option<u64>,
    ) -> Result<CredentialCreated, AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        self.issue_credential_for_user(user_id, name, scopes, expires_at)
            .await
    }

    pub async fn list_credentials_for_session(
        &self,
        session_token: &str,
    ) -> Result<Vec<CredentialSummary>, AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        let state = self.state.read().await;
        Ok(state
            .credentials
            .values()
            .filter(|credential| credential.user_id == user_id)
            .map(|credential| CredentialSummary {
                id: credential.id,
                name: credential.name.clone(),
                prefix: credential.prefix.clone(),
                scopes: credential.scopes.clone(),
                expires_at: credential.expires_at,
                last_used_at: credential.last_used_at,
                revoked_at: credential.revoked_at,
            })
            .collect())
    }

    pub async fn issue_credential_for_user(
        &self,
        user_id: Uuid,
        name: String,
        scopes: Vec<RepositoryScope>,
        expires_at: Option<u64>,
    ) -> Result<CredentialCreated, AuthError> {
        if name.trim().is_empty() || scopes.is_empty() {
            return Err(AuthError::InvalidScope);
        }
        let (secret, prefix, verifier) = issue_secret("kntr_pat_");
        let credential = CredentialRecord {
            id: Uuid::new_v4(),
            user_id,
            name: name.clone(),
            prefix: prefix.clone(),
            verifier,
            scopes: scopes.clone(),
            expires_at,
            revoked_at: None,
            last_used_at: None,
        };
        let id = credential.id;
        let mut state = self.state.write().await;
        let user = state
            .users
            .values()
            .find(|user| user.id == user_id)
            .ok_or(AuthError::InvalidSession)?;
        let user = summary(user);
        if scopes.iter().any(|scope| {
            !user.can_access_repository(&scope.repository)
                || (!user.is_admin && scope.actions.contains(&Action::Admin))
        }) {
            return Err(AuthError::NoAccess);
        }
        state.credential_prefixes.insert(prefix.clone(), id);
        state.credentials.insert(id, credential);
        Ok(CredentialCreated {
            id,
            name,
            prefix,
            secret,
            scopes,
            expires_at,
        })
    }

    pub async fn revoke_credential(&self, id: Uuid) -> Result<(), AuthError> {
        let mut state = self.state.write().await;
        let credential = state
            .credentials
            .get_mut(&id)
            .ok_or(AuthError::InvalidCredentials)?;
        credential.revoked_at = Some(now_seconds());
        Ok(())
    }

    pub async fn revoke_credential_for_session(
        &self,
        session_token: &str,
        id: Uuid,
    ) -> Result<(), AuthError> {
        let user_id = self.session_user_id(session_token).await?;
        let mut state = self.state.write().await;
        let is_admin = state
            .users
            .values()
            .find(|user| user.id == user_id)
            .is_some_and(|user| user.is_admin);
        let credential = state
            .credentials
            .get_mut(&id)
            .ok_or(AuthError::InvalidCredentials)?;
        if credential.user_id != user_id && !is_admin {
            return Err(AuthError::NoAccess);
        }
        credential.revoked_at = Some(now_seconds());
        Ok(())
    }

    pub async fn mint_token(
        &self,
        username: &str,
        secret: &str,
        service: &str,
        requested: &[RepositoryScope],
    ) -> Result<MintedToken, AuthError> {
        if service != self.service {
            return Err(AuthError::InvalidService);
        }
        let user_id = {
            let state = self.state.read().await;
            state
                .users
                .get(username)
                .map(|user| user.id)
                .ok_or(AuthError::InvalidCredentials)?
        };
        let prefix: String = secret.chars().take(16).collect();
        let now = now_seconds();
        let (credential_id, scopes, credential_expiry) = {
            let mut state = self.state.write().await;
            let credential_id = *state
                .credential_prefixes
                .get(&prefix)
                .ok_or(AuthError::InvalidCredentials)?;
            let credential = state
                .credentials
                .get_mut(&credential_id)
                .ok_or(AuthError::InvalidCredentials)?;
            if credential.user_id != user_id || !credential.verifier.verify(secret) {
                return Err(AuthError::InvalidCredentials);
            }
            if credential.revoked_at.is_some() {
                return Err(AuthError::CredentialRevoked);
            }
            if credential
                .expires_at
                .is_some_and(|expires_at| expires_at <= now)
            {
                return Err(AuthError::CredentialExpired);
            }
            credential.last_used_at = Some(now);
            (
                credential.id,
                credential.scopes.clone(),
                credential.expires_at,
            )
        };
        let access = if requested.is_empty() {
            scopes_to_access(&scopes)
        } else {
            intersect_scopes(&scopes, requested)
        };
        if access.is_empty() {
            return Err(AuthError::NoAccess);
        }
        let expiry = credential_expiry
            .map_or(now.saturating_add(self.token_ttl.as_secs()), |value| {
                value.min(now.saturating_add(self.token_ttl.as_secs()))
            });
        let claims = RegistryClaims {
            iss: self.issuer_name.clone(),
            sub: user_id.to_string(),
            aud: self.service.clone(),
            exp: expiry,
            iat: now,
            nbf: now,
            jti: format!("{}-{}", credential_id, Uuid::new_v4()),
            access,
        };
        Ok(MintedToken {
            token: self.issuer.issue(&claims).map_err(|_| AuthError::Bearer)?,
            expires_in: expiry.saturating_sub(now),
            issued_at: now,
        })
    }

    pub async fn verify_bearer(
        &self,
        token: &str,
        repository: &RepositoryName,
        action: Action,
    ) -> Result<UserSummary, AuthError> {
        let claims = self
            .issuer
            .verify(token, &self.service)
            .map_err(|_| AuthError::Bearer)?;
        if !claims.access.iter().any(|entry| {
            entry.typ == "repository"
                && entry.name == repository.as_str()
                && entry
                    .actions
                    .iter()
                    .any(|value| value == &action.to_string() || value == "admin")
        }) {
            return Err(AuthError::NoAccess);
        }
        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AuthError::InvalidCredentials)?;
        let state = self.state.read().await;
        let user = state
            .users
            .values()
            .find(|user| user.id == user_id)
            .map(summary)
            .ok_or(AuthError::InvalidCredentials)?;
        if !user.can_access_repository(repository) {
            return Err(AuthError::NoAccess);
        }
        Ok(user)
    }

    pub async fn verify_service_token(&self, token: &str) -> Result<UserSummary, AuthError> {
        let claims = self
            .issuer
            .verify(token, &self.service)
            .map_err(|_| AuthError::Bearer)?;
        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AuthError::InvalidCredentials)?;
        let state = self.state.read().await;
        state
            .users
            .values()
            .find(|user| user.id == user_id)
            .map(summary)
            .ok_or(AuthError::InvalidCredentials)
    }

    async fn session_user_id(&self, session_token: &str) -> Result<Uuid, AuthError> {
        let now = now_seconds();
        let state = self.state.read().await;
        let session = state
            .sessions
            .get(&raw_digest(session_token))
            .ok_or(AuthError::InvalidSession)?;
        if session.revoked || session.expires_at <= now {
            return Err(AuthError::InvalidSession);
        }
        Ok(session.user_id)
    }

    async fn failed_login(&self, username: &str) -> AuthError {
        let now = now_seconds();
        let mut state = self.state.write().await;
        let attempt = state
            .login_attempts
            .entry(username.to_owned())
            .or_insert(LoginAttempt {
                failures: 0,
                blocked_until: 0,
            });
        attempt.failures = attempt.failures.saturating_add(1);
        if attempt.failures >= 5 {
            let exponent = attempt.failures.saturating_sub(5).min(5);
            attempt.blocked_until = now.saturating_add(2u64.saturating_pow(exponent));
        }
        AuthError::InvalidCredentials
    }
}

pub fn parse_scope(value: &str) -> Result<RepositoryScope, AuthError> {
    let mut pieces = value.splitn(3, ':');
    if pieces.next() != Some("repository") {
        return Err(AuthError::InvalidScope);
    }
    let repository = RepositoryName::parse(pieces.next().ok_or(AuthError::InvalidScope)?)
        .map_err(|_| AuthError::InvalidScope)?;
    let actions = pieces
        .next()
        .ok_or(AuthError::InvalidScope)?
        .split(',')
        .filter(|value| !value.is_empty())
        .map(|value| match value {
            "pull" => Ok(Action::Pull),
            "push" => Ok(Action::Push),
            "delete" => Ok(Action::Delete),
            "admin" => Ok(Action::Admin),
            _ => Err(AuthError::InvalidScope),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actions.is_empty() {
        return Err(AuthError::InvalidScope);
    }
    Ok(RepositoryScope {
        repository,
        actions,
    })
}

fn intersect_scopes(actual: &[RepositoryScope], requested: &[RepositoryScope]) -> Vec<AccessEntry> {
    requested
        .iter()
        .filter_map(|wanted| {
            let allowed = actual
                .iter()
                .find(|scope| scope.repository == wanted.repository)?;
            let actions = wanted
                .actions
                .iter()
                .filter(|action| allowed.allows(**action))
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            (!actions.is_empty()).then(|| AccessEntry {
                typ: "repository".to_owned(),
                name: wanted.repository.to_string(),
                actions,
            })
        })
        .collect()
}

fn scopes_to_access(scopes: &[RepositoryScope]) -> Vec<AccessEntry> {
    scopes
        .iter()
        .map(|scope| AccessEntry {
            typ: "repository".to_owned(),
            name: scope.repository.to_string(),
            actions: scope.actions.iter().map(ToString::to_string).collect(),
        })
        .collect()
}

fn summary(user: &UserRecord) -> UserSummary {
    UserSummary {
        id: user.id,
        username: user.username.clone(),
        is_admin: user.is_admin,
    }
}

fn issue_session(state: &mut AuthState, user: &UserRecord) -> Session {
    let (token, _, _) = issue_secret("kntr_session_");
    let expires_at = now_seconds().saturating_add(8 * 60 * 60);
    state.sessions.insert(
        raw_digest(&token),
        SessionRecord {
            user_id: user.id,
            expires_at,
            revoked: false,
        },
    );
    Session {
        token,
        user: summary(user),
        expires_at,
    }
}

fn validate_username(username: &str) -> Result<(), AuthError> {
    if !(3..=64).contains(&username.len())
        || username != username.to_ascii_lowercase()
        || username.bytes().any(|byte| {
            !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte))
        })
    {
        return Err(AuthError::InvalidUsername);
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, AuthError> {
    if password.chars().count() < 12 {
        return Err(AuthError::WeakPassword);
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::PasswordHash)
}

fn verify_password(encoded: &str, password: &str) -> Result<(), AuthError> {
    let parsed = PasswordHash::new(encoded).map_err(|_| AuthError::PasswordFormat)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AuthError::InvalidCredentials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn federated_users_are_stable_non_admin_and_cannot_grant_other_namespaces() {
        let auth = AuthService::new("knotree-registry", "knotree-registry", 300);
        auth.bootstrap_admin("admin", "correct horse battery staple")
            .await
            .unwrap();
        let first = auth
            .login_federated("https://accounts.knotree.com", "alice-subject")
            .await
            .unwrap();
        assert!(!first.user.is_admin);
        assert!(
            auth.login(&first.user.username, "arbitrary password")
                .await
                .is_err()
        );
        let again = auth
            .login_federated("https://accounts.knotree.com", "alice-subject")
            .await
            .unwrap();
        assert_eq!(first.user.id, again.user.id);
        let other = auth
            .login_federated("https://other.example.com", "alice-subject")
            .await
            .unwrap();
        assert_ne!(first.user.username, other.user.username);
        let own = scope(&format!("repository:{}/app:pull,push", first.user.username));
        let credential = auth
            .create_credential_for_session(&first.token, "own".into(), vec![own.clone()], None)
            .await
            .unwrap();
        assert!(
            auth.create_credential_for_session(
                &first.token,
                "other".into(),
                vec![scope("repository:admin/app:pull")],
                None
            )
            .await
            .is_err()
        );
        assert!(
            auth.create_credential_for_session(
                &first.token,
                "admin".into(),
                vec![scope(&format!(
                    "repository:{}/app:admin",
                    first.user.username
                ))],
                None
            )
            .await
            .is_err()
        );
        let token = auth
            .mint_token(
                &first.user.username,
                &credential.secret,
                "knotree-registry",
                std::slice::from_ref(&own),
            )
            .await
            .unwrap();
        assert!(
            auth.verify_bearer(&token.token, &own.repository, Action::Pull)
                .await
                .is_ok()
        );
        assert!(
            auth.verify_bearer(
                &token.token,
                &RepositoryName::parse("admin/app").unwrap(),
                Action::Pull
            )
            .await
            .is_err()
        );
        let restored = AuthService::from_snapshot(
            "knotree-registry",
            "knotree-registry",
            300,
            auth.snapshot().await.unwrap(),
        )
        .unwrap();
        let restored_session = restored
            .login_federated("https://accounts.knotree.com", "alice-subject")
            .await
            .unwrap();
        assert_eq!(restored_session.user.id, first.user.id);
        assert_eq!(restored_session.user.username, first.user.username);
    }

    fn scope(value: &str) -> RepositoryScope {
        parse_scope(value).expect("scope")
    }

    #[tokio::test]
    async fn pat_mints_intersected_bearer_and_revocation_stops_new_tokens() {
        let auth = AuthService::new("knotree-registry", "knotree-registry", 300);
        let user = auth
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let session = auth
            .login("admin", "correct horse battery staple")
            .await
            .expect("login");
        let credential = auth
            .create_credential_for_session(
                &session.token,
                "ci".to_owned(),
                vec![scope("repository:team/app:pull,push")],
                None,
            )
            .await
            .expect("credential");
        let minted = auth
            .mint_token(
                "admin",
                &credential.secret,
                "knotree-registry",
                &[scope("repository:team/app:pull,push,delete")],
            )
            .await
            .expect("mint");
        let repository = RepositoryName::parse("team/app").expect("repository");
        assert_eq!(
            auth.verify_bearer(&minted.token, &repository, Action::Pull)
                .await
                .expect("pull")
                .id,
            user.id
        );
        assert!(matches!(
            auth.verify_bearer(&minted.token, &repository, Action::Delete)
                .await,
            Err(AuthError::NoAccess)
        ));
        auth.revoke_credential(credential.id).await.expect("revoke");
        assert!(matches!(
            auth.mint_token(
                "admin",
                &credential.secret,
                "knotree-registry",
                &[scope("repository:team/app:pull")]
            )
            .await,
            Err(AuthError::CredentialRevoked)
        ));
    }

    #[test]
    fn scope_parser_rejects_unknown_actions() {
        assert!(matches!(
            parse_scope("repository:team/app:push,unknown"),
            Err(AuthError::InvalidScope)
        ));
    }

    #[tokio::test]
    async fn repeated_failed_logins_apply_backoff() {
        let auth = AuthService::new("knotree-registry", "knotree-registry", 300);
        auth.bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");

        for _ in 0..5 {
            assert!(matches!(
                auth.login("admin", "wrong password").await,
                Err(AuthError::InvalidCredentials)
            ));
        }

        let state = auth.state.read().await;
        let attempt = state.login_attempts.get("admin").expect("login attempt");
        assert_eq!(attempt.failures, 5);
        assert!(attempt.blocked_until > now_seconds());
    }

    #[tokio::test]
    async fn password_change_requires_current_password_and_rotates_sessions() {
        let auth = AuthService::new("knotree-registry", "knotree-registry", 300);
        auth.bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let old_session = auth
            .login("admin", "correct horse battery staple")
            .await
            .expect("login");
        let new_session = auth
            .change_password(
                &old_session.token,
                "correct horse battery staple",
                "a different secure password",
            )
            .await
            .expect("change password");
        assert!(matches!(
            auth.session_user(&old_session.token).await,
            Err(AuthError::InvalidSession)
        ));
        assert!(matches!(
            auth.login("admin", "correct horse battery staple").await,
            Err(AuthError::InvalidCredentials)
        ));
        assert_eq!(
            auth.login("admin", "a different secure password")
                .await
                .expect("new password")
                .user
                .id,
            new_session.user.id
        );
    }

    #[tokio::test]
    async fn totp_setup_requires_confirmation_and_gates_login() {
        let auth = AuthService::new("knotree-registry", "knotree-registry", 300);
        auth.bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let session = auth
            .login("admin", "correct horse battery staple")
            .await
            .expect("login");
        let setup = auth
            .begin_totp_setup(&session.token, "correct horse battery staple")
            .await
            .expect("begin setup");
        assert!(
            !auth
                .totp_status(&session.token)
                .await
                .expect("status")
                .enabled
        );
        assert!(matches!(
            auth.confirm_totp_setup(&session.token, "000000").await,
            Err(AuthError::InvalidTwoFactorCode)
        ));
        assert!(
            auth.login("admin", "correct horse battery staple")
                .await
                .is_ok()
        );
        let code = crate::totp::code_for_test(&setup.secret, now_seconds() / 30);
        assert!(
            auth.confirm_totp_setup(&session.token, &code)
                .await
                .expect("confirm")
                .enabled
        );
        assert!(matches!(
            auth.login("admin", "correct horse battery staple").await,
            Err(AuthError::TwoFactorRequired)
        ));
        let code = crate::totp::code_for_test(&setup.secret, now_seconds() / 30);
        assert!(
            auth.login_with_totp("admin", "correct horse battery staple", Some(&code))
                .await
                .is_ok()
        );
        let restored = AuthService::from_snapshot(
            "knotree-registry",
            "knotree-registry",
            300,
            auth.snapshot().await.expect("snapshot"),
        )
        .expect("restore");
        assert!(
            restored
                .totp_status(&session.token)
                .await
                .expect("status")
                .enabled
        );
        let code = crate::totp::code_for_test(&setup.secret, now_seconds() / 30);
        assert!(
            !restored
                .disable_totp(&session.token, "correct horse battery staple", &code)
                .await
                .expect("disable")
                .enabled
        );
    }

    #[tokio::test]
    async fn snapshot_round_trip_preserves_credentials_and_signing_key() {
        let auth = AuthService::new("knotree-registry", "knotree-registry", 300);
        auth.bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let session = auth
            .login("admin", "correct horse battery staple")
            .await
            .expect("login");
        let credential = auth
            .create_credential_for_session(
                &session.token,
                "ci".to_owned(),
                vec![scope("repository:team/app:pull,push")],
                None,
            )
            .await
            .expect("credential");
        let restored = AuthService::from_snapshot(
            "knotree-registry",
            "knotree-registry",
            300,
            auth.snapshot().await.expect("snapshot"),
        )
        .expect("restore");
        let minted = restored
            .mint_token(
                "admin",
                &credential.secret,
                "knotree-registry",
                &[scope("repository:team/app:pull")],
            )
            .await
            .expect("mint after restore");
        assert_eq!(minted.expires_in, 300);
        assert_eq!(
            restored
                .verify_bearer(
                    &minted.token,
                    &RepositoryName::parse("team/app").expect("repository"),
                    Action::Pull,
                )
                .await
                .expect("verify after restore")
                .username,
            "admin"
        );
    }
}
