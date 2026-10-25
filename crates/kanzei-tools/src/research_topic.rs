//! Conversation topic identity and its artifact directory, without workflow policy.
use std::path::{Path, PathBuf};

use kanzei_harness::{
    refreshing_fallible_source, Component, ContextRequirement, HarnessDraft, ProfileKind,
    ResolveCtx,
};

/// Resolve an existing topic within the project's research directory.
pub fn directory(root: &Path, topic: &str) -> Result<PathBuf, String> {
    kanzei_memory::docstore::DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    let research = root
        .join(".kanzei/research")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let directory = research
        .join(topic)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !directory.is_dir() || !directory.starts_with(&research) {
        return Err("研究课题目录越界".into());
    }
    Ok(directory)
}

pub struct ResearchTopicContext(pub String);

impl Component for ResearchTopicContext {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Research {
            return Ok(());
        }
        let topic = self.0.clone();
        draft.context.insert(
            "research/topic",
            refreshing_fallible_source("research/topic", ContextRequirement::Required, move |ctx: &ResolveCtx| {
                let directory = directory(&ctx.project_root, &topic).map_err(anyhow::Error::msg)?;
                Ok(Some(format!("<research_topic>\n当前对话关联的课题及工件目录：\ntopic: {topic}\nartifact_root: {}\n</research_topic>", directory.display())))
            }),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::{config::KanzeiConfig, Harness};
    use std::sync::Arc;

    #[test]
    fn binding_is_refreshing_factual_context_independent_of_old_workflow_files() {
        let directory = kanzei_base::test_support::TestDir::new("kz-research-topic-context-");
        let root = directory.as_path();
        let topic = root.join(".kanzei/research/topic-one");
        std::fs::create_dir_all(&topic).unwrap();
        std::fs::write(topic.join("workflow.json"), "OBSOLETE_PHASE_BUDGET_CANARY").unwrap();
        let mut harness = Harness::default();
        harness.add(ResearchTopicContext("topic-one".into()));
        let snapshot = harness
            .resolve(&ResolveCtx {
                cwd: root.join("worktrees/a"),
                project_root: root.into(),
                profile: ProfileKind::Research,
                config: Arc::new(KanzeiConfig::default()),
            })
            .unwrap();
        let (text, sources) = snapshot.refreshable_system_baseline_with_report().unwrap();
        assert!(text.contains("topic: topic-one"));
        assert!(text.contains(&topic.canonicalize().unwrap().display().to_string()));
        assert!(!text.contains("OBSOLETE_PHASE_BUDGET_CANARY"));
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].key, "research/topic");
        assert_eq!(sources[0].requirement, ContextRequirement::Required);
        assert!(snapshot
            .stable_system_baseline_with_report()
            .unwrap()
            .0
            .is_empty());
        assert!(!snapshot
            .materialize_tools()
            .iter()
            .any(|tool| tool.name() == "research_workflow"));
        // A disappeared binding must be reported, not silently replaced with another topic.
        std::fs::rename(&topic, root.join(".kanzei/research/moved-topic")).unwrap();
        assert!(snapshot.refreshable_system_baseline_with_report().is_err());
        assert!(super::directory(root, "../outside").is_err());
    }
}
