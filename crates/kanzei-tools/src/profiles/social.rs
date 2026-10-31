//! Social execution is an independent capability set, even with auto-allow or custom Agents.
use super::*;

pub struct SocialWorkspaceScope;

impl Component for SocialWorkspaceScope {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        let Some(workspace) = kanzei_harness::social_workspace_root(&ctx.project_root) else {
            return Ok(());
        };
        draft.tools = Default::default();
        draft.deferred_tools.clear();
        draft.skills = Default::default();
        draft.context = Default::default();
        draft.agents = Default::default();
        draft.agent_sources.clear();
        draft.permissions = Default::default();
        crate::base::add_source_policy(draft);
        draft.tools.insert(
            "memory_search",
            Arc::new(kanzei_memory::memory::MemorySearchTool),
        );
        draft.tools.insert(
            "memory_note",
            Arc::new(kanzei_memory::memory::MemoryNoteTool),
        );
        draft.tools.insert(
            "sandbox",
            Arc::new(crate::sandbox::WorkspaceSandboxTool::new(workspace)),
        );
        draft
            .tools
            .insert("webfetch", Arc::new(crate::webfetch::WebFetchTool));
        draft.tools.insert(
            "websearch",
            Arc::new(crate::websearch::WebSearchTool::new(ctx.profile)),
        );
        draft.permissions.push(rule("*", "*", Effect::Allow));
        for name in ["general", "dev-pair", "dev-auto", "isolated"] {
            draft.agents.insert(name, AgentDef {
                name: name.into(), profile: ProfileScope::All, model: "primary".into(),
                mode: AgentMode::Primary, steps: 0,
                system: "你是 kanzei 养的大肥鱼，在独立社交工作区里聊天和做事。个人助手、QQ 好友和 B站共享个人记忆；群聊只使用本群记忆。所有命令、文件读写和计算都使用 sandbox，工作目录 /workspace 的文件会保留。你没有宿主终端、宿主文件、桌面控制、浏览器登录态或子代理能力。上网查资料使用 websearch 和 webfetch；沙箱命令自身不联网。不能读取本机路径、localhost 或内网服务。按真实工具结果说明完成状态，沙箱不可用时直接说明，不能改用宿主执行。".into(),
            });
        }
        Ok(())
    }
}
