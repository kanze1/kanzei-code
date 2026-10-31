//! Application-owned QQ conversation identities. A peer never supplies a storage path.
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QqPeerKind {
    Friend,
    Group,
}

impl QqPeerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Friend => "friend",
            Self::Group => "group",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QqConversation {
    pub account: String,
    pub kind: QqPeerKind,
    pub peer: String,
}

impl QqConversation {
    pub fn new(account: &str, kind: QqPeerKind, peer: &str) -> anyhow::Result<Self> {
        for value in [account, peer] {
            let id: u64 = value.parse()?;
            anyhow::ensure!(
                id > 0 && id.to_string() == value,
                "QQ identity must be a canonical positive integer"
            );
        }
        Ok(Self {
            account: account.into(),
            kind,
            peer: peer.into(),
        })
    }

    pub fn root_at(&self, home: &Path) -> PathBuf {
        home.join("conversations/qq")
            .join(&self.account)
            .join(self.kind.label())
            .join(&self.peer)
    }

    pub fn root(&self) -> anyhow::Result<PathBuf> {
        Ok(self.root_at(
            &crate::kanzei_home().ok_or_else(|| anyhow::anyhow!("Kanzei home unavailable"))?,
        ))
    }

    pub fn from_root_at(root: &Path, home: &Path) -> Option<Self> {
        let kind_dir = root.parent()?;
        let account_dir = kind_dir.parent()?;
        if !crate::project_root::is_same_dir(account_dir.parent()?, &home.join("conversations/qq"))
        {
            return None;
        }
        let kind = match kind_dir.file_name()?.to_str()? {
            "friend" => QqPeerKind::Friend,
            "group" => QqPeerKind::Group,
            _ => return None,
        };
        Self::new(
            account_dir.file_name()?.to_str()?,
            kind,
            root.file_name()?.to_str()?,
        )
        .ok()
    }

    pub fn from_root(root: &Path) -> Option<Self> {
        Self::from_root_at(root, &crate::kanzei_home()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespaces_separate_accounts_friends_and_groups() {
        let dir = tempfile::tempdir().unwrap();
        let friend = QqConversation::new("100", QqPeerKind::Friend, "200").unwrap();
        let group = QqConversation::new("100", QqPeerKind::Group, "200").unwrap();
        let other = QqConversation::new("101", QqPeerKind::Friend, "200").unwrap();
        assert_ne!(friend.root_at(dir.path()), group.root_at(dir.path()));
        assert_ne!(friend.root_at(dir.path()), other.root_at(dir.path()));
        std::fs::create_dir_all(friend.root_at(dir.path())).unwrap();
        assert_eq!(
            QqConversation::from_root_at(&friend.root_at(dir.path()), dir.path()),
            Some(friend.clone())
        );
        assert!(QqConversation::from_root_at(
            &friend.root_at(dir.path()).join("artifacts"),
            dir.path()
        )
        .is_none());
        for invalid in ["../200", "0200", "0", "", "group"] {
            assert!(QqConversation::new("100", QqPeerKind::Friend, invalid).is_err());
        }
    }
}
