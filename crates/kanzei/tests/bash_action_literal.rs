//! D-269 的分流判据是一个**跨 crate 的字面量耦合**,这条用例把它钉住。
//!
//! `kanzei-harness` 用 `permission::BASH_ACTION`(= `"bash"`)决定一条资源按 shell 文本
//! 还是按文件路径处理:`normalize_resource_for_action` 与 `resource_match_for_action`
//! 都只看这一个判据。action 名的另一端在 `kanzei-tools` 的 bash 工具 —— `Tool::action()`
//! 默认返回 `Tool::name()`,所以**把 bash 工具改个名(或单独给它实现一个 `action()`)**,
//! harness 那边就会静默走进路径分支:命令文本重新被 `..` 折叠、被 Windows 整串小写,
//! D-269 的提权洞原样回来,而且今天没有任何一条测试会因此变红。
//!
//! 这条用例必须放在下游 crate:`kanzei-harness` 是 `kanzei-tools` 的**上游**,拿不到那个
//! 工具来断言;`kanzei` 同时依赖两者,是最近的可断言处。
//!
//! **不按名字去取工具**(那样等于用被测量自己当尺):先按行为找出「产出结构化 bash 资源
//! `{"command":…,"workdir":…}` 的那个工具」,再断言它的 `action()`。工具改名后这条仍然
//! 找得到它,并且立刻红在 action 上。

use std::sync::Arc;

use kanzei_harness::permission::{
    is_structured_bash_resource, normalize_resource_for_action, BASH_ACTION,
};
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, Tool, ToolCtx};
use kanzei_tools::{BaseComponent, DevProfile};

/// 按 CLI 的装配方式起一份 dev profile 的工具集。
fn dev_tools() -> Vec<Arc<dyn Tool>> {
    let mut config = KanzeiConfig::default();
    config.fill_defaults();
    let root = std::env::temp_dir();
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: root.clone(),
        project_root: root,
        config: Arc::new(config),
    };
    let mut harness = Harness::default();
    harness.add(BaseComponent).add(DevProfile);
    harness.resolve(&rctx).unwrap().materialize_tools()
}

/// 行为判据:喂一份 bash 形态的输入,谁产出结构化 `{"command":…,"workdir":…}` 资源,
/// 谁就是权限层眼里的「bash 工具」。
fn structured_bash_tool(tools: &[Arc<dyn Tool>]) -> Arc<dyn Tool> {
    let ctx = ToolCtx {
        cwd: std::env::temp_dir(),
        project_root: std::env::temp_dir(),
        ..ToolCtx::default()
    };
    let input = serde_json::json!({ "command": "git status", "workdir": "." });
    let mut hits: Vec<Arc<dyn Tool>> = tools
        .iter()
        .filter(|tool| {
            tool.resources_with_ctx(&input, &ctx)
                .iter()
                .any(|resource| is_structured_bash_resource(resource))
        })
        .cloned()
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "只该有一个工具产出结构化 bash 资源，实际 {:?}",
        hits.iter().map(|t| t.name()).collect::<Vec<_>>()
    );
    hits.remove(0)
}

#[test]
fn 产出结构化bash资源的工具其action就是权限分流用的字面量() {
    let tools = dev_tools();
    let bash = structured_bash_tool(&tools);
    assert_eq!(
        bash.action(),
        BASH_ACTION,
        "工具 `{}` 产出的是结构化 bash 资源，它的 action 必须与 permission::BASH_ACTION 逐字节相同。\
         对不上就意味着 bash 资源会被当成文件路径规范化 —— 那正是 D-269 的提权洞:\
         `normalize_resource` 是为路径语义故意设计的非单射函数，施加到命令文本上，\
         一条规则准入的就不再是一条命令，而是它在该函数下的整个原像类。\
         改了工具名 / action 的人:要么把 BASH_ACTION 一起改，要么先弄明白为什么不能改。",
        bash.name()
    );
}

/// 顺带钉住这条耦合真正保护的东西:走那个工具的 `action()` 出来的资源,
/// 在规范化分流里必须**原样返回**,一个字节都不许动。
#[test]
fn 经由bash工具action的资源不被路径规范化() {
    let tools = dev_tools();
    let action = structured_bash_tool(&tools).action();
    // 这两个串在路径语义下会被改写:`/../` 弹掉前一段、Windows 上整串小写。
    for resource in [
        r#"{"command":"cargo test -p x/../; evil ;/y","workdir":"c:/proj"}"#,
        r#"{"command":"Get-Content A/./B","workdir":"c:/proj"}"#,
    ] {
        assert_eq!(
            normalize_resource_for_action(action, resource),
            resource,
            "bash 资源必须逐字节原样进评估"
        );
    }
}
