use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositoryName(String);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RepositoryError {
    #[error("repository name is empty")]
    Empty,
    #[error("repository name must be lowercase")]
    Uppercase,
    #[error("repository name contains an invalid component")]
    InvalidComponent,
    #[error("repository name is too long")]
    TooLong,
}

impl RepositoryName {
    pub fn parse(value: &str) -> Result<Self, RepositoryError> {
        if value.is_empty() {
            return Err(RepositoryError::Empty);
        }
        if value.len() > 255 || value.contains(char::is_whitespace) {
            return Err(RepositoryError::TooLong);
        }
        if value != value.to_ascii_lowercase() {
            return Err(RepositoryError::Uppercase);
        }
        if value.starts_with('/') || value.ends_with('/') || value.contains("//") {
            return Err(RepositoryError::InvalidComponent);
        }
        if value.split('/').any(|component| {
            component.is_empty()
                || component.starts_with(['.', '-', '_'])
                || component.ends_with(['.', '-', '_'])
                || component.bytes().any(|byte| {
                    !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte))
                })
        }) {
            return Err(RepositoryError::InvalidComponent);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for RepositoryName {
    type Err = RepositoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for RepositoryName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_nested_lowercase_repository_names() {
        let name = RepositoryName::parse("team/platform-api").expect("valid repository");
        assert_eq!(name.as_str(), "team/platform-api");
    }

    #[test]
    fn rejects_names_that_are_ambiguous_or_not_docker_compatible() {
        assert_eq!(
            RepositoryName::parse("Team/app"),
            Err(RepositoryError::Uppercase)
        );
        assert_eq!(
            RepositoryName::parse("team//app"),
            Err(RepositoryError::InvalidComponent)
        );
        assert_eq!(
            RepositoryName::parse("team/app?debug"),
            Err(RepositoryError::InvalidComponent)
        );
    }
}
