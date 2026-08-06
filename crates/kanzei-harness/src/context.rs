//! Context Source:向 system baseline 贡献一段文本。
//! M1 只做 baseline;M2 引入 Context Epoch(baseline 字节不变 + 追加式更新)。

use std::sync::Arc;

use crate::harness::ResolveCtx;

pub trait ContextSource: Send + Sync {
    /// 稳定 key,如 "core/env"、"dev/requirements"。
    fn key(&self) -> &str;
    /// 渲染 baseline 片段;None = 本轮无内容。加载失败自行降级,不得 panic。
    fn baseline(&self, ctx: &ResolveCtx) -> Option<String>;
}

/// 用闭包快速定义 Context Source。
pub struct FnSource<F: Fn(&ResolveCtx) -> Option<String> + Send + Sync> {
    key: String,
    render: F,
}

pub fn source<F>(key: impl Into<String>, render: F) -> Arc<dyn ContextSource>
where
    F: Fn(&ResolveCtx) -> Option<String> + Send + Sync + 'static,
{
    Arc::new(FnSource { key: key.into(), render })
}

impl<F: Fn(&ResolveCtx) -> Option<String> + Send + Sync> ContextSource for FnSource<F> {
    fn key(&self) -> &str {
        &self.key
    }

    fn baseline(&self, ctx: &ResolveCtx) -> Option<String> {
        (self.render)(ctx)
    }
}
