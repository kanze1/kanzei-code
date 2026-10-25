//! Durable references accompany input text without becoming instructions.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAttachment {
    pub name: String,
    pub media_type: String,
    pub path: PathBuf,
    pub original_path: PathBuf,
    pub bytes: u64,
    pub content_hash: String,
}
