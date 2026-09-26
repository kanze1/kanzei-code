# 界面配色:表面层级与颜色语义

- 身份: live_design
- 状态: 设计基线(2026-09-26 起草,随 release/2026-09-26-ui2 分支实施;本文描述的是已落地的实现)
- 日期: 2026-09-26
- 上游文档: [ui_surface_stack.md](ui_surface_stack.md)(组件层 token 与弹层外观)、[chat_presentation_contract.md](chat_presentation_contract.md)
- 关联需求: 无(用户 2026-09-26 第二轮 UI 问题清单第 2 条「深色发灰」、第 3 条「指示颜色不统一」;tracker 条目待登记)
- 关联缺陷: 无
- 一句话: 深色表面按 Codex 桌面端实测重排(主区最深、侧栏亮一档、输入区浮起、标题纯白),并定一张语义色表——一种含义只用一种颜色:橙 = 进行中,琥珀 = 需要注意,绿 = 成功收尾,红 = 失败与 P0,灰 = 其余一切,蓝只给代码着色。由 ui-a11y-smoke ⑥ 机械守住。

## 1. 症状与根因

### 1.1 深色为什么「发灰」

同一场景取像素,与 Codex 深色实测对照:

| 部位 | Codex | 旧值 | 问题 |
|---|---|---|---|
| 主区 | #181818,全屏最深 | #1e1e1e | 比侧栏还亮,层次倒了 |
| 侧栏 / 活动栏 | #1f1f1f | #181818 | 比主区暗 |
| 输入区 | #353535,明显浮起 | #262626 | 只比主区高 8 级,看上去是平的 |
| 大标题 | #ffffff | #ececec | 不是白 |
| 正文 | 约 #dededd | #d6d6d6 | 偏暗 |

整屏最暗的一块在左边、中间反而是一块中灰,又没有纯白标题提神。文字对比度本身不低(旧正文对主区 11.5:1),灰感来自明暗层次倒置和没有白点。另外状态色偏粉(绿 #7ec79a、红 #f28b82),强调色 #d97a48 / #ee9d6b 偏棕。

### 1.2 颜色为什么不统一

同一种含义用了好几种颜色,同一种颜色又表示好几种含义:

- 琥珀同时是「阻塞 / 注意」、P1 优先级和「运行中」(鞭挞推进扫光),同一个值还有 --warn/--alert/--log-gold/--arch-unindexed 四个名字。
- 绿同时是「空闲」(状态点)和「完成」,「可执行」「被取得」「已归档」和每一次工具调用成功也是绿。
- 蓝没有固定含义,一处扛了约十种:P2、研究模式、项目级来源、排队投递、上下文进度条、记忆 SOP、研究 V1、路线图运行中节点、测试引用签……Codex 本身没有蓝。
- 优先级在一行里画三遍(左竖条 + 编号染色 + 胶囊),批次格又随优先级换色;在做的一行左边是橙色竖条和琥珀优先级竖条两条并排。
- 线路 / 角色身份色(哈希到蓝、琥珀、绿、紫)撞上状态色。
- 「等你批准 / 回答」一半用橙、一半用琥珀;子代理「等待批准」与主线「等首个 token」共用 data-state=waiting。
- ⚡ ✅ 📎 是彩色 emoji,CSS 的 color 管不到。
- 门禁只查「颜色是否 token 化、对比度、选中与焦点中性」,不查「含义和颜色一一对应」,语义漂移没人拦。

## 2. 表面层级(暗色,对齐 Codex)

表面只靠明度分层,全部 R=G=B 纯中性。

| 用途 | token | 取值 |
|---|---|---|
| 主区、状态栏、对话区 | --bg / --statusbar / --chat-bg | #181818,全屏最深 |
| 侧栏、活动栏 | --sidebar-bg / --activitybar | #1f1f1f |
| 输入框底 | --input | #212121 |
| 卡片 | --panel | #232323 |
| 次级卡片、胶囊 | --panel2 | #2a2a2a |
| 菜单、弹窗 | --surface-overlay | #2c2c2c |
| 输入区(composer) | --surface-raised | #303030,明显浮起 |
| 选中项 | --surface-selected | #ffffff14,叠在侧栏上正好 #313131(Codex 实测 #313231) |
| 代码块 | --code-bg | #202020,比主区亮,能看出边 |
| 大标题 | --fg-strong | #ffffff |
| 正文 | --fg | #e5e5e5 |
| 次要文字 | --dim | #9e9e9e(Codex 标签 #727272 在弹窗上达不到 4.5:1;取「悬停中的焦点卡」上仍能过线的最暗值,见 §6) |
| 强调色填充 | --accent | #d25e28(Codex 发送键同色) |
| 强调色文字 | --accent-text | #ff8c52 |
| 状态色 | --ok / --err / --warn | #5fc98c / #ff7b72 / #e8b84a |

- 顺序约束(⑥g):L(--bg) < L(--sidebar-bg) < L(--surface-raised),且 L(--code-bg) > L(--bg);另有最小明度级差(对比度同一公式):侧栏、代码块比主区 ≥ 1.05,输入区比主区 ≥ 1.3。只比大小拦不住退回旧值(旧主区 #1e1e1e 仍比 #1f1f1f 暗一级,旧输入区 #262626 对主区只有 1.17)。
- 主区更深之后同样的阴影更难看见,--elev-2/--elev-3/--elev-composer 各加深一档。
- 亮色基本不动(主区纯白、侧栏 #f7f7f7 浅灰,本来就是 Codex 亮色的层次),只把品牌橙统一成与暗色同一个填充值 #d25e28。文字值从 #a04b1a 微调到 #9a4817、--warn 从 #8a5b00 微调到 #845700(连同由它派生的 --badge-warn-soft / --alert-soft),让「在做」「阻塞」胶囊在悬停中的焦点卡上也过 4.5(§6)。
- 暗色 --dim(#9a9a9a → #9e9e9e)与 --accent-text(#ff8549 → #ff8c52)的微调同理(复核 2026-09-26);这四处都只朝「对比度更高」的方向动(暗色字更亮、亮色字更深),不透明表面上的数值只升不降。
- :root 的 token 名集合与改前逐一比对,一个不少、一个不多(只改值);别名 token 只在 :root 定义一次(见 §5 ⑥e)。

## 3. 语义色表:一种含义一种颜色

颜色只做冗余强化,文字和字形才是主信号(D-105 原则不变)。

| 颜色 | token | 含义 | 覆盖范围 |
|---|---|---|---|
| 橙 | --accent / --accent-text | 正在进行 | running、starting、waiting(主线等首个 token)、pending(待续跑)、doing / fixing、在跑的线、鞭挞推进扫光、测试运行中、路线图运行中节点;另外只用于品牌标记、发送键、正文里真正的链接,以及一次性的「看这里」(sa-flash、ref-highlight、filter-exempt、搜索命中) |
| 琥珀 | --warn(--alert 是别名) | 需要注意 | 阻塞(非零时)、卡住、超时、未启动、中断、暂停、配置未保存、额度、上下文告警、风险开关(总是允许 / 自动放行)、待澄清;**需要你**:权限卡与提问胶囊(--surface-attention)、子代理等待批准(data-state=attention)、待决策、研究等你选方向 |
| 绿 | --ok | 一件工作成功收尾 | 完成、测试通过、门禁通过、合并、子代理完成、研究 V2 / V3、kz-dot done |
| 红 | --err | 失败、错误,以及 P0 | failed、error、非法状态、门禁未过、加载失败、P0 胶囊、危险按钮 |
| 灰 | --dim / --fg / --fg-strong | 其余一律中性 | todo、open、draft、空闲、停止中、已停止、可执行、被取得、被读取、记忆生效、排队投递、归档、配置来源标签、线路和角色身份、每一次工具调用成功(默认不着色,失败才红)、批次格与批次进度条(部分完成也不绿)、记忆 SOP 分类、按停止收尾的度量轮次 |
| 蓝 | --info、--syntax-*、--diff-* | 语法 / 数据 | 只用于 .sv-json 键和代码着色,不表达任何状态。例外:记忆图谱的类别色里有天蓝(fact)与靛(habit),见下一行 |
| 架构图 / markdown 图 | --diagram-*(别名到 --panel / --bg / --panel2 / --border-* / --fg-strong / --dim / --accent) | 结构(不是状态) | 04-diagram.js 注入 mermaid:分组是凹下去的底、节点是凸起的中性卡片,边与次要文字 --dim;五个语义类只有 focus(本图主角)用强调色——属于「一次性的看这里」,ext 虚线描边 + 次要色、store 分组底、muted 淡化、entry 实描边,都不表达运行状态。图源码里不准写颜色(lint D4)。门禁 ui-diagram-smoke:标签对节点底 ≥ 4.5、边对画布 ≥ 3、token 解析结果必须是 hex(mermaid 只认 hex)。见 [architecture_diagrams.md](architecture_diagrams.md) §4 |
| 图谱类别色 | --graph-fact / --graph-sop / --graph-habit / --graph-preference | 记忆分类(不是状态) | 只用于记忆图谱画布与图例(.kz-graph-*、#memory-graph-*),区分 fact / sop / habit / preference 四类记忆,不表达任何状态;取非状态色相(天蓝、紫、靛、梅)靠明度拉开。门禁见 ui-a11y-smoke 分区:记忆图谱 ②(类别色只准出现在图谱选择器、必须是自己的 hex);取值与色觉异常分辨见 [memory_knowledge_graph.md](memory_knowledge_graph.md) §10 |

## 4. 具体规则

- **优先级**只编码一次,在行内胶囊上:P0 = --badge-err 底 + --err 字;P1 = --badge-soft 底 + --fg-strong 字(实底亮字);P2 = 无底 + --fg 字(无底亮字);P3 与未定 = 透明底 + 1px --border 描边 + --dim 字(描边灰字)。三档靠「底」与「字」分层。没有竖条,编号不染色。P1 底用半透明 --badge-soft:焦点卡本身是 --panel2 底,不透明 panel2 会让胶囊消失。条目上的 pri-P* 类只作钩子保留。
- **半透明底上的字按合成后的底色算**:胶囊底 ∘ 卡底 ∘ 悬停叠色三层合成(③b)。--dim 叠 --badge-soft 放在焦点卡上只有 3.98、悬停 3.42,所以带半透明底的中性胶囊(待办类 todo / open / draft / active、P1、项目卡「空闲」)字色一律 --fg 以上;项目卡「失败」胶囊换成不透明 --badge-err 底(与 P0 同一对),因为项目卡是暗色最亮的 --surface-raised,半透明红底上 --err 只有 3.85。
- **批次格**是进度不是状态:已完成格一律 var(--dim);唯一的彩色是「线在跑时的当前格」(强调色描边 + 扫光,动效分区)。tracker 字段视图里「批次 n/m」的进度条(.tf-progress-fill)同理,一律 --dim。
- **计数**:可执行白字(默认 --fg-strong);阻塞非零琥珀、非法非零红;值为 0 时 backlogStat 挂 is-zero,一律灰。
- **列表行与卡片不画彩色竖条或描边**(需求行、焦点卡、工作单元卡、记忆行、记忆候选、度量轮次行、确认的发现、外部阻塞、归档列表):border-left、border-inline-start、整圈 border-color、横向偏移的 inset 阴影、::before / ::after 底色条都算。焦点卡左边框只用线型(实线 = 运行证据 / 取得线,虚线 = 推断)表达依据,颜色中性。在做的一行由 doing 胶囊(强调色)+ 线真在跑时的呼吸点表达,不再染底。度量页按停止收尾的轮次由轮次头的结局文字(halted)表达,不画琥珀竖条。
- **全局不画彩色左竖条**,显式例外两处(⑥b 的 STRIPE_EXCEPTIONS):活动面板子行 .bg-child.warn / .err / .running(左边框就是子行的状态位,暂保留,是否改中性待用户拍板),自检失败项 .sv-check-fail(引文式缩进块)。新增例外必须登记并写明理由。
- **被读取 / 记忆生效 / 排队投递**:召回明细里「被读取」.memory-recall-hit.read 用 --fg 亮字区分,不用绿;.memory-status-badge.active、.queue-delivery 灰。
- **引用**:编号 / 路径引用(.ref-link、.sv-ref、.sv-path、a.md-path)中性 + 虚下划线,悬停才变橙;正文链接(.msg.md a、.sv-md a、.link-btn、.research-open)保持 --accent-text。
- **输入区芯片**:自主推进模式是强调色文字、不加底(Codex「完全访问」同款,引擎自己在推进 = 进行中家族);研究与结伴中性;模型来源标签全中性,「临时」用 --fg-strong 亮字区分;⚡ 快速档 --dim。鞭挞开关圆点:开着待命中性(currentColor),推进中 / 等待下一轮强调色;阶段字「等待下一轮」--accent-text、「已暂停」--warn;不用绿(开关打开不是成功收尾。UI2-0926 #11 复核,门禁 ⑥w)。
- **发送键**:#send 可用时 --accent 填充 + --on-accent 白色箭头(白在 #d25e28 上 3.9:1,只够图标,所以强调色填充上不放正文字);禁用态回落 button.primary 的中性样式。button.primary 其余用法仍是单色(⑤)。
- **rail 运行数徽标**:--accent-text 底 + --bg 字(7.72:1)。
- **kz 原语**:kz-dot idle / stopping 灰,done 绿,running / waiting / pending 强调色(pending 慢呼吸),attention 琥珀慢呼吸,failed 红;kz-glyph 同表。attention 是本轮新增的状态,专给「等你批准 / 回答」,与主线「等首个 token」的 waiting 分开;减少动效列表同步收录。
- **身份不用色**:线路身份就是字母代号(M / A / B…),代号框中性,只有真在跑的线把代号框点亮为强调色;子代理人格签 --fg-strong 加粗;活动面板编排轨迹去掉哈希取色的色点,以角色名为身份。--line-1..4 已弃用,暂留为 var(--fg-strong) 中性别名,下一版删。
- **工具调用成功**的 ✓ / ● 一律 --dim,失败才 --err;活动面板子行 .bg-child.ok 边框中性。
- **研究可信度**:V0 灰、V1 中性亮字、V2 绿、V3 绿 + 描边。金色是「注意」,不能表示「最可信」。
- **diff 增删**与 --ok / --err 同源(--diff-add / --diff-del 是别名)。
- **彩色 emoji** 绕过调色板:⚡ 一律加 U+FE0E 变成文字字形(JS 里写 `"⚡\uFE0E"`,HTML 里写 `⚡&#xFE0E;`),从此继承 color;通知里的 ✅ 换成 `✓ `(后接空格,与全库 `✓ ` / `⚠ ` 前缀一致),且只用于真正的成功(目标达成、需求与缺陷清空);「档位不匹配、鞭挞已关闭」不是成功,不带前缀;附件行去掉 📎。
- **背景神经流**(22-neural-flow.js 读 --memory-flow)改用强调色别名 --memory-flow: var(--accent)、--memory-flow-hot: var(--accent-text),不再是两主题同值的第二种橙 #c98a66。

## 5. 门禁

全部在 scripts/ui-a11y-smoke.mjs 与 scripts/ui-runtime-smoke.mjs,报错写全判据与改法。

**③ 对比度(扩充)**:底色加入 --surface-raised(暗色下最亮的一层,dim 在它上面最紧);加非文本对:--accent 在 --bg、--sidebar-bg 上 ≥ 3,--on-accent 在 --accent 上 ≥ 3;加 rail 徽标文字 --bg 在 --accent-text 上 ≥ 4.5。

**③b 叠色对比度**(chipContrastViolations):③ 只算 token 对 token 的不透明底,半透明胶囊底与悬停叠色都漏算(复核实测:P2 胶囊 --dim 叠 --badge-soft 放在焦点卡上 3.98、悬停 3.42,③ 全绿)。③b 按宿主逐层合成「胶囊底 ∘ 卡底 ∘ 悬停」后再算,两套主题都算,≥ 4.5:

| 类族(从样式表按选择器自动收集) | 宿主底色 |
|---|---|
| .st-*、.pri-badge.P0-P3 / .unset、.blocked-badge、.clarify-badge | 焦点卡 --panel2、侧栏行 --sidebar-bg、文档页行 --bg,以及三者各叠一层 --surface-hover |
| .workspace-status.* | 项目卡 --surface-raised(悬停只加阴影,不叠底色) |
| .focus-*(焦点卡里的编号、chip、阻塞原因、标题等纯文字) | --panel2、--panel2 叠 --surface-hover |

- 同一选择器的多条规则按源码顺序合并字色与底色,所以把字色单独写在另一条规则里也逃不掉;底色不是 token / transparent 的直接报「无法计算」。
- 自测 4 个反例:P2 回到 --dim 叠 --badge-soft、.st-todo 单独把字色改回 --dim、项目卡空闲胶囊回到 --muted、暗色 --dim 回到 #9a9a9a(悬停焦点卡上 4.37);类族在样式表里一条都匹配不到也红。
- 基线证据:同一判据跑本轮之前的样式表 23 条(在做 / 待澄清 / 阻塞胶囊在悬停焦点卡上 4.39-4.45,待办胶囊 3.57-4.44),跑上一版(0270de05)38 条,跑现行 0 条。

**⑥ 颜色语义**(colorSemanticsViolations):

| 编号 | 判据 | 典型违例 |
|---|---|---|
| ⑥a | 任何选择器不得把 .pri-P* 与 .complexity-cell / ::before / .id 组合 | `.doc-item.pri-P1 .complexity-cell.filled { background: var(--warn) }` |
| ⑥b | 列表行与卡片本身(.doc-item / .focus-card / .memory-row / .memory-candidate / .work-unit-card / .metrics-round,按选择器主体即最后一个复合选择器判定)的 border / border-left / border-inline-start / border-color / box-shadow,以及它们 ::before / ::after 的底色,不得引用状态色(ok / warn / alert / info / err / danger / accent 及其 -soft / -text 变体与别名);全局任何元素不得用状态色画左侧竖条(border-left、border-inline-start、横向偏移的 inset 阴影),例外只有 STRIPE_EXCEPTIONS 里的 .bg-child.warn / .err / .running 与 .sv-check-fail | `.doc-item.agent-active { box-shadow: inset 3px 0 0 var(--accent) }`、`.focus-card.blocked { border-color: var(--warn) }`、`.metrics-round.halted { border-left: 2px solid var(--warn) }` |
| ⑥c | var(--info) / var(--badge-info) 只准出现在 .sv-json / 语法类选择器里 | `.queue-delivery { color: var(--info) }` |
| ⑥d | 中性语义选择器(todo / open / draft / active、可执行、被取得、被读取、依赖可做层、idle / stopping 点与字形、批次格与 .tf-progress-fill、P1-P3 胶囊、项目卡空闲、线路代号框、人格签、来源标签、工具单步成功、归档、.memory-status-badge.active、.queue-delivery、研究 V1)不得引用状态色,按前缀匹配;编号 / 路径引用(.ref-link、.sv-chip.sv-ref、.sv-chip.sv-path、a.md-path)同样中性,但 :hover / :focus* 分支放行(悬停才变橙是语义表的一部分) | `.kz-dot[data-state="idle"] { background: var(--ok) }`、`.ref-link { color: var(--accent-text) }`、`.tf-progress-fill { background: var(--ok) }` |
| ⑥p | 必须着色的几处:is-zero 灰、阻塞琥珀、attention 点与字形琥珀、kz-dot done 绿、P0 胶囊红 | 删掉 is-zero 规则 |
| ⑥e | 别名 token 在 :root 必须精确指向语义表(--alert / --log-gold / --arch-unindexed / --surface-attention → --warn,--badge-alert → --badge-warn,--dot-idle → --dim,--dot-run / --memory-flow → --accent,--memory-flow-hot / --statusbar-run-fg → --accent-text,--statusbar-fg → --dim,--diff-add → --ok,--diff-del → --err),且不得在亮色块里重给值;--line-1..4 只准是中性别名 | 亮色块写 `--dot-idle: #1d7a3c` |
| ⑥g | 暗色表面 R=G=B;L(--bg) < L(--sidebar-bg) < L(--surface-raised);L(--code-bg) > L(--bg);最小级差 (L+.05)/(L(--bg)+.05):侧栏、代码块 ≥ 1.05,输入区 ≥ 1.3 | 侧栏改回 #181818、代码块改回 #171717、主区改回 #1e1e1e、输入区改回 #262626 |
| ⑥w | 输入区鞭挞组(#auto-continue-wrap 圆点、.auto-progress、.auto-phase)不得引用 --ok;.autorun-bar[data-phase=running / pending] 的圆点背景必须是 var(--accent);pending 阶段字颜色必须是强调色家族。静息态(选择器不带 [data-phase])归 ⑥d 中性前缀 | `#auto-continue-wrap:has(> input:checked)::before { background: var(--ok) }`、`.autorun-bar:is([data-phase="pending"], [data-phase="paused"]) .auto-phase { color: var(--warn) }` |
| ⑥t | :root 与亮色块剥掉注释和全部声明后不得有残留 | 注释里写了星号紧跟斜杠(如 `--dot-*` 后接 `/`),注释提前结束,剩下的文字成了非法声明、吞掉下一条声明 |

- ⑥t 的来历:实施时亮色块注释里的别名列表写成了星号加斜杠的简写,浏览器把紧随其后的 `--bg: #ffffff` 吞进一条非法声明,文档页、线路页这类直接以 body 的 --bg 为底的页面在亮色下整块成了 #181818;对话页走 --chat-bg,所以只看对话页发现不了。①-⑤ 与 ⑥e 都按「声明」解析 token,照样读到 #ffffff,全绿。修复后另用浏览器实测核对:两套主题各 108 个颜色 token 的计算值与静态解析逐一相等。
- 自测:照 selfTestSurfaceRules 的做法,每条判据喂反例(共 29 个;复核补了 ⑥b 六种绕过写法、⑥d 引用与进度条三个、⑥g 退回旧值两个;UI2-0926 #11 复核补 ⑥w 三个、⑥d 待命圆点一个),任何一条恒绿先红;锚点找不到也红。
- 基线证据:同一套判据跑本轮之前的 style.css 报 120 条(a 21、b 19、c 15、d 23、e 33、g 5、p 4),跑上一版(0270de05)3 条(度量轮次琥珀竖条、被读取绿、批次进度条绿),跑现行样式表 0 条。
- 批次格断言:已完成格必须是 `.doc-item .complexity-cell.filled, .focus-card .complexity-cell.filled { background: var(--dim); border-color: var(--dim); }`,且不得再出现 `.pri-P*…complexity-cell`。

**运行时守卫**(ui-runtime-smoke):阻塞 = 0 的计数带 is-zero、= 2 不带(实渲染 + 直接调 backlogStat 两路);子代理等待批准渲染出 `.sa-glyph[data-state=attention]`、字符 ⏸,主线等首个 token 仍是 waiting;人格签不带 line-accent-*;活动面板没有 .bg-dot、编排轨迹以 .bg-tool 角色名为身份;状态栏 ⚡ 与输入区模型芯片 .picker-fast 的 ⚡ 都是文字字形(textContent 精确等于 ⚡ + U+FE0E)。ui-a11y-smoke 另静态断言 index.html 里「自动放行」写成 `⚡&#xFE0E;`、全文件没有不带 U+FE0E 的 ⚡。

**样例页**(ui-surface-gallery-smoke,由 ui-lint-smoke 调用):亮色静态矩阵是嵌套在暗色页面里的 [data-theme="light"] 区块,:root 上以 var() 定义的 token(--danger、--alert、--dot-idle 等别名与组件层 --surface-*)在 :root 就求好了值,区块里不重声明就是暗色值(曾经危险菜单项 #ff7b72 在白底上 2.52:1)。gallery.js 在区块上重声明 :root 里所有 var() 值(亮色块自己给了值的除外);冒烟逐个比对区块上约 170 个主题 token 的计算值与 html[data-theme=light] 上的计算值,不等即红。

## 6. 对比度(WCAG,文字 ≥ 4.5,非文本 ≥ 3)

**暗色**

| 文字 \ 底色 | 主区 | 侧栏 | 卡片 | 次级卡片 | 浮层 | 输入区 |
|---|---|---|---|---|---|---|
| fg | 14.10 | 13.08 | 12.48 | 11.39 | 11.09 | 10.48 |
| fg-strong | 17.76 | 16.48 | 15.72 | 14.35 | 13.97 | 13.20 |
| dim | 6.63 | 6.15 | 5.87 | 5.36 | 5.21 | 4.93 |
| accent-text | 7.72 | 7.16 | 6.83 | 6.24 | 6.07 | 5.73 |
| ok | 8.64 | 8.02 | 7.65 | 6.99 | 6.80 | 6.42 |
| err | 7.04 | 6.54 | 6.23 | 5.69 | 5.54 | 5.23 |
| warn | 9.63 | 8.94 | 8.52 | 7.78 | 7.57 | 7.16 |
| info | 8.29 | 7.70 | 7.34 | 6.70 | 6.52 | 6.16 |

- 状态栏:普通 6.63,运行态 7.15;rail 徽标 7.72。
- 悬停中的焦点卡(--panel2 叠 --surface-hover ≈ #353535,常见文字底里最亮的一块):dim 4.59,「在做」胶囊(accent-text 叠 accent-soft)4.54,阻塞胶囊 5.18,待办胶囊(fg 叠 badge-soft)7.65,P2(fg 无底)9.76。
- 非文本:accent 在主区 4.54、在侧栏 4.22;白箭头在 accent 上 3.91;焦点环在主区 / 卡片 / 侧栏 6.31 / 5.59 / 5.86。

**亮色**:最低一项是 ok 在次级卡片上 4.81;dim 最低 5.21,accent-text 最低 5.66;accent 在主区 3.91、在侧栏 3.65,白箭头在 accent 上 3.91。悬停中的焦点卡(≈ #e9e9e9):dim 4.82,「在做」胶囊 4.69,阻塞胶囊 4.68。

## 7. 未做与后续

- 移动端 PWA(crates/kanzei-app/mobile-pwa)仍是独立的 Tailwind 蓝、只有浅色:用户决定先不动,门禁里「PWA 配色待用户决策」的注释保持原样。
- 背景神经流只改了取色 token;背景渲染器本身另有改造计划,本文不约束其形态。
- 输入区几何(发送键尺寸、芯片排布)另有改造计划;本文只约束颜色。
- 仍有彩色 emoji:通知里的 ⚠️(带 U+FE0F,强制彩色)、窗口标题的 🔔、模型芯片 tooltip 里的 ⚡(系统提示框渲染)。前两者属于通知文案,留待下一轮统一处理。
- `.bg-child.warn/err/running` 的状态左边框暂时保留(活动面板子行,边框就是它的状态位),`.sv-check-fail` 的红色左边框保留(自检失败项的引文式缩进块);两者已在 ⑥b 的 STRIPE_EXCEPTIONS 显式登记。.bg-child 是否也改成中性、只靠子行文字表达状态,待用户拍板。
- 焦点卡悬停仍叠 --surface-hover:本轮选择微调 --dim / --accent-text(暗)与 --accent-text / --warn(亮)让所有胶囊过线,而不是去掉悬停叠色;若以后加深悬停叠色,③b 会先红。
- --badge-info 已无人引用,--line-1..4 为弃用别名,下一版随 token 清理一并删除。
