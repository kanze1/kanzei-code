# Harness M1 设计稿

> 状态:已评审通过(2026-08-06)。Q1=纯 markdown;Q2=硬 deny;Q3=强制引用。本文档随实现演进。

## 0. 一句话

一切喂给模型的东西都是 **Harness 组件**,汇入六个注册表;每轮对话解析成不可变快照;所有规则走**代码硬门禁**(拦截器链),不靠提示词恳求;其上提供两套**模式(Profile)**:软件开发模式(dev)和研究模式(research)。

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
    commands:    Registry<CommandDef>,      // slash 命令模板
    skills:      Registry<SkillDef>,        // 索引进提示词,正文走 skill 工具
    context:     Registry<ContextSource>,   // baseline/update/removal 三段式渲染
    permissions: RulesetBuilder,            // 有序规则
}

trait Component { fn contribute(&self, draft: &mut HarnessDraft) -> Result<()>; }
```

- **组件源**:内置 → kanzei.toml → markdown 目录(`.kanzei/{agents,commands,skills}/`)→(M4)MCP。配置本身就是组件,没有第二条 config→runtime 路径。
- **快照**:轮次边界 `resolve(profile) -> Arc<HarnessSnapshot>`;确定性排序、同名 last-wins;Arc 共享零复制。
- **拦截器链(硬门禁落点)**:
  - `before_request`:注入 Context Source、系统提示词预算检查(baseline >2k token 报警);
  - `before_tool_call`:权限 Ruleset 检查(ask 弹给用户)→ profile 工具过滤 → 输入宽容解析+schema 校验(失败回喂纠错,不崩);
  - `after_tool_result`:写后校验 warning、输出截断、(dev)文档一致性检查。

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

**行为约定**(写进 dev agent 提示词,一句话级):开始做一件事→对应需求置 doing;发现 bug→先记 defect 再修;完成→更新状态。

## 5. 研究模式(research)

```
.kanzei/research/<topic>/
├── sources.md    # S-001 来源:URL/文献,抓取时间,要点摘录
├── findings.md   # F-001 发现:结论 + 证据(必须引用 S-xxx)
└── report.md     # 最终报告(自由写作,普通 write 即可)
```

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

`.kanzei/skills/<name>/SKILL.md`:frontmatter `{name, description}`,索引进提示词,正文走 skill 工具按需加载。
`.kanzei/commands/*.md`:frontmatter `{name, description, agent?}`,正文模板支持 `$ARGUMENTS`/`$1..$N`/`@file`。

内置 agent:`dev`(dev 模式主力)、`research`(研究模式主力)、`explore`(subagent,默认 model: fast,只读工具)。

## 7. 实现顺序(M1 内)

1. 核心类型:六注册表 + Snapshot + Component trait + resolve
2. kanzei.toml 解析(toml crate)+ 层叠合并 + providers/models 角色
3. 权限 Ruleset(last-match-wins + wildcard)+ 拦截器链框架
4. 工具修复回路完整版(宽容 JSON 解析:尾逗号/单引号/裸键)
5. dev profile:req/defect 工具 + 文档序列化 + Context Source
6. research profile:source/finding 工具 + 引用校验
7. markdown 组件源(agents/skills/commands 目录扫描)
8. CLI 接线:`kz run` 走 harness resolve;`kz req list` 等人用入口

## 8. 已决事项

- Q1:**纯 markdown**——人可用任何编辑器直接读改,工具解析宽容(用户手改后格式轻微走样不报错,工具下次写入时归一)。
- Q2:**硬 deny**——模型的 write/edit 碰 `.kanzei/project/{requirements,defects}.md` 直接拒绝,只能走 req/defect 工具;用户本人手改不受限。
- Q3:**强制引用**——finding 必须挂至少一个存在的 S-xxx,否则拒绝并回喂现有 source 列表。
