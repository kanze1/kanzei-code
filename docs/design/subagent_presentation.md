# 子代理呈现:一张卡贯穿一次委派

- 身份: live_design
- 状态: 设计基线,2026-09-26 起草(用户原话:「关于子代理的呈现,我喜欢 claude 的这种感觉,需要你出一版设计方案」)
- 上游: [chat_presentation_contract.md](chat_presentation_contract.md)(本文修订其 §3「后台」一行)、[cc_codex_alignment_20260925.md](cc_codex_alignment_20260925.md) §5.3、[subagent_management.md](subagent_management.md)、[ui_surface_stack.md](ui_surface_stack.md)
- 相关编号: R-174 R-184 R-281 R-369 D-725 D-727 D-729
- 复用的第一波原语: 动效 `.kz-glyph`/`.kz-dot` 与 `motionSync`/`motionOnce`/`motionCount`(UI-0926 #7);侧栏外观 `.k-surface.k-panel`(#9);工具行摘要 05-tool-summary.js、展开区结构化渲染 04-structured.js(#6/#10)
- 一句话: 一次委派对应主对话里的一张卡。跑的时候看得见它在干什么,跑完收成一行,点开看全过程,侧栏看完整版。

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
2. **人格签** `.sa-agent`:explore / plan / 编排角色名。加粗,颜色由 `agentRoleAccent` 取 `line-accent-1..4`。还不知道人格时显示「子代理」。
3. **描述** `.sa-desc`:单行,超长省略,`title` 给出全文。
4. **修饰签** `.sa-chip`(可选):「续聊」「后台运行」,R-369 落地前不会出现。
5. **计数** `.sa-meta`:形如 `7 次工具 · 18.2k token · 41s`,值为 0 的项不显示;启动中显示「启动中」;终态前面加状态词。数字等宽;工具次数用 `motionCount` 写入(上升时 tick 一次)。
6. **动作**:■ 单条停止(仅运行中出现,悬停或 `:focus-within` 时显示);↗ 在侧栏查看完整过程(常驻,低对比)。

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
- 只有 1 个成员时不显示组头,看起来就是单卡(`.solo`)。第 2 个成员加入时组头出现,成员切换为 member 变体。
- **组头**:
  - 聚合字形:只要有成员在运行就是 running;否则有失败则 failed;否则 done(同样是 `.kz-glyph`)。
  - 文本:「N 个子代理 · a 运行中 · b 完成 · c 失败」;编排批次前面加阶段名,如「勘察 · 5 个子代理 · 3/5 完成」。
  - 右侧「查看全部 ↗」,打开侧栏列表。
- **member 变体**:一行,依次是字形、人格、描述、(运行中时)最新一次工具的摘要、计数;不出现 3 行尾迹。点这一行,就在它下方内联展开(同 §5.4)。
- 竖线和每行前的短横线用 CSS 边框绘制。
- 编排角色跨轮重派时:新一轮就是新的一批、新的卡。D-725 的「上一轮结果不被覆盖」语义保留。

### 5.6 侧栏(由原子代理面板改造)

```
列表模式                                   详情模式
┌ 子代理 · 2 运行中                ✕ ┐     ┌ ‹  子代理                        ✕ ┐
│ 本轮 · 3 个子代理                   │     │ ◌ explore  找出 token 校验的调用点 │
│  ◌ explore 找出 token 校验点  6 · 12s ⌖│  │ qwen3-coder:30b · 7 次工具 · 18.2k │
│  ◌ plan    设计刷新方案      3 · 12s ⌖│  │ token · 41s        [停止][复制][⌖] │
│  ✓ explore 定位相关测试   完成 · 9s ⌖│  │ 指令                              │
│ 上一批 · 勘察 · 5 个子代理          │     │   …markdown…                     │
│  ✓ architecture_scout …           │     │ 过程                              │
│ ─────────────────────────────── │     │   ▤ read  src/auth.rs  ⎿ 212 lines│
│ 运行审计摘要(一行,可展开)         │     │   子代理:先看中间件…              │
└───────────────────────────────┘     │ 结果                              │
                                          └──────────────────────────────┘
```

- **外观与位置**:`#agent-panel` 保持 `class="k-surface k-panel"`(第一波 #9),底色/边框/圆角/阴影全部来自 surface.css;style.css 只写位置与尺寸——全高(从顶部 `--sp-4` 到状态栏上方),宽度 `min(480px, 100vw − 32px)`,`position:absolute`。它是常驻侧面板,不是弹层:不用 `<details>`、不自写 `position:fixed`、不直接切弹层的 hidden(ui_surface_stack §5)。继续与活动面板互斥。
- **列表模式**:
  - 列出当前线路的全部子代理,按批次分组,最新的批次在上面。
  - 每行是 member 变体:点行进入详情;行尾 ⌖ 表示「定位到对话」。
  - 底部是运行审计摘要,默认一行、可展开(沿用 `#agent-audit`)。
  - fast 模型就绪提示行只在未就绪时出现。
- **详情模式**:
  - 顶部 ‹ 返回。
  - 头部:字形、人格、描述、状态词。次行:模型 id · 工具次数 · token · 耗时。
  - 动作:停止 / 复制结果 / 定位到对话。「复制结果」的反馈用 00-surface.js 的 `toast`。
  - 正文是指令/过程/结果三节,与内联展开用同一个渲染器,不设高度上限(侧栏自身滚动);运行中实时追加。
- **rail 开关 `#agent-toggle`**:右上角加徽标 `.rail-badge`,显示当前线路运行中的子代理数;数量大于 0 时徽标旁放一个 `.kz-dot[data-state=running]`(第一波原语,不另写呼吸动画)。
- **取消**旧面板的「运行中/已完成/已关闭」三段,以及「关闭/删除/清空」三个动作。它们只改本地视图、不碰后端,用户还得理解「关闭≠停止≠删除」;列表按线路自动生成就够了。

## 6. 状态

字形一列的 data-state 取第一波 `.kz-glyph` 的词表(idle|running|waiting|pending|stopping|done|failed),动画随之而来;warn 色的终态(超时/未启动/中断)在 `.sa-card[data-sa-state]` 上只覆盖**静态颜色**为 `var(--warn)`,不加动画。

| 状态 | 判定来源 | 字符 · glyph data-state | 状态词 | 动效 | 尾迹 | 可用动作 |
|---|---|---|---|---|---|---|
| starting 启动中 | 已收到 tool-start,尚无任何 task-progress | ◌ · running | 启动中 | 呼吸(原语) | 显示「启动中」 | 停止、侧栏 |
| running 运行中 | 收到任一 task-progress(包括 meta 与 trace=null 的轮次文本) | ◌ · running | 无(计数本身就是状态) | 呼吸(原语);新尾迹行淡入 | 3 行 | 停止、侧栏 |
| stopping 停止中 | 用户点了 ■,尚未收到终态 | ◌ · stopping | 停止中 | 快呼吸(原语) | 冻结 | 侧栏 |
| waiting 等待批准(预留) | 带 taskId 的 kz:ask(R-369 B2 之后) | ⏸ · waiting | 等待批准 | 呼吸(原语) | 冻结不动 | 去批准、停止 |
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
- **组头「查看全部 ↗」**:打开侧栏列表。**侧栏行**:点击进入详情。**⌖ 定位到对话**:卡片 `scrollIntoView({block:"center"})`,并加 1.2s 的 accent 描边 `.sa-flash`(outline,无动画);卡片已被裁剪出视图时,`toast`「该子代理已滚出当前视图」。
- 结束不会自动展开;失败只常显错误行,不展开过程。
- **与活动面板的关系**:两者互斥。活动面板照旧只收终端调用、失败调用与编排条目(R-168/R-173/D-729 的口径不动)。子代理的主入口改为主对话里的卡片与侧栏。
- **切换线路**:侧栏列表换成新线路的数据;如果当前详情不属于新线路,回到列表;徽标重新计算。

## 8. 键盘与无障碍

- `.sa-head` 是原生 button,Enter/Space 展开;`aria-expanded` 配合 `aria-controls` 指向 `.sa-body`。
- `aria-label` 为「{人格} {描述} — {状态词} — {计数}」,只在状态变化和结束时重写,不随每秒的计时刷新。
- 字形 `aria-hidden`,靠形状加状态词双重编码,不只靠颜色。
- ↗ 与 ■ 是带 `aria-label` 的 icon-btn;■ 在 `:hover` 或 `:focus-within` 时出现,键盘用户 Tab 进卡片就能看到。
- **播报**:在 `#view-chat` 里放一个 `.sr-only role=status aria-live=polite` 的 `#sa-announcer`。它只播报活动线路的终态(如「子代理 explore 完成」);尾迹行不进入 live region。
- **侧栏焦点**:进入详情时焦点落在「‹ 返回」;Esc 在详情模式回到列表,在列表模式关闭侧栏,并把焦点还给打开它的元素。Esc 监听挂在 `#agent-panel` 自身,不挂 document/window(ui_surface_stack §5 禁令)。
- **`prefers-reduced-motion`**:第一波的动效纪律块已统一关掉 `.kz-glyph`/`.kz-dot` 的循环动画与一次性动效;本组新增的尾迹淡入与 `.sa-flash` 同样在该媒体查询下关闭。字形字符本身(◌/✓/✕/⏱…)不依赖动画即可区分。

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
- 文字:人格签 `var(--line-color)`(未知时 `var(--fg-strong)`);描述 `var(--fg)`;计数与尾迹 `var(--dim)`;最新一行尾迹 `var(--fg)`。
- 字形:基础色由 `.kz-glyph[data-state]` 给出;超时/未启动/中断覆盖为 `var(--warn)`。
- 徽标:`var(--accent)` 底、`var(--on-accent)` 字;定位闪烁:`outline 2px var(--accent)`。
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
- **R-174**:名称、类型、时长、工具次数、token、当前工具、单条停止、transcript,这些字段全部保留,只是转移到卡片和侧栏。三段式和关闭/删除/清空按本文取消——这是有意的行为变更,实施提交说明写明,并在 tracker 登记。
- **R-369**:后台、续聊、隔离的呈现位点已经预留。R-369 B1 计划中的 `subagent_timeout`/`subagent_cancelled` 两个码由本次先落地,B1 实施时跳过这两项。`kz:task-done` 的订阅要与后端 emit 同批接入(ipc-event-smoke 要求两侧一致)。
- **R-281**:子代理 transcript 阅读器由侧栏详情承担,数据来自实时事件与 run.trace 回放,本次不新增 transcript 读取命令。

## 14. 不做

- 子代理嵌套的呈现(子代理拿不到 task)。
- 停靠式分栏(先用全高抽屉,与活动面板同族)。
- 回放时补建编排角色卡。
- 子代理费用/预算展示。
- PWA 端展示卡片。

## 15. 验收

1. 模型并行派发 3 个 task 时,主对话出现一个组,每个子代理一行,各自计数实时跳动;结束后收成一行;全程不出现调用 id 和 JSON。
2. 单个子代理运行时显示 ≤3 行尾迹和「+N」;token 与后端累计值一致,不偏大。
3. 超时、停止、超额、空答分别显示对应的字形和状态词;整轮停止后没有停在「运行中」的卡。
4. 非活动线路的子代理照常推进,切回后是最新状态。
5. 点卡头看到指令/过程/结果;↗ 打开侧栏详情;⌖ 定位回对话;Esc 行为符合 §8。
6. 重开对话后,卡片计数、工具列表、耗时与运行时一致。
7. 亮色/暗色主题、中英文界面、reduced-motion 下都正确;ui-preview 的 chat/agents/parallel 场景零 console 错误。
