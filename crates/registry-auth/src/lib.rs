//! Authentication primitives shared by the registry protocol and control plane.

mod jwt;
mod secret;
mod service;

pub use service::{
    AuthError, AuthService, CredentialCreated, MintedToken, Session, UserSummary, parse_scope,
};
