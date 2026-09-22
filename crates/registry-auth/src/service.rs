use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use registry_core::{Action, RepositoryName, RepositoryScope};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    jwt::{AccessEntry, JwtIssuer, RegistryClaims, now_seconds},
    secret::{SecretVerifier, issue_secret, raw_digest},
};

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct UserSummary {
    pub id: Uuid,
    pub username: String,
    pub is_admin: bool,
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
}

#[derive(Clone)]
struct UserRecord {
    id: Uuid,
    username: String,
    password_hash: String,
    is_admin: bool,
}

#[derive(Clone)]
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

struct SessionRecord {
    user_id: Uuid,
    expires_at: u64,
    revoked: bool,
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
            })),
            issuer: JwtIssuer::new(&issuer_name),
            issuer_name,
            service: service.into(),
            token_ttl: Duration::from_secs(token_ttl_seconds.clamp(60, 3600)),
        }
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
        };
        let summary = summary(&user);
        state.users.insert(user.username.clone(), user);
        Ok(summary)
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<Session, AuthError> {
        let user = {
            let state = self.state.read().await;
            state.users.get(username).cloned()
        }
        .ok_or(AuthError::InvalidCredentials)?;
        verify_password(&user.password_hash, password)?;
        let (token, _, _) = issue_secret("kntr_session_");
        let expires_at = now_seconds().saturating_add(8 * 60 * 60);
        self.state.write().await.sessions.insert(
            raw_digest(&token),
            SessionRecord {
                user_id: user.id,
                expires_at,
                revoked: false,
            },
        );
        Ok(Session {
            token,
            user: summary(&user),
            expires_at,
        })
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
        let result = CredentialCreated {
            id: credential.id,
            name,
            prefix,
            secret,
            scopes,
            expires_at,
        };
        let mut state = self.state.write().await;
        if !state.users.values().any(|user| user.id == user_id) {
            return Err(AuthError::InvalidCredentials);
        }
        state
            .credential_prefixes
            .insert(credential.prefix.clone(), credential.id);
        state.credentials.insert(credential.id, credential);
        Ok(result)
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
        state
            .users
            .values()
            .find(|user| user.id == user_id)
            .map(summary)
            .ok_or(AuthError::InvalidCredentials)
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
}
