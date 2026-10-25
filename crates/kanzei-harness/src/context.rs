//! Context sources contribute complete system sections and report their read status.

use std::sync::Arc;

use crate::harness::ResolveCtx;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRequirement {
    Optional,
    Required,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSourceStatus {
    Loaded,
    Empty,
    Failed,
}

/// Metadata only: diagnostics never copy potentially large source contents into traces.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ContextSourceReport {
    pub key: String,
    pub requirement: ContextRequirement,
    pub refresh_each_step: bool,
    pub status: ContextSourceStatus,
    pub chars: usize,
    pub bytes: usize,
    pub error: Option<String>,
}

impl ContextSourceReport {
    pub fn failure_message(&self) -> Option<String> {
        self.error.as_ref().map(|error| {
            let outcome = match self.requirement {
                ContextRequirement::Optional => "可选上下文未加载，继续运行",
                ContextRequirement::Required => "必需上下文读取失败，已停止",
            };
            format!("{outcome}：{}：{error}", self.key)
        })
    }
}

#[derive(Debug)]
pub struct ContextAssemblyError {
    pub sources: Vec<ContextSourceReport>,
}

impl std::fmt::Display for ContextAssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "必需上下文读取失败，已停止装配；修复以下来源后重新运行："
        )?;
        for report in &self.sources {
            if report.requirement == ContextRequirement::Required {
                if let Some(error) = &report.error {
                    write!(f, "\n{}: {error}", report.key)?;
                }
            }
        }
        Ok(())
    }
}

impl std::error::Error for ContextAssemblyError {}

pub type ContextBaseline = Result<(String, Vec<ContextSourceReport>), ContextAssemblyError>;

pub trait ContextSource: Send + Sync {
    /// 稳定 key,如 "core/env"、"development/requirements"。
    fn key(&self) -> &str;
    /// None means legitimately absent content; read/parse failures must remain Err.
    fn baseline(&self, ctx: &ResolveCtx) -> anyhow::Result<Option<String>>;
    fn requirement(&self) -> ContextRequirement {
        ContextRequirement::Optional
    }
    /// true = 每个模型步骤前重新渲染,并替换本步骤的临时 system 段。
    /// 默认 false,既有项目文档/记忆上下文保持一次快照语义。
    fn refresh_each_step(&self) -> bool {
        false
    }
}

/// 用闭包快速定义 Context Source。
pub struct FnSource<F> {
    key: String,
    render: F,
    requirement: ContextRequirement,
    refresh_each_step: bool,
}

pub fn source<F>(key: impl Into<String>, render: F) -> Arc<dyn ContextSource>
where
    F: Fn(&ResolveCtx) -> Option<String> + Send + Sync + 'static,
{
    fallible_source(key, ContextRequirement::Optional, move |ctx| {
        Ok(render(ctx))
    })
}

/// 构造轮内可刷新的临时上下文源。它只进入每次请求的 system 段,不会追加到对话历史。
pub fn refreshing_source<F>(key: impl Into<String>, render: F) -> Arc<dyn ContextSource>
where
    F: Fn(&ResolveCtx) -> Option<String> + Send + Sync + 'static,
{
    refreshing_fallible_source(key, ContextRequirement::Optional, move |ctx| {
        Ok(render(ctx))
    })
}

pub fn fallible_source<F>(
    key: impl Into<String>,
    requirement: ContextRequirement,
    render: F,
) -> Arc<dyn ContextSource>
where
    F: Fn(&ResolveCtx) -> anyhow::Result<Option<String>> + Send + Sync + 'static,
{
    Arc::new(FnSource {
        key: key.into(),
        render,
        requirement,
        refresh_each_step: false,
    })
}

pub fn refreshing_fallible_source<F>(
    key: impl Into<String>,
    requirement: ContextRequirement,
    render: F,
) -> Arc<dyn ContextSource>
where
    F: Fn(&ResolveCtx) -> anyhow::Result<Option<String>> + Send + Sync + 'static,
{
    Arc::new(FnSource {
        key: key.into(),
        render,
        requirement,
        refresh_each_step: true,
    })
}

impl<F: Fn(&ResolveCtx) -> anyhow::Result<Option<String>> + Send + Sync> ContextSource
    for FnSource<F>
{
    fn key(&self) -> &str {
        &self.key
    }

    fn baseline(&self, ctx: &ResolveCtx) -> anyhow::Result<Option<String>> {
        (self.render)(ctx)
    }

    fn requirement(&self) -> ContextRequirement {
        self.requirement
    }

    fn refresh_each_step(&self) -> bool {
        self.refresh_each_step
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Harness, HarnessDraft, KanzeiConfig, ProfileKind};

    struct Sources(ContextRequirement);
    impl Component for Sources {
        fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
            draft
                .context
                .insert("loaded", source("loaded", |_| Some("约束 complete".into())));
            draft.context.insert("empty", source("empty", |_| None));
            draft
                .context
                .insert("blank", source("blank", |_| Some(" \n".into())));
            draft.context.insert(
                "unreadable",
                fallible_source("unreadable", self.0, |_| {
                    anyhow::bail!("fixture.md: invalid UTF-8")
                }),
            );
            Ok(())
        }
    }

    fn snapshot(requirement: ContextRequirement) -> Arc<crate::HarnessSnapshot> {
        let mut harness = Harness::default();
        harness.add(Sources(requirement));
        harness
            .resolve(&ResolveCtx {
                profile: ProfileKind::DevAuto,
                cwd: ".".into(),
                project_root: ".".into(),
                config: Arc::new(KanzeiConfig::default()),
            })
            .unwrap()
    }

    #[test]
    fn optional_failure_is_distinct_from_empty_and_loaded() {
        let (text, report) = snapshot(ContextRequirement::Optional)
            .system_baseline_with_report()
            .unwrap();
        assert_eq!(text, "约束 complete");
        assert_eq!(report.len(), 4);
        assert_eq!(report[0].status, ContextSourceStatus::Loaded);
        assert_eq!(report[0].chars, text.chars().count());
        assert_eq!(report[0].bytes, text.len());
        assert_eq!(report[1].status, ContextSourceStatus::Empty);
        assert_eq!(report[2].status, ContextSourceStatus::Empty);
        assert_eq!(report[3].status, ContextSourceStatus::Failed);
        assert!(report[3].error.as_deref().unwrap().contains("fixture.md"));
    }

    #[test]
    fn required_failure_refuses_baseline_and_retains_source_report() {
        let error = snapshot(ContextRequirement::Required)
            .system_baseline_with_report()
            .unwrap_err();
        assert!(error.to_string().contains("unreadable"));
        assert!(error.to_string().contains("invalid UTF-8"));
        assert_eq!(error.sources.len(), 4);
        assert_eq!(error.sources[3].requirement, ContextRequirement::Required);
    }
}
