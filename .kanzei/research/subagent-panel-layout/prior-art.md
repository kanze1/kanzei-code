---
kind: prior_art
topic: subagent-panel-layout
status: complete
trigger: explicit_user
entry_refs: R-334
websearch_round_limit: 4
---

# 先行方案对照：子代理与活动呈现

## 0. 结论摘要

本轮采用“主对话为主、按需打开临时上下文浮层”的形态，而不是把子代理或活动作为永久占用宽度的右侧栏。外部资料共同确认的有效边界是：子代理有自己的可检查工作上下文/线程，活动是主任务的辅助反馈；用户需要能在需要时查看细节与结果，但不应为低频诊断内容持续牺牲主对话空间。

本轮不复制外部产品的后端对象或权限模型，只借鉴呈现层：保留仓内真实 `RunEvent`、活动条目、子代理 transcript/历史消费者；将两个现有 sibling 面板改成同一侧的临时浮层，始终最多打开一个。面板本身有显式关闭、键盘可达、滚动边界和空态，主区布局宽度不因打开面板而减少。

## 外部已有实现

### 1.1 Codex：子代理线程与活动结果回到主聊天上下文
- 出处: https://developers.openai.com/codex/subagents
- 证据等级: V2
- 证据深度: OpenAI 官方文档正文级；文档明确写出 Codex 的 subagent activity 出现在 ChatGPT desktop app、Codex CLI 和 IDE extension；app 会展示每个 subagent thread，用户可以 inspect 其 work，主 chat 收到 summary returned。文档没有在该入口承诺跨重启历史保留或具体展开动画，因此不对未说明部分做推断。
- 呈现位置: 主聊天所属的 activity/thread 入口，而非要求用户持续保留一个独立的固定侧栏；桌面 app、CLI、IDE 都有对应 activity surface。
- 对象模型: 一个主任务派生多个 subagent，各自执行 model/tool work，形成可检查的 subagent thread，并向主 chat 返回摘要。
- 展开/历史/状态反馈: 展示 subagent thread 供 inspect，结果以 summary 返回；该官方入口没有规定本仓应复制的历史存储或跨重启策略，仓内继续以已有事件/历史真源为准。
- 差异: Codex 将“查看子代理做了什么”和“主对话继续得到结果”放在同一任务上下文中，避免把用户赶到另一个主视图；仓内已有两个独立 DOM 面板，但都在 `#app` 中作为固定 flex sibling，空间代价更大。
- 决策: 采用“主对话不换页、按需 inspect、结果回到任务上下文”的呈现原则；不采用 Codex 未在此入口说明的托管线程/跨平台数据模型。

### 1.2 Claude Code：隔离上下文，结果摘要回父对话，运行态集中观察
- 出处: https://code.claude.com/docs/en/sub-agents
- 证据等级: V2
- 证据深度: Claude Code 官方文档正文级；文档明确说明每个 subagent 使用独立 context window，只有 final message 返回 parent；在 transcript 中，delegation 以包含 subagent 名称和短任务描述的 tool-call row 出现；文档同时把 background agents 与集中监控列为独立的观察路径。
- 呈现位置: 主 transcript 中以 delegation/tool-call row 作为轻量入口，需要时再查看 agent work；运行中的 background agents 由集中 Agent view/monitoring surface 观察，而不是把所有中间工具输出永久铺在主对话。
- 对象模型: subagent 是独立 agent instance，有自己的上下文、工具权限、模型与任务描述；父对话消费最终消息/摘要，不能把所有中间工具结果自动塞回父上下文。
- 展开/历史/状态反馈: transcript row 显示“谁被派发、做什么”；background agent 是非阻塞运行形态，集中观察入口用于查看运行状态；同一 session 的 subagent 与跨 session background agent 是不同范围，不能在仓内混成一套历史语义。
- 差异: Claude Code 明确把“过程隔离”和“结果回父对话”分开，降低主上下文噪声；仓内 R-281 已把子代理正文与工具轨迹落入既有事件/历史通道，本条不改变该数据契约。
- 决策: 采用轻量摘要行 + 按需展开完整内容 + 运行态/终态明确分段；保留仓内 transcript 与 Tauri 历史读取，不引入 Claude 的独立权限/上下文实现。

## 外部呈现逐项对照矩阵（摘要，不新增方案）

| 方案 | 呈现位置 | 对象模型 | 展开/历史 | 状态反馈 | 仓内取舍 |
|---|---|---|---|---|---|
| Codex | 主聊天关联的 activity/subagent thread；桌面、CLI、IDE 均可见 | 主任务派生的 subagent thread，最终摘要回主 chat | 可 inspect thread work；具体跨重启保留未由该入口确认 | activity 与结果 summary | 采用按需 inspect 和不换主视图；保留仓内事件历史真源 |
| Claude Code | transcript delegation row + Agent view/background monitoring | 独立 context 的 agent instance，最终消息回 parent | tool-call row 轻量展示，过程按需查看；同 session 与 background 范围有区分 | foreground/background、agent 状态和最终结果 | 采用轻量入口、默认收纳、运行/完成分区；不复制其权限与上下文模型 |

## 仓内既有设计

### 2.1 现有两个面板是固定 flex sibling，打开即占用主区宽度
- 出处: file:crates/kanzei-app/ui/style.css:1180
- 证据等级: V1
- 证据深度: 读码核实；`#bg-panel` 和 `#agent-panel` 都设置 `width: 300px`、`flex-shrink: 0`、`position: relative`，并作为 `#main` 后的同级元素存在。宽屏没有把它们从 flex 布局中移出，因此面板打开会直接减少主对话可用宽度。
- 复现步骤: 在宽屏打开活动或子代理入口，观察 `#app` 的 flex 子项；再读取 `#main` 与打开面板前后的宽度。面板显示时 300px 被固定分配给右侧 sibling。
- 差异: 用户要求“不占用一个侧边栏”，而当前实现是永久侧栏式布局；窄屏媒体查询只覆盖 `max-width:1400px`，没有解决宽屏的固定占位问题。
- 决策: 改为临时浮层定位，不参与 `#app` flex 尺寸计算；保留面板内部滚动和可调整宽度能力，但不让 `#main` 为其预留列。

### 2.2 活动面板的状态与事件消费者集中在既有活动投影
- 出处: file:crates/kanzei-app/ui/03-shell.js:303
- 证据等级: V1
- 证据深度: 读码核实；`activityPanelOpen` 由 localStorage 和 `syncActivityPanel()` 控制显示，`bgSync()` 在事件到达时只同步可见状态、筛选、分组和三段状态；`bgEntries` 保存工具调用、状态、筛选和 diff 投影。这里是本次应保留的真实事件消费者，不应在视觉重做中另造一套活动数据。
- 复现步骤: 运行产生工具事件后打开活动面板，观察 `bgSync()` 更新 `#bg-running/#bg-attention/#bg-done`；切换类型/状态/角色筛选，观察同一 `bgEntries` 投影被过滤而非重新取一套数据。
- 差异: 当前数据和操作模型可用，问题主要是容器定位与信息密度，不应以重写事件协议解决视觉问题。
- 决策: 保留 `bgEntries`、筛选、分段、diff 摘要、终端复制和历史回放消费者；只替换 shell 容器定位、开关互斥和关闭路径。

### 2.3 子代理面板已消费真实进度、正文、工具调用和终态动作
- 出处: file:crates/kanzei-app/ui/06-activity.js:1194
- 证据等级: V1
- 证据深度: 读码核实；`agentEntries` 维护 running/finished/closed 三段，`agentProgress()` 消费 usage/text/tool trace，`renderAgentTranscript()` 用既有 Markdown 渲染正文并保留工具输入，`agentEnd()` 将终态移动到完成区，历史/关闭/重开/删除动作均有真实调用方。
- 复现步骤: 触发真实 task 子代理，观察 `agentProgress()` 对 `trace.phase === "text"` 追加消息、对工具 start/end 更新 transcript；结束后点击“打开/关闭/删除”动作，确认不是静态展示壳。
- 差异: 子代理数据链路已有可用能力，但入口把所有信息压在一个 300px 固定面板中；活动和子代理两套面板也没有共享“同一临时上下文浮层”的空间模型。
- 决策: 保留 `agentEntries`、实时 text/tool trace、终态分段、单条停止和 transcript 展开；本条只改呈现容器与入口互斥，不改 R-281 的事件/历史语义。

### 2.4 两个入口声称互斥，但活动入口缺少反向关闭
- 出处: file:crates/kanzei-app/ui/06-activity.js:1275
- 证据等级: V1
- 证据深度: 读码核实；`agentTogglePanel()` 打开子代理时会隐藏 `#bg-panel`，但 `activity-toggle` 的 handler 只翻转 `activityPanelOpen` 并调用 `syncActivityPanel()`，没有关闭 `#agent-panel`。因此注释所称“互斥切换”不是双向不变量。缺陷已登记为 D-720。
- 复现步骤: 先点击 `#agent-toggle` 打开子代理面板，再点击 `#activity-toggle`；根据两处 handler，活动面板会被显示，子代理面板不会被隐藏，两个容器可同时可见。
- 差异: 两个低频面板同屏会叠加空间和信息密度问题，也使关闭/焦点归属不确定。
- 决策: 本条实现时将“最多一个临时面板可见”集中为唯一开关路径；D-720 保留为独立缺陷，修复证据回指本条但不把缺陷静默吞掉。

### 2.5 现有布局回归只覆盖窄窗口的活动抽屉快乐路径
- 出处: file:scripts/ui-narrow-layout-smoke.mjs:8
- 证据等级: V2
- 证据深度: 本地 Edge/Node 运行时实测；测试记录 T-1786922726814 的命令 `node --experimental-vm-modules scripts/ui-runtime-smoke.mjs; node scripts/ui-narrow-layout-smoke.mjs; node scripts/ui-a11y-smoke.mjs` 通过：运行时 27 个 UI 脚本/2364 次 invoke/10 个视图/0 错误，窄布局 4 个视口×3 个状态/0 重叠越界，无障碍 22 个 icon-btn。代码上窄布局脚本只把 `#bg-panel` 作为 drawer，强制隐藏 `#agent-panel`，没有验证两个面板的互斥、宽屏主区尺寸不变或子代理浮层。
- 复现步骤: 执行上述基线命令；观察命令全绿，但检查脚本的 state 只有 `drawer` 布尔值且每次 `agentPanel.classList.add("hidden")`，因此基线不能证明 R-334 的宽屏/双入口验收。
- 差异: 现有回归证明既有页面能渲染，却没有覆盖本条改变的关键不变量；不能把既有通过写成新设计已验收。
- 决策: B3 扩展同一真实 Edge 脚本：分别测试 activity/agent 浮层、主区尺寸恒定、互斥、焦点/关闭路径和 800/1024/1280/1440+ 视口；六条既有前端冒烟仍全部运行。

## 设计建议与边界

### 3.1 保留：事件、历史和真实操作消费者
- 出处: file:crates/kanzei-app/ui/06-activity.js:107
- 证据等级: V1
- 证据深度: 读码核实；活动和子代理都已有真实 Map 状态、事件 handler、Markdown/transcript 渲染、筛选、终态动作和历史相关入口。
- 决策: 保留所有这些调用方；不改 `RunEvent`/Tauri transcript 数据源，不新增仅用于展示的假数据。

### 3.2 改变：固定列改为临时上下文浮层
- 出处: file:crates/kanzei-app/ui/style.css:1180
- 证据等级: V1
- 证据深度: 仓内样式与设计正文核验；既有设计已把“对话(主) > 思考(淡) > 工具(右侧/一行痕迹)”作为层级原则，但当前 CSS 在宽屏仍以固定 flex sibling 实现。
- 决策: 两个面板继续使用现有 DOM/渲染器，改为右下方按需出现的临时上下文浮层：宽度 `min(420px, calc(100vw - 72px))`，高度受视口限制，滚动只发生在浮层内部；不再设置 `flex-shrink:0` 侧栏列，不再从 `#main` 扣除 drawer 宽度。活动与子代理共用同一视觉层级、边框、标题/关闭路径，但保持各自真实内容模型。

### 3.3 改变：开关互斥、显式关闭和可恢复焦点
- 出处: file:crates/kanzei-app/ui/03-shell.js:303; file:crates/kanzei-app/ui/06-activity.js:1275; file:scripts/ui-a11y-smoke.mjs:144
- 证据等级: V1
- 证据深度: 读码核实；当前入口分别散落在两个脚本，只有单向互斥。
- 决策: 保留 rail 入口，增加活动浮层关闭按钮；两个入口通过统一的“打开 A 就关闭 B”路径保证最多一个可见，关闭后焦点回到触发按钮；Escape 关闭当前浮层。活动内容更新不能自动打开浮层，子代理事件也不能擅自改变用户可见性。

### 3.4 待确认：历史默认范围与浮层记忆策略
- 出处: file:crates/kanzei-app/ui/06-activity.js:1458; file:docs/design/parallel_lines_ui.md:83
- 证据等级: V1
- 证据深度: 读码核实；当前活动条目保留完成区，子代理保留 finished/closed 区和 transcript；跨重启完整历史由既有后端/历史读取条目决定，前端当前面板打开状态只对活动使用 localStorage，子代理打开状态不持久化。
- 决策: 本次不缩减历史、不改变清理/关闭语义；默认浮层关闭，打开/关闭状态不影响事件积累。是否把“最近一次查看的面板/筛选/展开项”跨重启恢复，留作后续产品决策，不以视觉重做顺便改变历史范围。

## 4. 设计冻结后的实现边界

- **保留**：`bgEntries`/`agentEntries`、真实 `RunEvent`/Tauri 数据源、活动筛选与三段状态、子代理 transcript/Markdown/工具输入、单条停止、关闭/重开/删除和现有 rail 入口。
- **改变**：`#bg-panel`/`#agent-panel` 的布局定位为临时浮层；两入口双向互斥；活动面板增加显式关闭；面板标题与关闭按钮补齐 dialog/region 语义；窄屏与宽屏统一为不占主区列的呈现。
- **不改变**：子代理调度、权限、执行协议、事件格式、历史真源、活动/子代理业务状态机和后端命令。
- **待确认**：历史查看默认范围、筛选/展开状态跨重启恢复、浮层是否进一步演进为可拖动/可停靠工作区；这些不阻塞本轮低风险呈现改造。

## 5. 范围结论

- 已覆盖当前两个入口、容器、状态源、事件/历史消费者、CSS 定位和现有回归；关键锚点见 §2。
- 已对照两份官方一手正文资料：OpenAI Codex Subagents 与 Claude Code Create custom subagents；两者均支持“主任务上下文中按需检查子代理工作/结果”，但没有要求本仓复制其后端模型。
- B1 只完成审计与设计，不把现有基线通过写成新布局已验收；实现与六条前端回归留在 B2/B3。
