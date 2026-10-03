//! Authentication primitives shared by the registry protocol and control plane.

mod jwt;
mod secret;
mod service;
mod totp;

pub use service::{
    AuthError, AuthService, CredentialCreated, CredentialSummary, MintedToken, RevokedCredential,
    Session, TotpSetup, TotpStatus, UserSummary, parse_scope,
};
