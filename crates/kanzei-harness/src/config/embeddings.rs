//! 向量检索配置域(R-257 B5):EmbeddingsSection 结构。自 config.rs 原样迁出,
//! 零行为变更。

use serde::{Deserialize, Serialize};

/// 向量检索通道配置(R-164)。两个字段都带 serde default:
/// 旧配置没有 `[embeddings]` 节时通道关闭,检索退化为 lexical(设计 §5 验收①)。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EmbeddingsSection {
    /// provider 名(providers 表里的键,如 "ollama")。None = 通道关闭。
    #[serde(default)]
    pub provider: Option<String>,
    /// 模型名(如 "nomic-embed-text" / "text-embedding-3-small")。
    #[serde(default)]
    pub model: Option<String>,
}

impl EmbeddingsSection {
    /// 通道是否启用:provider 与 model 都配置了才生效。
    pub fn enabled(&self) -> bool {
        self.provider.as_deref().is_some_and(|p| !p.is_empty())
            && self.model.as_deref().is_some_and(|m| !m.is_empty())
    }
}

/// kanzei.toml [embeddings] 节已知键名单(R-220 单源)。
pub(crate) const EMBEDDINGS_KEYS: &[&str] = &["provider", "model"];
