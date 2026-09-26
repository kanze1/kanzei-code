# 子代理呈现:一张卡贯穿一次委派

- 身份: live_design
- 状态: 设计基线,2026-09-26 起草(用户原话:「关于子代理的呈现,我喜欢 claude 的这种感觉,需要你出一版设计方案」);同日按用户第二条原话「子代理也弄成侧栏，参考claude的结构呢？弹出也自动化一点」修订 §5.6/§7,活动与子代理合成停靠的「后台任务」侧栏(修订记录见 §16)
- 上游: [chat_presentation_contract.md](chat_presentation_contract.md)(本文修订其 §3「后台」一行)、[cc_codex_alignment_20260925.md](cc_codex_alignment_20260925.md) §5.3、[subagent_management.md](subagent_management.md)、[ui_surface_stack.md](ui_surface_stack.md)
- 相关编号: R-174 R-184 R-281 R-334 R-369 D-725 D-727 D-729
- 复用的第一波原语: 动效 `.kz-glyph`/`.kz-dot` 与 `motionSync`/`motionOnce`/`motionCount`(UI-0926 #7);侧栏外观 `.k-surface.k-panel`(#9);工具行摘要 05-tool-summary.js、展开区结构化渲染 04-structured.js(#6/#10)
- 一句话: 一次委派对应主对话里的一张卡。跑的时候看得见它在干什么,跑完收成一行,点开看全过程,右侧「后台任务」侧栏看完整版;侧栏在有活时自己停靠出来,干完自己收起。

## 1. 参照物:Claude 的做法

- **Claude Code(CLI)**:头部一行 `● Explore(Find auth code)`;下面缩进显示最近 2~3 次工具调用,如 `⎿ Read(src/auth.ts)`,更早的折成 `+12 more tool uses`。工具次数、token、耗时实时跳动。结束后收成 `⎿ Done (15 tool uses · 42.1k tokens · 1m 3s)`,展开可看完整子对话。并行时写成 `Running 3 agents…`,每个子代理一行、各带状态;后台子代理保留一行紧凑的实时状态,完成时发通知。
- **Claude 桌面端**:信息量相同,但放进安静的中性卡片;状态字形会动;完整过程在侧栏里看。

取它的骨架,不照抄字符画:kanzei 主区是富文本而不是终端,所以用卡片、细竖线和 CSS 画的连接线,代替 `⎿ ├ └` 这些字符。

## 2. 目标

1. **一眼可读**:谁在做(人格)、做什么(描述)、做到哪了(最近 1~3 次工具)、花了多少(工具次数 · token · 耗时)。
2. **一张卡贯穿生命周期**:从启动、运行到完成/失败/停止,都是同一个 DOM 节点原地变化,不再在主区、活动面板、子代理面板三处各长一份。
3. **完成即收敛**:成功的子代理只占一行(契约 §3 规则 1);失败时保留一行错误(契约 §4.1)。
4. **过程可追溯**:展开能看到指令、每次工具调用(与主对话工具行同一套摘要和详情)、子代理自己说的话、最终结果;侧栏看完整版。
5. **并行成组**:同一次派发的多个子代理合成一组,每个一行,各自带状态。
6. **实时与回放同形**:重开对话后,卡片的计数、工具列表、结果与运行时一致。
7. **不显示无意义字符**:不出现调用 id、裸 JSON 入参、机器标记头。

## 3. 现状与根因

行号取自勘察基线 cd87471e(第一波合并前),实施时以函数名为准。

| # | 现象 | 根因(file:line) |
|---|---|---|
| 1 | 运行中主对话只有一个折叠行,没有进度 | `07-events.js:317-331` 里 task-progress 不进主对话;`05-chat-render.js:598-623` 只建静态折叠组 |
| 2 | 折叠组标题是 `call_…` 乱码 | `05-chat-render.js:611,575` 用调用 id 当组名 |
| 3 | 并行不成组,编排角色跨轮挤在一组 | `05-chat-render.js:550-596` 以角色名为键全局复用组 |
| 4 | 浮层面板信息堆叠、满屏 JSON | `index.html:1195-1227` 三段式;`06-activity.js:1509-1515,1580-1585` 打印 JSON;`1618-1694` 关闭/删除/清空 |
| 5 | token 偏大 | `subagent.rs:659-679` 发的是累计值,`06-activity.js:1393-1402` 又逐次累加 |
| 6 | 后台线路的子代理不动 | `01-core.js:75-83` 的后台渲染集合里没有 `kz:task-progress` |
| 7 | 停止后仍显示运行中;终态靠正则猜 | `drive.rs:1026-1032` 补占位但不发 ToolEnd;`07-events.js:482-500` 不收尾;超时/被停/超额都没有 code |
| 8 | 没有描述、看不到实际人格和模型 | `subagent.rs:403-421` schema 没有 description;`537-559` 选中的人格与模型不上报 |
| 9 | 重开对话后过程丢失 | `15-views-misc.js:488-511` 当普通工具渲染;`06-activity.js:1008-1054` 忽略已落库的 task-progress |
| 10 | 状态没有动效 | 工具与子代理的运行态是静态字形;第一波 #7 已交付 `.kz-glyph` 原语,本文直接复用 |

## 4. 数据盘点:已有与缺口

| 数据 | 现有来源 | 缺口与处理 |
|---|---|---|
| 身份键 | `kz:tool-start.id`(模型的 call id,或编排角色名) | 只作键使用,**永不显示** |
| 指令 | `input.prompt` | — |
| 短描述 | task 可选参数 `description`(本文 §12 新增);编排角色的 ToolStart 也带 `description`(角色简介冒号前那段) | 两者都没有时,取 prompt 首句(≤60 字) |
| 人格 | meta trace 的 `agent`(本文新增,实际选中值) → `input.agent` → `input.role`(编排) | — |
| 实际模型 | meta trace 的 `model`(本文新增,模型 id) | `input.model` 只有 fast/primary 档位,只作兜底 |
| 子工具调用 | trace `start`/`end`(name、input、summary、ok、outcome、code、preview、display) | trace **不带** content(契约 §2.2),子工具的 ⎿ 摘要由 preview + display 降级产出 |
| 子代理自述 | trace `text` | 已有 |
| token | trace `usage`(**累计值**) | 前端改为替换,不再累加 |
| 轮次 | TaskProgress.text「第 N/M 轮」(此时 trace=null) | 已有 |
| 耗时 | 实时由前端计时,结束时以 `kz:tool-end.durationMs` 校准;回放用 run.trace 里 `tool.completed.durationMs` | 已有 |
| 终态 | `kz:tool-end` 的 ok/outcome/code/preview/display | 超时/被停/超额的 code 与整轮停止时的 ToolEnd 由本文 §12 补齐 |
| 结果全文 | 实时:task 的 `kz:tool-end.content`(#6 通道,≤256 KiB,与历史同源);回放:消息历史里的 `tool_result.content` | 兜底依次为最后一条 text trace、preview(首行 120 字) |
| 历史过程 | run.trace 中已落库的 task-progress(入参截成 4K 字符串) | 前端未消费 → 接上 |
| 后台/续聊/隔离 | R-369 规划:`display.kind=background_task`、`kz:task-done`、`input.resume` | 本文先预留呈现位点,事件随 R-369 接线 |
| 等待批准 | 子代理的询问恒为 Deny,当前不存在这个状态 | 预留:R-369 可写子代理发出的 `kz:ask` 需要带 `taskId` |

## 5. 组件解剖

### 5.1 单卡

```
运行中
◌ explore  找出 token 校验的调用点              7 次工具 · 18.2k token · 41s   ■ ↗
│ +4 次更早的工具调用
│ ⌕ grep   verify_token
│ ▤ read   crates/kanzei-app/src/auth/session.rs
│ ▤ read   crates/kanzei-app/src/auth/middleware.rs        ← 最新一行用前景色

完成(整卡一行)
✓ explore  找出 token 校验的调用点      完成 · 15 次工具 · 42.1k token · 1m 3s     ↗

失败(一行头 + 一行错误)
✕ plan     设计 token 刷新方案          失败 · 3 次工具 · 9.8k token · 12s         ↗
│ subagent failed: provider returned 429 Too Many Requests
```

从左到右依次是:

1. **状态字形**:`<span class="kz-glyph sa-glyph" data-state="…" aria-hidden="true">字符</span>`。动画与基础配色全部来自第一波的 `.kz-glyph[data-state]`,本组**不另写 keyframes**;字符与 data-state 的映射见 §6。形状和颜色双重编码(D-105)。卡片重建时调 `motionSync(glyph)` 与既有呼吸点同相。
2. **人格签** `.sa-agent`:explore / plan / 编排角色名。加粗,中性 `var(--fg-strong)`:身份靠角色名文本本身,不用色(原先按 `agentRoleAccent` 哈希取 `line-accent-1..4`,4 种身份色撞上琥珀/绿这些状态色,已随 [ui_color_semantics.md](ui_color_semantics.md) 去掉)。还不知道人格时显示「子代理」。
3. **描述** `.sa-desc`:单行,超长省略,`title` 给出全文。
4. **修饰签** `.sa-chip`(可选):「续聊」「后台运行」,R-369 落地前不会出现。
5. **计数** `.sa-meta`:形如 `7 次工具 · 18.2k token · 41s`,值为 0 的项不显示;启动中显示「启动中」;终态前面加状态词。数字等宽;工具次数用 `motionCount` 写入(上升时 tick 一次)。
6. **动作**:■ 单条停止(位置常驻;仅运行中可用,悬停或 `:focus-within` 时显示);↗ 在侧栏查看完整过程(单卡常驻低对比,组成员悬停/焦点时出现)。

### 5.2 实时尾迹 `.sa-live`

- 只在 starting/running/waiting 三个状态显示;进入终态立即收起(守住一行原则)。
- 显示最近 3 次子工具调用(按 start 顺序)。每行是 `toolIconNode(name)` + 工具名 + `toolArgSummary(name, input).text`——与主对话工具行同一个摘要真源(05-tool-summary.js,路径相对化、长参数智能截断),绝不输出 JSON。入参在回放里只剩截断字符串、解析失败时,退回 trace 的 `summary`。
- 颜色规则:还在执行的一行用前景色;已结束的用 dim;失败的前面加 ✕,用 err 色。
- 超过 3 次时,顶部加一行「+N 次更早的工具调用」。
- 还没有任何工具调用时:显示一行 dim 的轮次文本(「第 2/12 轮」)。如果最近一次事件是子代理自述,显示自述首行(≤80 字)。
- 左侧一根 1px 细线(`--border-soft`)从字形下方垂下,代替 `⎿`。

### 5.3 页脚与错误

- **成功/无回答**:不加页脚,状态词并入 `.sa-meta`,整卡一行。
- **失败/超时/未启动/中断**:加一行 `.sa-error`。内容取 preview 首行,经 `stripToolOutcome` 剥掉 `[tool_outcome=…]` 机器头,≤160 字;超时类用 warn 色,其余用 err 色。**不**自动展开过程。

### 5.4 内联展开 `.sa-body`

点卡头展开,依次是三节:

1. **指令**:prompt 用 `renderMarkdown` 渲染,默认显示 4 行,附「展开全文」。带 schema 时再加一行 dim 的「要求结构化返回」。
2. **过程**:时间线。子代理自述(dim 的 markdown 小块)与子工具行按发生顺序交错。子工具行复用 `buildToolBlock(name, input)` / `fillToolBlock(block, { ok, outcome, code, content: preview, display })`,与主对话工具行同一套图标、⎿ 摘要(`toolResultSummary`)和可展开详情(diff/终端块);`fillToolBlock` **不传** input,展开区不出现裸 JSON。
3. **结果**:最终回答用 markdown 渲染;带 schema 的结果先 `parseJsonish`,成功则用 04-structured.js 的 `renderJsonTree`,失败回退 markdown;还在运行时显示占位「运行中…」。

- 底部动作:停止 / 复制结果 / 在侧栏查看完整过程。
- 展开区 `max-height: 420px`,内部滚动(契约 §3 规则 3);超出部分的出口就是「在侧栏查看完整过程」。

### 5.5 并行组 `.sa-group`

```
◌ 3 个子代理 · 2 运行中 · 1 完成                                          查看全部 ↗
  ├ ◌ explore  找出 token 校验点     ⌕ grep verify_token        6 次工具 · 12s
  ├ ◌ plan     设计刷新方案          ▤ read src/auth.rs          3 次工具 · 12s
  └ ✓ explore  定位相关测试          完成 · 4 次工具 · 2.1k token · 9s
```

- **分组规则**:同一批 task 的 tool-start 是连续到达的(drive.rs 的 `run_subagent_calls` 先循环发出全部 ToolStart 再并发执行;phase_pipeline.rs 的 `dispatch_roles` 也是这样)。新卡挂进 pane 的**最后一个子节点**,前提是它是一个仍 open(`data-open="1"`)的 `.sa-group`;否则新建一个组。
- **封口**:组内任一成员收到第一条 task-progress 或 tool-end 时,置 `data-open="0"`。任何其它内容追加进 pane 后,这个组自然不再是最后一个子节点,也就不会再被并入。
  - 例外:超额派发(`subagent_limit`)在后端是逐个「ToolStart、ToolEnd」紧跟在同批 ToolStart 之后发出的。没跑起来(无任何进度)的「未启动」卡收到 tool-end 时**不封口**,否则同一批从第 2 张超额卡起每张都单独成组。
- 只有 1 个成员时不显示组头,看起来就是单卡(`.solo`)。第 2 个成员加入时组头出现,成员切换为 member 变体。
- **组头**:
  - 聚合字形:只要有成员在运行就是 running;否则有失败则 failed;否则 done(同样是 `.kz-glyph`)。
  - 文本:「N 个子代理 · a 运行中 · b 完成 · c 失败」;编排批次前面加阶段名,如「勘察 · 5 个子代理 · 3/5 完成」。
  - 右侧「查看全部 ↗」,打开侧栏列表。
- **member 变体**:一行,依次是字形、人格、描述、(运行中时)最新一次工具的摘要、计数;不出现 3 行尾迹。点这一行,就在它下方内联展开(同 §5.4)。
- 竖线和每行前的短横线用 CSS 边框绘制。
- 编排角色跨轮重派时:新一轮就是新的一批、新的卡。D-725 的「上一轮结果不被覆盖」语义保留。

### 5.6 后台任务侧栏(活动与子代理合一的停靠侧栏)

2026-09-26 用户原话:「子代理也弄成侧栏，参考claude的结构呢？弹出也自动化一点」。这推翻了 R-334 当时「这两个不用占用一个侧边栏」的结论:原先的活动浮层 `#bg-panel` 与子代理浮层 `#agent-panel` 合成一个停靠侧栏 `#tasks-panel`(标题「后台任务」),两块浮层与它们的两个 rail 开关一起删除。

```
列表模式(停靠在对话列右侧)                 详情模式
┌ 后台任务  2 运行中        ⚲ ⤢ ✕ ┐     ┌ ‹  后台任务                    ✕ ┐
│ 运行中                           │     │ ● explore  找出 token 校验的调用点 │
│ ┌ 先看鉴权链路再定改法    ■  41s ┐│     │ qwen3-coder:30b · 7 次工具 · 18.2k │
│ │ 3 个子代理 · 18.2k token      ││     │ token · 41s        [停止][复制][⌖] │
│ │ 委派 1/3  ● ● ✓            ⌄  ││     │ 指令                              │
│ │ 子代理     模型   Token  用时 ││     │   …markdown…                     │
│ │ ● explore  qwen3  6.1k   41s  ││     │ 过程                              │
│ │ ● plan     qwen3  4.0k   41s  ││     │   ▤ read  src/auth.rs  ⎿ 212 lines│
│ │ ✓ explore  qwen3  8.1k    9s  ││     │   子代理:先看中间件…              │
│ └──────────────────────────────┘│     │ 结果                              │
│ $ cargo test -p kanzei-app  12s  │     └──────────────────────────────┘
│ 需要关注  1              知道了  │
│ ✕ 子代理 · 超时 …                │
│ › 已完成  4                      │
│ 运行审计摘要(一行,可展开)       │
└─────────────────────────────────┘
```

- **位置与外观**:`<aside id="tasks-panel" class="k-surface k-panel" data-dock="side|drawer">`,放在 `#main` 里。`#main` 改为两列三行网格:第 1 列是对话/视图与运行日志,第 2 列宽 `--kz-side-col`,侧栏从顶部一直到状态栏上沿(跨第 1、2 行);状态栏跨两列、全宽。停靠时对话列与运行日志一起变窄,**不压住任何内容**。外观来自 surface.css 的 `.k-panel[data-dock]`(直角、左侧 1px 分隔线、底色 `--surface-panel-docked` = 左侧栏同色,停靠态无阴影);style.css 只写网格位置、宽度和内部排版。它是常驻侧栏,不是弹层:不进 00-surface.js 的栈,不用 `<details>`、不自写 `position:fixed`。
- **宽度**:默认 `clamp(360, 26vw, 520)`;左缘分隔条(00-frame.js 的 `installSplit`,ui_surface_stack §4.6)可拖,范围 `[320, min(760, 主区宽 − 600)]`,偏好写进 `ui_layout.splits.tasks`;头部 ⤢ 在默认宽与上限之间切换。
- **抽屉**:停靠后对话列不足 600px(`主区宽 − 侧栏宽 < 600`)时改为抽屉:`data-dock="drawer"`,贴 `#main` 右缘绝对定位,宽 `min(侧栏宽, 100% − 96px)`,带阴影,下面铺一层遮罩 `#tasks-scrim.k-scrim`(点遮罩收起)。抽屉只由用户打开,自动开合只亮徽标(§7)。
- **三段**:
  - **运行中**:段头吸顶。每段里委派卡在前(最新一批在上),终端条目在后(按开始先后)。
  - **需要关注**:未确认的失败(子代理失败/超时/中断/未启动、跑满 3 秒后失败的终端命令)。段头右侧「知道了」把它们一次挪进已完成并清掉红徽标;只打开其中一条的详情,也算确认了那一条。
  - **已完成**:默认折叠,展开才建卡。
- **委派卡**(一批一张,Claude 桌面端「后台任务」卡片的骨架):
  - 标题:单个子代理取它的描述;并行一批取派发前那段助手话的首句(≤60 字,较长时下一行给 240 字的摘录),没有就「并行委派」;编排批次写「阶段 · N 个子代理」。点标题 = 定位到对话里的那张卡。右侧 ■ 停这一批(只停在跑的)与用时(结束后用 `durationMs` 校准)。
  - 统计行:「N 个子代理 · token 合计 · k 失败」。
  - 阶段行:阶段名(编排的阶段名,模型派发写「委派」)、完成数 `done/N`、每个子代理一个进度点(完成中性、在跑强调色、失败 `--err`、超时/未启动/中断 `--warn`;批次进度不用绿,ui_color_semantics §4);点阶段行折叠/展开下面的小表,运行中/需要关注默认展开,已完成默认折叠。
  - 小表:「子代理 · 模型 · Token · 用时」一行一个子代理,点行进入详情。行的可访问名只随状态变化重写,1 秒刷新只改用时文本。
- **终端条目**:终端命令(bash 等)沿用原活动面板的条目卡(`bg-entry`、详情、复制、筛选),放进同样的三段;编排派发的 task 只在委派卡里出现,不再在终端条目里重复一份。头部「筛选与清理」菜单(`data-kz-menu`)保留类型/成败筛选与「清空已完成」;原来的「角色」筛选随编排条目一起删除。
- **详情模式**:沿用改造前的详情——顶部 ‹ 返回;头部字形、人格、描述、状态词;次行模型 id · 工具次数 · token · 耗时;动作停止 / 复制结果(`toast` 反馈) / 定位到对话;正文指令/过程/结果三节与内联展开共用渲染器,不设高度上限,运行中实时追加。
- **底部**:运行审计摘要(`#agent-audit`,一行可展开;「查看失败」直接展开已完成段);fast 模型就绪提示只在未就绪时出现。
- **rail 开关**:只剩一个 `#tasks-toggle`(面板图标,`aria-controls="tasks-panel"`、`aria-expanded`),**所有视图都显示**;在别的视图点它会先回到对话再打开。徽标 `#tasks-badge`:活动线路有在跑的子代理/终端命令时显示数量(`--accent-text` 底);没有在跑但有未确认的失败时显示失败数、转红(`--err` 底,`--bg` 字);徽标出现时 #7 的呼吸点让位。命令面板有「后台任务」一项。
- **唯一写入者**:显隐、`data-dock`、`#main[data-side]`、`--kz-side-col` / `--kz-tasks-w` / `--kz-dock-right`、遮罩、rail 开关的 aria 与徽标,只由 06-agent-panel.js 的 `reconcileTasksPanel()` 按策略决策写入;视图切换、窗口尺寸变化、分隔条拖动、事件到达都只是请它重算。
- 仍然**取消**旧子代理面板的「关闭/删除」动作;「运行中/已完成」两段以「运行中/需要关注/已完成」三段的形式回来,段的归属只由状态和「是否已确认」决定。

## 6. 状态

字形一列的 data-state 取第一波 `.kz-glyph` 的词表(idle|running|waiting|pending|stopping|attention|done|failed;attention 是配色语义新增的「需要你」,琥珀慢呼吸),动画随之而来;warn 色的终态(超时/未启动/中断)在 `.sa-card[data-sa-state]` 上只覆盖**静态颜色**为 `var(--warn)`,不加动画。

| 状态 | 判定来源 | 字符 · glyph data-state | 状态词 | 动效 | 尾迹 | 可用动作 |
|---|---|---|---|---|---|---|
| starting 启动中 | 已收到 tool-start,尚无任何 task-progress | ● · running | 启动中 | 呼吸(原语) | 显示「启动中」 | 停止、侧栏 |
| running 运行中 | 收到任一 task-progress(包括 meta 与 trace=null 的轮次文本) | ● · running | 无(计数本身就是状态) | 呼吸(原语);新尾迹行淡入 | 3 行 | 停止、侧栏 |
| stopping 停止中 | 用户点了 ■,或收到 phase=cancelled 的 trace,尚未收到终态 | ● · stopping | 停止中 | 快呼吸(原语) | 冻结 | 侧栏 |
| waiting 等待批准(预留) | 带 taskId 的 kz:ask(R-369 B2 之后) | ⏸ · attention | 等待批准 | 慢呼吸(原语,琥珀:属于「需要你」,与主线等首个 token 的 waiting 区分) | 冻结不动 | 去批准、停止 |
| done 完成 | tool-end ok 且 outcome=success | ✓ · done | 完成 | `motionOnce(glyph, "kz-pop")` 一次 | 收起 | 侧栏、复制 |
| empty 无回答 | code=subagent_empty_answer 或 outcome=noop | ○ · idle | 无回答 | kz-pop 一次 | 收起 | 侧栏 |
| failed 失败 | 其余 ok=false 的情况 | ✕ · failed | 失败 | kz-pop 一次 | 收起 + 错误行 | 侧栏、复制 |
| timeout 超时 | code=subagent_timeout(旧数据兜底:preview 含 `wall-clock safety limit`/`超时`) | ⏱ · failed + warn 色 | 超时 | kz-pop 一次 | 收起 + 错误行 | 侧栏 |
| cancelled 已停止 | code=subagent_cancelled,或 trace phase=cancelled,或被 kz:stopped 收尾(旧数据兜底:`stopped by the user`/`cancelled: run stopped`) | ■ · idle | 已停止 | 无 | 收起 | 侧栏 |
| rejected 未启动 | code=subagent_limit(旧数据兜底:`too many parallel subagent tasks`) | ⊘ · failed + warn 色 | 未启动 | 无 | 错误行说明超出上限 | — |
| interrupted 中断 | 运行中遇到 terminal kz:error,或 kz:done 时仍无终态;回放中有调用没有结果 | ⚠ · failed + warn 色 | 中断 | 无 | 收起 | 侧栏 |
| background 后台运行(预留,R-369 B1) | tool-end 的 `display.kind=background_task`;之后由 `kz:task-done` 转为终态 | ◌ · pending | 后台运行 | 慢呼吸(原语) | 1 行最新工具 | 停止、侧栏 |
| resumed 续聊(修饰态,预留,R-369 B3) | `input.resume` 存在 | 字形不变,加「续聊」签 | — | — | — | 跳到原任务 |

- 判定顺序:**code 优先**,旧文案正则只给没有 code 的历史数据兜底。
- `subagentSettle` 只把 starting/running/stopping/waiting 改成 cancelled 或 interrupted,background 不受影响。
- 一次性动效只在实时状态跳变时触发;历史回放不播(`motionOnce` 的调用点不在回放路径上)。
- 整轮停止:后端对未结束的 task 补发 `kz:tool-end{code:"subagent_cancelled"}`(§12);前端 `kz:stopped` 的 settle 与这条 ToolEnd 谁先到都收成 cancelled,之后到达的事件不得把终态卡改回运行态(与 #7 的 turnPhase 粘滞逻辑一致)。

## 7. 交互

- **点卡头**:内联展开/收起(§5.4)。**点 ↗**:打开侧栏详情。**点 ■**:单条停止,调用 `stop_task{projectDir, processId, taskId}`,不会停整轮;点下后字形立即转 stopping。
- **组头「查看全部 ↗」**:打开侧栏列表,并滚到这一批的委派卡、短暂描边。**侧栏小表的行**:点击进入详情。**⌖ 定位到对话 / 点委派卡标题**:卡片 `scrollIntoView({block:"center"})`,并加 1.2s 的 accent 描边 `.sa-flash`(outline,无动画);卡片已被裁剪出视图时,`toast`「该子代理已滚出当前视图」。
- 结束不会自动展开;失败只常显错误行,不展开过程。
- **活动与子代理同在一个侧栏**(§5.6):终端调用照旧以条目卡进入(R-168/R-173 的口径不动),子代理以委派卡进入;编排批次只在委派卡里出现(D-729 的编排条目随之并入委派卡)。「去后台任务侧栏看全」「最近一次压缩纪要」等入口一律打开这个侧栏并定位,不再切换两块面板。
- **切换线路**:侧栏的列表、徽标、自动开合状态都换成新线路的;当前详情不属于新线路就回到列表。后台线路的事件不进当前线路的侧栏。

### 7.1 自动开合(纯策略:`ui/06-side-policy.js`)

策略模块零 import、零 DOM,时钟由调用方注入;06-agent-panel.js 把实时事件喂给它(`sideEvent`),再按它的决策(`sideDecide` → 显隐/停靠形态/多久后再算/徽标)统一写 DOM。

| 规则 | 口径 |
|---|---|
| 自动打开 | 活动线路上**实时**开始了一个子代理,或一条终端命令跑满 **3 秒**;历史回放、后台线路、编排 task 的终端重复条目都不触发。 |
| 不抢焦点 | 自动打开时焦点留在原处(用户可能正在打字),对话列宽度变化时保持「跟随最新」或原阅读位置不跳。 |
| 本次运行内的压制 | 「本次运行」= 用户手动发出一条消息到下一条用户消息;鞭挞自动续跑的轮次属于同一次运行。本次运行里用户关过侧栏(且当时还有活),就不再自动打开,直到下一条用户消息。 |
| 自动收起 | 由自动打开的侧栏在活动线路**全部结束 6 秒后**收起;指针悬停或焦点在侧栏内时暂停计时,离开后重新计时。用户手动打开的(rail、↗、命令面板)不自动收起。 |
| 失败保留 | 值得停留的实时失败(子代理失败/超时/中断/未启动,跑满 3 秒后失败的终端命令)出现时不自动收起,徽标转红显示失败数。「知道了」、关闭侧栏、打开该失败的详情、下一条用户消息都算确认。 |
| 按线路记状态 | 压制、自动态、失败保留都按线路分别记;手动打开(pinned)是全局的,切线路后仍保持打开。 |
| 抽屉不自动开 | 停靠后对话列会不足 600px 时只亮徽标,不自动弹出抽屉;窗口变宽后仍在自动态则直接停靠显示。 |
| 只在对话视图 | 其它视图一律不显示;切走不改状态,切回按状态重算(收起计时在别的视图也照走)。 |
| 设置 | 设置页「后台任务侧栏」:「自动弹出」「全部完成后自动收起」两个开关,默认都开,写进 `ui_layout.side_panel`(随 app.json 持久化)。 |

## 8. 键盘与无障碍

- `.sa-head` 是原生 button,Enter/Space 展开;`aria-expanded` 配合 `aria-controls` 指向 `.sa-body`。
- `aria-label` 为「{人格} {描述} — {状态词} — {计数}」,只在状态变化和结束时重写,不随每秒的计时刷新。
- 字形 `aria-hidden`,靠形状加状态词双重编码,不只靠颜色。
- ↗ 与 ■ 是带 `aria-label` 的 icon-btn;■ 在 `:hover` 或 `:focus-within` 时出现,键盘用户 Tab 进卡片就能看到。
- **播报**:在 `#view-chat` 里放一个 `.sr-only role=status aria-live=polite` 的 `#sa-announcer`。它只播报活动线路的终态(如「子代理 explore 完成」);尾迹行不进入 live region。
- **侧栏焦点**:`#tasks-panel` 是 `<aside>` 地标(`aria-labelledby` 指向标题),不是对话框,不设焦点陷阱。进入详情时焦点落在「‹ 返回」,返回列表后落在 ✕;Esc 在详情模式回到列表,在列表模式关闭侧栏(算用户关闭,§7.1),并把焦点还给打开它的元素(没有就还给 rail 开关)。Esc 监听挂在 `#tasks-panel` 自身,不挂 document/window(ui_surface_stack §5 禁令)。自动打开从不移动焦点;用户手动打开的抽屉把焦点放进侧栏头部。
- **rail 开关**:`#tasks-toggle` 带 `aria-controls="tasks-panel"` 与 `aria-expanded`;徽标数字同时写进开关的可访问名(「N 运行中」/「N 个失败待查看」),不只靠颜色。
- **`prefers-reduced-motion`**:第一波的动效纪律块已统一关掉 `.kz-glyph`/`.kz-dot` 的循环动画与一次性动效;本组新增的尾迹淡入(声明按契约 §1 写在 style.css「分区:动效」,与 rail 徽标让位规则放在 rail 点旁边)由全局 `.01ms` 规则关掉,`.sa-flash` 只是描边、无动画。字形字符本身(◌/✓/✕/⏱…)不依赖动画即可区分。

## 9. 移动端 PWA

PWA 是审批与通知的遥控器,不渲染对话,所以卡片不搬过去,只改通知行:

- 通知行改成「字形 · 身份 · 摘要 · 时间」:
  - **字形**按 `status`:running → 呼吸点,succeeded → ✓,failed → ✕,stopped → ■,`requires_action` → ⚠。
  - **身份**按 `agent_id`:primary → 「主代理」;`task:` 前缀 → 「子代理」。
  - 去掉对用户没有意义的 `[序号] agent_status_changed —`。
- R-369 B1 的后台子代理通知(`agent_id=task:<id>`,summary 形如「explore · 找出 token 校验点 — 完成(15 次工具 · 1m 3s)」)会按子代理行缩进显示。本期不改后端的通知内容;PWA 配色本版不改(ui_surface_stack §6 暂缓项)。

## 10. 视觉规格(只用 token)

**颜色**
- 卡片底:`color-mix(in srgb, var(--panel) 60%, transparent)`;边框 `var(--border-soft)`;运行中边框 `color-mix(in srgb, var(--accent) 30%, var(--border-soft))`(强调色只用于「运行中」,契约 §2.9)。选中/悬停/焦点一律中性(`--surface-hover`/`--surface-selected`/`--focus-ring`)。
- 文字:人格签 `var(--fg-strong)`(身份不用色);描述 `var(--fg)`;计数与尾迹 `var(--dim)`;最新一行尾迹 `var(--fg)`。
- 字形:基础色由 `.kz-glyph[data-state]` 给出;超时/未启动/中断覆盖为 `var(--warn)`。
- 徽标:`var(--accent-text)` 底、`var(--bg)` 字(文字放在强调色上要 ≥ 4.5,白字在 `--accent` 填充上只有 3.9);定位闪烁:`outline 2px var(--accent)`。
- 后台任务侧栏:底色 `--surface-panel-docked`(与左侧栏同色,ui_surface_stack §3)、左侧 1px `--border-soft` 分隔;失败徽标 `var(--err)` 底、`var(--bg)` 字(a11y 冒烟校验 ≥ 4.5);进度点完成 `--dim`、在跑 `--accent`、失败 `--err`、超时/未启动/中断 `--warn`;抽屉遮罩 `--surface-backdrop-soft`。
- 不新增颜色 token;D-380 的字面量颜色判据照常生效。

**间距**
- 卡片:`padding: var(--sp-3) var(--sp-4)`;`gap: var(--sp-3)`;`margin: var(--sp-2) 0`;`border-radius: var(--radius)`。
- 尾迹:缩进 18px(字形列 12px + gap 6px,与描述左缘对齐);行高 1.5。
- 组成员:`padding: var(--sp-2) var(--sp-4)`,成员之间用 1px `--border-soft` 分隔,不嵌套边框。
- 展开区:`margin-top` 与 `padding-top` 都是 `var(--sp-3)`,顶边 1px 分隔线。
- 节标题:`var(--fs-11)`,dim。

**字号**
- 卡头 `var(--fs-13)`;人格签 600 字重;计数 `var(--fs-11-5)` 并加 `font-variant-numeric: tabular-nums`;尾迹 `var(--mono)` `var(--fs-12)`;错误行 `var(--fs-12)`。
- 只用 `--fs-*` 和 `--z-*`,不写裸 px 字号或裸 z-index。

**动效**(全部来自第一波原语,本组不写 `@keyframes`)
- 运行/停止中/等待/后台:`.kz-glyph[data-state]` 的循环动画,时长只走 `--motion-loop*` token;频繁重建的节点 `motionSync`。
- 终态:`motionOnce(glyph, "kz-pop")`;计数上升:`motionCount`。
- 尾迹行淡入:复用既有 `@keyframes fadein`,时长用 `--motion-*` token。
- 定位闪烁:1.2s 的 outline,无动画。
- 展开/收起不做高度动画,避免布局抖动。
- reduced-motion 下全部关闭(§8)。

## 11. 数据契约

### 11.1 UI 部件 ← 事件字段

| UI 部件 | 事件与字段 |
|---|---|
| 卡片创建、身份键 | `kz:tool-start{sessionId,id,name:"task",input}`,键为 `${sessionId}\|${id}` |
| 描述 | `input.description`(模型 task 与编排角色同一字段)→ prompt 首句 |
| 人格签 | task-progress 中 `trace.phase="meta"` 的 `.agent` → `input.agent` → `input.role` → 「子代理」 |
| 阶段/组头前缀 | `input.phase`(scouting/review) |
| 模型(侧栏次行) | meta 的 `.model`;没有时用 meta 的 `.summary`(档位 fast/primary)或 `input.model` |
| 启动中 → 运行中 | 首条 `kz:task-progress`(通常就是 meta) |
| 尾迹行、工具次数 | `phase=start{child_id,name,input,summary}`;`phase=end{child_id,ok,outcome,code,preview,display}` |
| token | `phase=usage` 的 `.usage`(累计值,直接替换):input+output+cache_read+cache_write |
| 耗时 | 实时:收到 tool-start 时的本地时间,结束时用 `kz:tool-end.durationMs` 校准;回放:run.trace 中 `tool.completed{name:"task"}` 的 `durationMs` |
| 轮次提示 | trace=null 时的 `text` |
| 自述 | `phase=text` 的 `.text` |
| 终态分类 | `kz:tool-end{ok,outcome,code,preview,display}`,code 优先,旧文案正则兜底 |
| 错误行 | `tool-end.preview` 首行(经 `stripToolOutcome` 剥掉机器头) |
| 结果 | 实时:`kz:tool-end.content`;回放:消息历史 `tool_result.content`;兜底依次为最后一条 text、preview |
| 停止 | `invoke("stop_task",{projectDir,processId,taskId:id})` |
| 后台(预留) | `display.kind="background_task"`;`kz:task-done{id,ok,status,preview}`(R-369 B1 新增) |
| 续聊(预留) | `input.resume` |
| 等待批准(预留) | `kz:ask{taskId}`(R-369 B2 需新增该字段)加 `kz:permission-resolved` |

### 11.2 历史回放的原料

`conversation_trace_get` 返回的 run.trace payload 形状为 `{run_id, events:[…]}`,events 中包含三类条目:

- `{kind:"tool.started",id,name,summary}`
- 无 `kind` 字段的 task-progress 条目 `{id,text,trace:{…,input:"≤4096 字符的字符串",agent,model}}`
- `{kind:"tool.completed",id,name,ok,outcome,code,durationMs,preview}`

前端处理方式:

- 把 task-progress 条目按 id 写进回放缓存;卡片建立时(包括「载入更早的消息」时)消费缓存。
- `input` 是截断的字符串,能 `JSON.parse` 成功才当对象用,否则只用 `trace.summary`。
- 编排角色不在消息历史里,所以回放时没有它们的卡片(与现状一致)。
- 同一批并行的 task 在历史里也是「连续的 tool_call、随后才是各自的 tool_result」:卡片按实时同一条挂组规则成组,收到结果即封口;每条带调用的消息开头还会封住 pane 末尾仍开着的组(同一批 = 同一条助手消息里的调用)。
- **窗口边界**:历史按 120 条一窗渲染,边界可能把 task 的调用切到更早的窗口、结果留在已渲染的这一窗。这时结果配不上调用:沿完整历史往回找发出这批调用的那条消息,是 task 就在结果的位置用那次调用的入参建卡并直接收成终态(`subagentHistoryOrphan`,不再落成「tool result」通用块);之后「载入更早的消息」补出这次调用时按 part 对象认领,不建第二张、也不标「中断」。找不到调用(历史被压缩过)的仍按通用孤儿块显示。

### 11.3 前端落点

| 职责 | 位置 |
|---|---|
| 数据模型(runsBySession / liveIndex / 回放缓存)、卡片与组视图、「指令/过程/结果」共用渲染器、停止/复制/定位动作 | `ui/05-subagents.js` |
| 后台任务侧栏(三段列表、委派卡、详情、Esc、rail 开关与徽标、fast 就绪行、审计卡入口;显隐/停靠/`--kz-dock-right` 的唯一写入者 `reconcileTasksPanel`) | `ui/06-agent-panel.js` |
| 自动开合策略(纯函数,零 import:`sideEvent`/`sideDecide`、宽度与停靠判据) | `ui/06-side-policy.js` |
| 终端条目、三段落位与确认(`bgPlace`/`bgAck`)、跑满 3 秒上报 | `ui/06-activity.js` |
| 侧栏宽度分隔条、`ui_layout` 持久化(splits.tasks、side_panel 开关) | `ui/00-frame.js`、`ui/03-layout.js` |
| 事件分流:task 的 tool-start/tool-end → `subagentStart`/`subagentEnd`,task-progress → `subagentProgress`;复制上下文按卡导出 | `ui/07-events.js` |
| 后台线路推进(`kz:task-progress` 入 `BACKGROUND_RENDER_EVENTS`)、整轮停止/终态出错/轮末收尾 `subagentSettle` | `ui/01-core.js` 路由层 |
| 历史回放:消息历史建卡 `subagentHistoryCall/Result`;run.trace 回放 `subagentReplayTrace/Duration` | `ui/15-views-misc.js`、`ui/06-activity.js renderRecoveredTraces` |
| 冒烟:单卡生命周期、尾迹、token 替换、终态分类、并行组与封口(含超额不拆组)、后台推进、历史回放同形、窗口边界孤儿结果与补更早一窗的批次顺序、切语言、停止收尾、侧栏行可访问名、审计「已停止」;变异 `saUsageAccumulate`/`saSettle`/`saPrependBatch`/`saOrphanTask` | `scripts/ui-runtime-smoke.mjs`「分区:子代理」 |
| 冒烟:自动开合策略 12 个场景、集成(自动打开不抢焦点、6 秒收起、失败红徽标、切视图/切线路)、缺陷 A–F 回归、设置持久化;变异 `sideSuppressRun`/`sideFailureHold`/`sideLinger`/`sideLineScope`/`sideReplayPlace`/`sideReplayRunning`/`sideViewGate`/`sideNoFocusSteal`;窄窗口停靠/抽屉判据 | `scripts/ui-runtime-smoke.mjs`「分区:后台任务侧栏与可调框」、`scripts/ui-narrow-layout-smoke.mjs` |

实现取舍(与上文示意的差异):运行中字形用 `●`(`.kz-glyph[data-state=running]` 呼吸,比 `◌` 在各字体下都清楚);■ 的位置常驻(不跑时禁用且隐形),组内各行的计数列对得齐;组成员的 ↗ 只在悬停/键盘焦点时出现(一列箭头太吵),单卡的 ↗ 常驻低对比;rail 徽标是计数胶囊,取代 #7 在同一位置的呼吸点(`#agent-toggle[data-running]` 仍是布尔,#7 的冒烟与样式共用);卡片里的 markdown 复用 `.sv-md`,标题压到正文字号附近。

## 12. 最小后端增补(不新增事件名、不新增 IPC 命令)

1. **TaskTrace 两个新字段**(`kanzei-core/src/runner/event.rs`):`agent: Option<String>`、`model: Option<String>`,并 derive `Default`(各 phase 只填自己的字段,其余 `..Default::default()`)。
2. **meta trace**(`subagent.rs` 的 run_subagent):解析出人格与 route/model 之后、权限询问与租约之前,发一条 TaskProgress,因此先于任何子工具 start。
   - `text`:`"{agent} · {model}"`
   - trace:`phase:"meta"`,`child_id` 为父调用 id,`agent` 为实际选中的人格,`model` 为模型 id,`summary` 为档位 `fast|primary`。
3. **稳定错误码**(均为 `ToolOutput::failed`,文案全部不变):
   - 单条被停:`subagent_cancelled`。
   - 两处墙钟超时(前台/后台派发共用 `subagent_timeout_output`):`subagent_timeout`。
   - 超出单轮上限:`subagent_limit`。
   - 整轮停止时的 D-342 占位:`failed("subagent_cancelled","cancelled: run stopped by user")`,**并补发对应 ToolEnd**(经 `RunEvent::tool_end` 构造,与真实终态走同一条投影链)。
   - Failed 不属于 expected rejection,`model_content` 不带机器头,回喂模型的文本与改动前逐字节一致;工具失败遥测不经过 task 路径;运行画像只按 `is_error` 与机器头计数,不依赖 code 为空。
4. **task schema** 增加可选 `description`(「显示给用户的 3~8 词短标签」),工具描述的 Params 段同步一句;`required` 仍然只有 prompt。名册只有默认人格时 `task_spec_for` 仍与 `task_spec` 逐字节一致。
5. **事件负载**(`kanzei-app/src/run/events`):纯函数 `task_progress_payloads(id, text, trace)` 返回 (UI, 落库) 两份负载,两份都带 `agent`/`model`;落库那份 input 仍截到 4096 字符。
6. **编排**(`phase_pipeline.rs`):ToolStart 的 input 带 `description`(角色简介冒号前那段);ToolEnd 的 code:空答沿用 `subagent_empty_answer`,失败沿用子代理自己的码(如被停 `subagent_cancelled`),角色墙钟超时与屏障后报告缺失为 `subagent_timeout`,成功为空。

## 13. 与既有契约和条目的关系

- **chat_presentation_contract §3**:「后台」层加一条子代理例外——运行中主区显示最多 3 行实时尾迹,终态收成一行,完整过程进侧栏。规则 1(一行)与规则 3(展开有上限并有出口)不变。
- **R-184 折叠组退役**:按角色跨轮折叠改为按批次分组;D-725 的「同名重派不覆写」语义由「每次派发一张新卡」继续满足。
- **D-727**:复制上下文改为以卡片为单位导出(人格 · 描述 / 计数 / 结果前 400 字)。
- **R-174**:名称、类型、时长、工具次数、token、当前工具、单条停止、transcript,这些字段全部保留,只是转移到卡片和侧栏。关闭/删除按本文取消——这是有意的行为变更,实施提交说明写明,并在 tracker 登记;分段以「运行中/需要关注/已完成」的形式回到后台任务侧栏(§5.6)。
- **R-334**:当时用户说活动与子代理「这两个不用占用一个侧边栏」;2026-09-26 用户改口要 Claude 式的侧栏与自动弹出,本文 §5.6/§7.1 以新口径为准,两块浮层合成一个停靠侧栏。
- **R-369**:后台、续聊、隔离的呈现位点已经预留。R-369 B1 计划中的 `subagent_timeout`/`subagent_cancelled` 两个码由本次先落地,B1 实施时跳过这两项。`kz:task-done` 的订阅要与后端 emit 同批接入(ipc-event-smoke 要求两侧一致)。
- **R-281**:子代理 transcript 阅读器由侧栏详情承担,数据来自实时事件与 run.trace 回放,本次不新增 transcript 读取命令。

## 14. 不做

- 子代理嵌套的呈现(子代理拿不到 task)。
- 回放时补建编排角色卡。
- 子代理费用/预算展示。
- PWA 端展示卡片。
- 侧栏里汇总「其它线路」的后台任务(用户拍板不做;切线路即可看到那条线路的)。
- 手动打开状态跨重启保留(宽度与两个开关持久化,打开与否不记)。
- 抽屉态自动弹出(抽屉会盖住对话,只亮徽标)。

## 15. 验收

1. 模型并行派发 3 个 task 时,主对话出现一个组,每个子代理一行,各自计数实时跳动;结束后收成一行;全程不出现调用 id 和 JSON。
2. 单个子代理运行时显示 ≤3 行尾迹和「+N」;token 与后端累计值一致,不偏大。
3. 超时、停止、超额、空答分别显示对应的字形和状态词;整轮停止后没有停在「运行中」的卡。
4. 非活动线路的子代理照常推进,切回后是最新状态。
5. 点卡头看到指令/过程/结果;↗ 打开侧栏详情;⌖ 定位回对话;Esc 行为符合 §8。
6. 重开对话后,卡片计数、工具列表、耗时与运行时一致。
7. 亮色/暗色主题、中英文界面、reduced-motion 下都正确;ui-preview 的 chat/agents/parallel 场景零 console 错误。
8. 1600 宽(侧栏展开)开始委派时后台任务侧栏自动停靠出来、对话列变窄不被遮挡、输入框焦点不丢;全部结束 6 秒后收起;悬停时不收。
9. 本次运行里手动关掉侧栏后,鞭挞续轮再派发子代理不再弹出;发下一条消息后恢复自动弹出。
10. 子代理超时后侧栏不收、徽标转红;点「知道了」挪进已完成、徽标消失。
11. 窗口窄到对话列不足 600px 时不自动弹出,点 rail 开关以抽屉 + 遮罩打开,Esc/点遮罩收起并还焦点;切到文件/记忆等视图时侧栏不显示、rail 开关仍在。
12. ui-preview 的 tasks-drawer、tasks-failure 场景零 console 错误;ui-narrow-layout-smoke 在 7 个视口下停靠/抽屉判据成立。

## 16. 修订记录

- 2026-09-26 起草:主对话单卡、并行组、侧栏列表/详情(UI-0926 #8)。
- 2026-09-26 修订(UI2-0926 #14):`#bg-panel` 与 `#agent-panel` 两块浮层合成停靠的 `#tasks-panel`「后台任务」侧栏(§5.6),`#main` 改网格让出第 2 列、窄时改抽屉;一批委派一张 Claude 式卡片(标题、用时、统计、阶段进度点、子代理小表);自动开合改由纯策略模块 06-side-policy.js 决定(§7.1),设置页两个开关;rail 只剩 `#tasks-toggle`。顺带修掉:切视图后面板被下一条工具事件弹回、后台线路的终端命令混进当前线路、「去活动面板看全」在面板已开时反把它关掉、压缩纪要入口让两块面板同屏、回放条目挂在「运行中」、切到在跑的线路时在跑的命令被收成「中断」。§14 删去「停靠式分栏」。
