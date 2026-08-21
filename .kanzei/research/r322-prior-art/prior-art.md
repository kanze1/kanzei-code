---
kind: prior_art
topic: r322-prior-art
status: complete
trigger: core_requirement
entry_refs: R-322
websearch_round_limit: 4
---

# 先行方案对照

对照主题:agent 循环的控制权归属——引擎(harness)与模型各自拥有多少停机/编排决定权,
以及门禁强度能否按「有人监督 / 无人监督」显式分档。

## 外部已有实现

### Claude Code 的 Hook 事件体系与 Stop 钩位

- 出处: https://code.claude.com/docs/en/hooks
- 证据等级: V2
- 事实: 提供 31 个钩位,其中与控制权直接相关的有 `Stop`(原文:"When Claude finishes
  responding")、`StopFailure`、`PreToolUse`(原文:"Before a tool call executes.
  Can block it")、`PermissionRequest`、`PermissionDenied`、`PostToolBatch`、
  `TeammateIdle`。停机的**默认**归属是模型——`Stop` 是「模型已经决定结束」之后才触发
  的扩展点,不是引擎在模型之前作出的否决;要让循环继续必须由用户显式配置 Hook。
- 差异: 本仓 `auto_run` FSM 默认就持有反向否决权(`AutoRunAction::Nudge`),
  无需任何用户配置即生效;且结伴与自主两种模式共用同一套门禁机械,差异只落在系统提示词。
- 决策: **采用其停机权归属**——模型声明完成即终止,引擎不再追加推进指令。
  **不采用其「强制力全靠用户配 Hook」**:无人监督的过夜自主推进恰恰没有用户在场配 Hook,
  资源类兜底(限流/致命错误/连数上限/真实进展签名不变)必须留在引擎侧默认开启。
  另注:`PostToolBatch`(整批并行工具调用解析后触发)说明其并行执行是**批**语义,
  与本仓 wave 同构,可作为 D-661 调度改造的对照点。

### Codex CLI 的 sandbox_mode × approval_policy 双旋钮

- 出处: https://learn.chatgpt.com/docs/agent-approvals-security
- 证据等级: V2
- 事实: 两个**正交**维度。`sandbox_mode`(read-only / workspace-write /
  danger-full-access)决定「技术上能做什么」,由 OS 强制——macOS 走 Seatbelt 的
  `sandbox-exec`,Linux 走 `bwrap` + `seccomp`,Windows 有原生沙箱实现(WSL2 走 Linux 路径)。
  `approval_policy`(untrusted / on-request / never)决定「什么时候停下来问」,
  是叠加在沙箱之上的工作流选择。原文明确区分:sandbox 控制 what Codex can technically do,
  approval 控制 when Codex must ask permission。
- 差异: 本仓只有 agent 名(`dev` / `dev-pair`)这一个隐式旋钮,且它同时决定提示词、
  自主推进许可与阶段流水线可用性——三件事耦合在一个下拉框里,用户看不见自己在选什么。
- 决策: **采用其「监督强度是显式且独立于人格的维度」**——把门禁强度提成一等概念
  (`HarnessIntensity`),与 agent 人格解耦并在界面呈现。
  **不采用其 OS 沙箱路线**:托管文档必须对专用工具可写,内核层禁写区分不了
  「专用工具合法写入」与「shell 越界」,这正是本仓走结果侧围栏的原因
  (见 file:crates/kanzei-tools/src/managed.rs:1 模块头)。

## 仓内既有设计

### continue_prompt_dissection.md §4 / auto_run 状态机

- 出处: file:docs/design/continue_prompt_dissection.md:1
- 证据等级: V3
- 事实: 轮末判定全部下沉引擎,前端只执行。立论是「规则写在用户可编辑文案里会与引擎
  行为脱节(D-120/D-128/D-163)」。`Nudge` 是其中唯一一条**引擎否决模型判断**的动作:
  模型本轮没有实质动作时,引擎追加推进指令强制其继续。
- 差异: 该立论解决的是「规则应当写在哪里」,不等于「引擎应当拥有停机权」。两者被一并
  实现,但只有前者有证据支撑。
- 决策: **保留判定下沉**(规则仍在引擎,不回退到提示词);**收回引擎的停机否决权**——
  `Nudge` 改为受门禁强度门控,且模型显式声明完成后一律不触发。

### R-199 的 auto_allowed 档位条件

- 出处: file:crates/kanzei-app/src/run/coordinator.rs:367
- 证据等级: V3
- 事实: `auto_allowed = profile==Dev && agent.name=="dev"`,即结伴档(`dev-pair`)已经
  不允许自主推进。这是仓内**已经存在的**「按模式分档」先例,但粒度只有全开/全关一档,
  且只作用于自主推进这一个机制。
- 差异: 只有一个二值条件、只管一个机制;强度既不可见也无法在结伴档内再分级,
  用户无法表达「这次结伴但我要重门禁」或「这次自主但只跑轻门禁」。
- 决策: **采用并推广**——把这条二值条件泛化成 `HarnessIntensity`,让 Nudge、
  验收核查轮、冗余提醒等机制按同一维度分档,而不是各自再长一个布尔开关。

### phase_pipeline「不构造编排对象就是关」

- 出处: file:crates/kanzei-app/src/phase_pipeline.rs:1
- 证据等级: V3
- 事实: 阶段流水线用「构造与否」表达开关,拒绝再设第二个布尔值,理由是「没有第二个
  开关可以配错」。
- 差异: 该原则对单个机制成立,但机制数量增长后,用户面对的是 N 个互不关联的开关
  (`phase_pipeline_enabled` 已是第二个)。
- 决策: **采用其精神**(不为同一件事设两个开关),**方式上升级**为单一强度枚举驱动
  多个机制,避免 N 个布尔值互相矛盾。
