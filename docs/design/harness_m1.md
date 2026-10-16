# Harness M1 设计稿

- 身份: live_design
- 状态: 现行总纲；R-317 执行层已接管 Requirement/Work Unit 的工作权威
- 最近核验依据: 2026-10-17 Harness 补充审计；MarkdownComponent、全局 Skills catalog、core runner 执行边界
- 关联需求: R-023、R-028、R-317 (done)
- 执行模型权威: R-317 的 Outcome/Work Unit/事件投影；本文注册表、Profile 和硬门禁仍是实现约束

> 状态:已评审通过(2026-08-06)。Q1=纯 markdown;Q2=硬 deny;Q3=强制引用。本文档随实现演进。

> **文档状态(2026-10-17 核验):现行架构基线。** 五类装配集合（agents/tools/skills/context/permissions；commands 已移除）与 dev/research 双 profile 已实现。Skills 来自全局 catalog，正文由 `skill_read` 按需读取；权限、审批、取消和结果事件由 core runner 强制执行。文中 R-023/R-028 注记是历史交付记录，当前实现以代码及 `.kanzei/project/architecture/` 索引为准。

> **2026-08-21 修订(R-322)**:本文「所有规则走代码硬门禁」需加一条限定——**硬门禁的
> 强度不是常量**。权限 Ruleset、托管围栏与事件真源(约束副作用边界的部分)在所有档位恒定;
> 而引擎对**任务判断**的介入(Nudge、验收核查轮、冗余提醒)按 `HarnessIntensity`
> (Paired / Autonomous)分档。判定归属的拆分口径见
> `model_autonomy_and_harness_intensity.md` §讨论摘要。

## 0. 一句话

模型可用能力和项目上下文通过 **Harness 组件**装配为轮次快照；core runner 消费快照并执行权限、审批、取消与结果记录。权限和托管写入边界走**代码硬门禁**，任务判断按 HarnessIntensity 分档。可选的工具输入 Guard 不代替 runner 门禁。其上提供两套**模式(Profile)**：软件开发模式(dev)和研究模式(research)。

## 1. 配置:kanzei.toml

发现顺序(后写覆盖先写,同 opencode 的 config 层叠):
`~/.kanzei/kanzei.toml`(全局)→ 项目根 `.kanzei/kanzei.toml`(从 cwd 向上找)。

```toml
# ---- 模型:按角色引用 ----
[models]
primary = "anthropic:claude-sonnet-5"   # 主循环
fast    = "ollama:qwen3"                # 简单工具调用快速响应 / 并行子代理 / 杂活(标题、压缩摘要)

[providers.anthropic]
protocol = "anthropic"                  # anthropic | openai
base_url = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"

[providers.ollama]
protocol = "openai"
base_url = "http://127.0.0.1:11434/v1"  # loopback 自动直连,不走代理

[providers.kimi]
protocol = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key_env = "MOONSHOT_API_KEY"

# ---- 网络 ----
proxy = "env"                           # "env"(默认,读环境变量)| "http://..." | "off"

# ---- 模式 ----
[profile]
default = "dev"                         # dev | research,session 级可切换

# ---- 权限:有序规则,last-match-wins,无匹配默认 ask ----
[[permissions.rules]]
action = "bash"
resource = "git status*"
effect = "allow"

[[permissions.rules]]
action = "write"
resource = ".kanzei/project/*"
effect = "deny"    # dev 模式项目文档只能走专用工具(见 §4)
```

## 2. Harness 核心

```rust
struct HarnessDraft {
    agents:      Registry<AgentDef>,
    tools:       Registry<Arc<dyn Tool>>,
    skills:      Registry<SkillDef>,        // 全局 catalog；正文由 skill_read(id) 按需读取
    context:     Registry<Arc<dyn ContextSource>>, // stable / refreshable 上下文
    permissions: Ruleset,                   // 有序规则与托管资源边界
}

trait Component { fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> Result<()>; }
```

- **组件源**：调用方按顺序加入内置 profile、配置、Markdown 和宿主能力组件。Agent 文件先读 `KANZEI_HOME/agents`（默认 `~/.kanzei/agents`），再读项目 `.kanzei/agents`；项目同名定义覆盖全局定义。Skills 使用下面 §6 的独立全局 catalog，项目目录不参与。
- **快照**：轮次边界 `resolve(&ResolveCtx) -> Result<Arc<HarnessSnapshot>>`；同名注册保留位置、后写覆盖，Arc 共享不可变快照。`ResolveCtx` 显式携带 profile、代码 cwd、项目根和配置。
- **执行边界**：
  - Harness 负责组件装配、agent 选择、工具可见面、上下文来源和 Ruleset。工具 schema 的可见性不等于执行授权。
  - core runner 组装 stable / refreshable system、估算 system + schema + messages 的预算，并在每次工具调用前执行权限/Ask gate。取消、进度、panic 与 ToolEnd/结果配对也归 runner。
  - 工具本体负责参数解释和具体副作用；少量工具使用共享输入 Guard。托管写入围栏由相应文件/命令工具执行，不能从“用了工具包装函数”推断已覆盖所有门禁。
  - CLI 与桌面共用 Harness/core 合同；桌面通过宿主组件增加前端操作能力及 UI 审批交互，CLI 使用自身的输入/审批适配。宿主能力不能由一个 Markdown agent 自动获得。

## 3. Profile 机制

Profile = 一组组件的成套启用:agents + tools + context sources + 权限收窄 + **工作区文档**。session 创建时定 profile,可切换(切换=下个轮次边界重新 resolve)。

## 4. 软件开发模式(dev)

**核心:统一项目文档,agent 全程维护,任何 session 断点接上都不丢。**

```
.kanzei/project/
├── requirements.md   # 需求清单
└── defects.md        # 缺陷追踪
```

条目格式(markdown,人可直接读;结构由工具保证):

```markdown
## R-012 支持本地模型接入 [doing]
- 验收: kz 用 ollama 走通含工具调用的多轮循环
- 备注: loopback 需绕过代理
- 关联: D-003
```
```markdown
## D-003 代理把 127.0.0.1 请求送进代理 [fixed] (high)
- 复现: KANZEI_PROXY 设置后 ollama 超时
- 根因: Proxy::all 无 loopback 豁免
- 修复: proxy.rs is_loopback 硬豁免
```

**硬门禁**(全部代码强制):
1. 读写只能走专用工具 `req` / `defect`(action: list/get/add/update/close);`write`/`edit` 对这两个文件 **deny**(权限规则,§1 示例)。
2. ID 由工具分配(R-/D- 递增),状态机受限(todo→doing→done/dropped;open→fixing→fixed/wontfix),非法流转直接拒绝并回喂正确选项。
3. 格式由工具序列化,模型只提供字段值——文档永远不会被写坏。
4. Context Source 注入**索引**(ID+标题+状态,预算上限 ~300 token);正文按需 `req get R-012`。

**当前实现状态（R-028，2026-08-06）**：dev profile 已提供 `todowrite`，每次调用整体替换当前运行计划，状态限定为 `todo/doing/done/dropped`，并通过 `ToolOutput.display.kind=todo` 发送有界计划数据；桌面端右侧“当前计划”面板显示状态和完成比例。计划只属于当前运行，不写入项目 backlog。

### 4.1 现行执行层（R-317）

- `Requirement`/`Defect` 工具仍用于维护项目结果、来源和兼容性元数据；它们不是当前 Work Unit 的执行历史真源。
- R-317 的 Outcome 描述用户可验收结果，Work Unit 描述一次可执行 scope，并由 claim、checkpoint、block/unblock、verify、evidence、complete 事件投影形成可恢复历史。
- Harness 在每轮消费的是有界 Work Unit context 与事件投影；Requirement 正文中的旧 todo/doing/done、轮内 `todowrite` 和历史计划只作为 legacy 展示，不得覆盖 Work Unit 终态。
- 该执行层与组件注册、Profile、权限门禁正交：R-317 提供执行事实，Harness 提供快照与规则，core/app 持有执行和持久化状态。


## 5. 研究模式(research)

```
.kanzei/research/<topic>/
├── sources.md    # S-001 来源:URL/文献,抓取时间,要点摘录
├── findings.md   # F-001 发现:结论 + 证据(必须引用 S-xxx)
└── report.md     # 最终报告(自由写作,普通 write 即可)
```

**当前实现状态（R-023，2026-08-06）**：research profile 已注册 `websearch`，通过 DuckDuckGo HTML 搜索页返回有界的 `query/results/truncated` JSON；每条结果包含 `title`、`url`、`snippet`。搜索请求复用 kanzei 代理配置，research 下放行，其他 profile 不暴露该工具。结果数、查询长度和响应体均有硬上限，解析失败或非 2xx 会返回工具错误。

**硬门禁**:
1. `source` / `finding` 专用工具,同 §4 的 ID/格式机制。
2. **finding 必须引用至少一个 S-xxx**,引用不存在的 ID 直接拒绝——结论可溯源是研究模式的底线。
3. 工具面:webfetch/websearch/read/recall 全开;write/edit 限 `.kanzei/research/**`;bash 默认 ask。
4. Context Source 注入 sources+findings 索引。

## 6. Agent / Skill / Command 文件格式

`.kanzei/agents/*.md`(frontmatter 即配置,正文即系统提示词):

```markdown
---
name: build
profile: dev            # 属于哪个 profile(all = 两者可用)
model: primary          # primary | fast | "provider:model" 直指
mode: primary           # primary | subagent
steps: 40
---
你是构建 agent,……(正文)
```

Skills 在所有项目和对话中全局可用，来源为：

- 内置目录（随程序嵌入，不依赖可写项目目录）。
- `KANZEI_HOME/skills`（默认 `~/.kanzei/skills`）的 personal Skills。
- 用户目录 `~/.codex/skills`、`~/.claude/skills`、`~/.agents/skills` 的兼容目录。

项目 `.kanzei/skills` 不参与 catalog。外部文件采用 `<name>/SKILL.md` 或 `<name>.md` 布局，YAML 元数据包含名称、描述及调用选项；身份使用 `source:name`，防止不同来源的同名技能被无声混淆。启用状态存于全局 `skills.json`。

`MarkdownComponent` 注册 `skill_read` 并将其标为 deferred。常驻上下文只提供可由模型调用的技能名称与描述；agent 通过 `tool_search` 加载 `skill_read` schema 后，以 `skill_read(id)` 读取正文。若工具加载器不可见，现有物化逻辑直接暴露允许工具的 schema；正文仍使用同一个读取接口。CLI 与桌面执行共用此机制。桌面另有全局 Skills 管理界面及 `$source:name` 显式正文展开；该宿主入口检查启用状态和 `user_invocable`，不是 CLI 自动具有的行为。带 `disable_model_invocation` 的技能不会进入模型自动发现列表，也不能经模型的 `skill_read` 直接加载。

`.kanzei/commands/*.md` 不再由 MarkdownComponent 扫描或注入(D-748);这些文件目前不具备运行效果。

顶层 agent 的可用目录由当前 profile 的快照决定；显式名称和默认选择都必须满足 profile scope 与 `Primary` 身份。`task` 的子代理名册由 `SubagentRuntime` 单独提供，不能把子代理定义当作顶层 agent 使用。

## 7. 历史实施顺序(M1 内)

以下记录最初 M1 的拆分顺序；现行能力与执行边界以 §2、§4.1、§6 为准。

1. 核心类型:五类注册表 + Snapshot + Component trait + resolve
2. kanzei.toml 解析(toml crate)+ 层叠合并 + providers/models 角色
3. 权限 Ruleset(last-match-wins + wildcard)+ 拦截器链框架
4. 工具修复回路完整版(宽容 JSON 解析:尾逗号/单引号/裸键)
5. dev profile:req/defect 工具 + 文档序列化 + Context Source
6. research profile:source/finding 工具 + 引用校验
7. markdown 组件源(agents/skills 目录扫描)
8. CLI 接线:`kz run` 走 harness resolve;`kz req list` 等人用入口

## 8. 已决事项

- Q1:**纯 markdown**——人可用任何编辑器直接读改,工具解析宽容(用户手改后格式轻微走样不报错,工具下次写入时归一)。
- Q2:**硬 deny**——模型的 write/edit 碰 `.kanzei/project/{requirements,defects}.md` 直接拒绝,只能走 req/defect 工具;用户本人手改不受限。
- Q3:**强制引用**——finding 必须挂至少一个存在的 S-xxx,否则拒绝并回喂现有 source 列表。
