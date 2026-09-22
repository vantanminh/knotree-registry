use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Serialize};

use crate::RepositoryName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Pull,
    Push,
    Delete,
    Admin,
}

impl fmt::Display for Action {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pull => "pull",
            Self::Push => "push",
            Self::Delete => "delete",
            Self::Admin => "admin",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryScope {
    pub repository: RepositoryName,
    pub actions: BTreeSet<Action>,
}

impl RepositoryScope {
    pub fn allows(&self, action: Action) -> bool {
        self.actions.contains(&Action::Admin) || self.actions.contains(&action)
    }
}
