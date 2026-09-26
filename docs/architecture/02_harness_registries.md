# Harness 注册表

每次运行前,各档位组件往 HarnessDraft 的五个注册表里贡献条目(agents、tools、skills、上下文源、权限规则集),`Harness::resolve` 做能力覆盖校验后冻结成不可变快照;主循环每次调工具都走同一条流水线:guards → 工具本体 → 结果策略 → 观察者。详细设计见 docs/design/harness_m1.md。

```mermaid
flowchart LR
  subgraph source_grp["组件来源"]
    dev_profile["dev 档组件<br/>profiles/dev.rs"]:::entry
    research_profile["research 档组件<br/>profiles/research.rs"]:::entry
    readonly_profile["只读档<br/>profiles/readonly.rs"]:::muted
    config["kanzei.toml<br/>全局 → 项目层叠"]
    defs["agent / skill 定义<br/>defs.rs"]
  end
  subgraph draft_grp["HarnessDraft · 五个注册表"]
    agents["agents<br/>AgentDef"]
    tools_reg["tools<br/>Tool 实现"]
    skills["skills<br/>SkillDef"]
    context["上下文源<br/>ContextSource"]
    rules["权限规则集<br/>Ruleset"]
  end
  resolve["Harness::resolve<br/>能力覆盖校验"]:::focus
  snapshot["HarnessSnapshot<br/>本次运行的不可变快照"]
  runner["kanzei-core 主循环<br/>runner/drive.rs"]
  pipeline["工具流水线<br/>guards → 本体 → policies → observers"]
  design_doc["设计文档<br/>harness_m1.md"]:::ext

  dev_profile --> tools_reg
  dev_profile --> context
  dev_profile --> rules
  research_profile --> tools_reg
  readonly_profile -.-> rules
  config --> rules
  defs --> agents
  defs --> skills
  agents --> resolve
  tools_reg --> resolve
  skills --> resolve
  context --> resolve
  rules --> resolve
  resolve --> snapshot
  snapshot --> runner
  runner --> pipeline
  snapshot -.-> design_doc

  click dev_profile "crates/kanzei-tools/src/profiles/dev.rs" "dev 档:注册工具、上下文源与权限规则"
  click research_profile "crates/kanzei-tools/src/profiles/research.rs" "research 档组件"
  click readonly_profile "crates/kanzei-tools/src/profiles/readonly.rs" "只读档:收紧写权限"
  click config "crates/kanzei-harness/src/config.rs" "kanzei.toml:全局 ~/.kanzei 再叠项目 .kanzei"
  click defs "crates/kanzei-harness/src/defs.rs" "AgentDef / SkillDef / ProfileKind"
  click agents "crates/kanzei-harness/src/harness.rs" "HarnessDraft 的五个注册表"
  click tools_reg "crates/kanzei-harness/src/tool.rs" "Tool trait"
  click skills "crates/kanzei-harness/src/defs.rs" "SkillDef"
  click context "crates/kanzei-harness/src/context.rs" "ContextSource:每轮渲染的上下文片段"
  click rules "crates/kanzei-harness/src/permission.rs" "Ruleset:允许 / 询问 / 拒绝"
  click resolve "crates/kanzei-harness/src/harness.rs" "组件按序贡献,装配末尾校验专用工具都已注册"
  click snapshot "crates/kanzei-harness/src/harness.rs" "HarnessSnapshot"
  click runner "crates/kanzei-core/src/runner/drive.rs" "run_once_with_parts"
  click pipeline "crates/kanzei-harness/src/tool_pipeline.rs" "run_tool_pipeline:唯一流水线"
  click design_doc "docs/design/harness_m1.md" "Harness 设计文档"
```
