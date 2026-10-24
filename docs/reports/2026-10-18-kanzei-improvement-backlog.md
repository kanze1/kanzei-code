# Kanzei 10 月 18 日问题观察与验收条件（历史记录）

本文保存原始问题和后续复核，不作为当前待办队列。活动状态以 [需求](../../.kanzei/project/requirements.md)、[缺陷](../../.kanzei/project/defects.md) 为准；当前能力见 [实现口径](../current-state.md)，下一步候选方向见 [10 月 23 日调研](2026-10-23-optimization-research.md)。下文日期内的“当前”“缺口”“未找到”仅指对应核对时点。

## 2026-10-23 文档对账

| 原始条目 | 现有实现及剩余证据边界 |
| --- | --- |
| BRO-01/02/03/04 | 已有 select/check/upload/validate、本地 CDP connect/pages/frames 和 connection/page/URL/frame 身份；面板路由按活性与归属，隐藏不再触发切换。见 [浏览器合同](../design/preview_pane.md)。完整表单、真实面板和外部登录态仍按场景分别验收 |
| MEM-01/02 | 已有机会记录、修订绑定的采用/纠正/撤销反馈、冷记忆保留、隔离归档与恢复；见 [记忆反馈合同](../design/memory_feedback_reliability.md)。实际任务收益与误退役率仍需测量 |
| MEM-03/04/05 | 已有对话做梦入口、冻结来源与 hash、说话人和证据片段、待复核候选及整理作业。不能再按“缺少做梦入口”立项，也不能从代码存在推导长期偏好提炼准确率 |
| WEB-03 | 已有研究归属、来源工件与内容 hash，浏览器 source 可保存渲染正文；见 `browser_tool/source.rs`、`web_refs.rs` 与 `research_verify.rs`。来源完整性和引用语义正确性分别验收 |

10 月 22 日已核对的 8 项保留在下一节。本次没有重做其余安全审计，也没有把这 27 项批量关闭；原验收条件继续作为历史追溯依据，当前排期不再复制文末原实施顺序。

## 2026-10-18 原始范围

整理日期：2026-10-18。代码基线：8d479de1。范围包括已认可的四个问题、安全审计、网页搜索审查、浏览器填表、记忆升降级与对话做梦。

共 27 项，分为运行与上下文 4 项、安全边界 7 项、网页搜索 7 项、浏览器操作 4 项、记忆与做梦 5 项。P1 表示优先修复当前正确性、数据范围或明显效率问题，P2 表示随后完成的功能与架构工作，P3 表示后续扩展；这些优先级不是漏洞严重度评分。编号用于后续拆任务，不代表已经登记到 tracker。

当前两份来源文档都标注 2026-10-21，而本次会话审计发生在 2026-10-18；保留原文件名，日期事实纳入 RUN-04 纠正。本清单整理需求和验收条件，未实施功能，也未执行记忆清理。

## 2026-10-22 状态复核

下文保留 10 月 18 日的原始观察与验收条件，不再把已经实现的路径当成当前缺口。交接报告列出的 8 项有以下实现和可执行回归；本轮执行结果统一保存在 `output/session-handoff-repair/`。其余条目保留原状态，本表不替代 tracker。

| 编号 | 当前实现与回归锚点 |
| --- | --- |
| RUN-02 | 已修复整轮持有写资格：`crates/kanzei-app/src/run/assembly.rs` 的 `dev_run_can_reach_provider_while_another_run_owns_the_mutation_slot` 在另一运行持写资格时实际到达 mock provider。实际变更仍由工具执行路径取得写资格。 |
| RUN-03 | 已修复持久结果与实时终态冲突：`crates/kanzei-app/src/run/persistence.rs` 的 `durable_outcome_*` 用例覆盖成功、失败、停止及持久化失败后的发布顺序。真实安装版现场复现与本地回归分别记录。 |
| SEC-01 | 已接通统一敏感文件权限：`crates/kanzei-harness/src/permission/{sensitivity,filesystem}.rs`；`crates/kanzei-core/src/runner/drive/execution_tests.rs` 的 `credentials_ignore_auto_allow_and_broad_session_rules_but_honor_exact_user_consent` 验证 auto_allow/宽规则不放行、逐文件真实授权可放行。 |
| SEC-03 | 已在执行前拒绝越过步数预算的工具调用：`crates/kanzei-core/tests/context_assembly.rs` 的 `exhausted_budget_rejects_provider_tool_calls_before_side_effects` 使用仍返回工具调用的 mock provider，并核对副作用文件不存在。 |
| SEC-07 | 已在子代理准入与最终执行路径强制只读：`crates/kanzei-core/src/runner/drive/execution_tests.rs` 的 `readonly_child_boundary_rejects_an_incorrectly_constructed_snapshot` 和串行/并行执行用例。 |
| WEB-01 | 已按显式后端路由：`crates/kanzei-tools/src/websearch.rs` 的 `explicit_duckduckgo_never_contacts_codex`、`explicit_codex_failure_never_contacts_duckduckgo` 通过真实本地 HTTP endpoint 检查。 |
| WEB-04 | 已区分 challenge、页面形状失配与正常零结果：同文件 `a_changed_html_layout_is_not_evidence_of_zero_search_results`、`each_query_attempt_is_explicit_and_captcha_is_not_empty_success`。 |
| WEB-05 | 已阻止 JS 空壳、登录页、挑战页冒充已读正文，并保留原始响应、正文状态和浏览器/原始 API 后续路径：`crates/kanzei-tools/src/web_content.rs`、`webfetch.rs` 的 `unreadable_html_is_not_evidence_and_numbered_preview_resumes_from_cache`。上述用例不代表真实受限站点已取到正文。 |

## 2026-10-18 的能力与缺口观察

### 浏览器填表已有基础

浏览器工具已有 open、dom、click、type/fill、press、scroll、wait、eval 和截图。桌面端的内部网页面板可通过 CDP 点击、聚焦并输入；无头路径通过 Playwright 启动独立的 Edge/Chrome，使用 page.fill 填写输入框。现有回归测试包含连续 open → type → click → dom，不能把当前能力写成“完全不会填表”。

但“工具启动的 Edge”与“用户已经打开并登录的 Edge 标签页”是两个场景。当前 helper 使用 chromium.launch 和新 context，未提供外部 Edge 标签页枚举、选择与接管入口。内部面板也受可见状态、会话归属和路由条件约束。本轮只核验了代码及现有测试入口，尚不能把两条路径都标成当前版本已经完成真实表单验收。

| 使用场景 | 当前判断 | 应处理的事项 |
| --- | --- | --- |
| 工具自行启动的 Edge/Chrome，填写普通文本框并点击 | 有实现，有连续交互回归用例 | BRO-01 做当前版本的完整提交验收 |
| Kanzei 内部网页面板填写普通表单 | 有 CDP 输入与点击实现 | BRO-01、BRO-03 验证真实面板和状态连续性 |
| 用户现有 Edge 标签页，复用登录后填写 | 未找到标签页接管与会话授权入口 | BRO-02 |
| 下拉、多选、上传、iframe、多标签页、原生对话框 | 现有动作未形成完整的一等操作契约；部分页面可经 eval 操作 | BRO-01、BRO-02、BRO-04，不能仅凭 eval 宣称全部支持 |

代码证据：[共享浏览器工具](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-tools/src/browser_tool.rs:1>)、[无头浏览器启动](<C:/Users/kanzei/Documents/kanzei code/scripts/browser-helper.mjs:86>)、[桌面端路由](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/harness_ext.rs:340>)、[内部面板填入文本](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/agent.rs:248>)、[连续交互测试](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-tools/src/browser_tool/headless.rs:625>)。

### 记忆整理和部分生命周期已经接线

当前有 candidate、shadow、active、deprecated、invalid 状态，有来源门禁、晋升、归档与恢复。成功轮次会采集失败和已完成条目的 SOP/fact，再触发 inbox 整理与 candidate 处置；记忆页也有手动整理入口。候选记忆已有超龄清退和容量清退，默认年龄上限为 14 天、容量水位为 24 条。

缺口主要在 active 记忆的长期效用反馈、可解释的自动降级和零命中清退，以及完整对话到偏好、品味、决策、踩坑的做梦流程。当前“长期零命中”主要是 UI 按 hits=0 且更新时间距今至少 3 天的标记；使用观测接口明确供人工复查，并非自动退休机制。现有轮末采集也不等同于通读长对话并提炼用户偏好。

代码证据：[轮末采集及生命周期默认值](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/mod.rs:1139>)、[轮末自动整理调用](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/run/persistence.rs:189>)、[生命周期判据](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/lifecycle.rs:104>)、[候选处置](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/store.rs:703>)、[使用观测](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:487>)、[手动整理](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:649>)、[零命中标记](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/13-memory.js:1294>)。

## 运行与上下文

对应已认可的上下文效率、并发与写锁、状态真实性三个问题。

| 编号 | 优先级与现状 | 需要处理的问题 | 完成条件 |
| --- | --- | --- | --- |
| RUN-01 | P1，运行记录确认 | 主上下文读入过多源码和网页；子代理成果仍伴随大规模重复阅读，压缩后继续消耗上下文。 | 调查先定位问题、按段取证，子代理返回结论与可追溯证据；保留必要复核。用同一批任务比较重复读取量、上下文峰值、压缩次数与结论正确性，不把少读本身当成功。 |
| RUN-02 | P1，排队记录确认 | 调查任务为最终写报告长期持有共享代码树写资格，另一条只读调查等待约 18 分钟。 | 只读调查可以并行；写入在实际提交变更时获取必要范围的写资格；确需独立源码修改时复用 worktree 机制。验证双会话调查并行且交付写入无冲突。 |
| RUN-03 | P1，终态冲突确认 | p39 已完成却在协作快照显示 failed；p40 等写资格时被标疑似卡住；送达状态与运行结果也需区分。 | 持久完成事实、内存运行态、协作快照与 UI 由一致的状态转换驱动；排队应说明等待原因。成功、失败、停止、等待批准、排队与重启恢复均能回放一致；已接受的普通送达回执不常驻正文。 |
| RUN-04 | P2，报告日期错误确认 | 日期、代码基线、测试和发布事实容易由模型自行补写；两份本轮报告出现未来日期。 | 引擎提供时间、时区、run/episode、HEAD、实际执行的测试与产物信息；报告引用这些事实，并区分静态读码、测试通过、真实端到端、发布及安装。 |

RUN-03 的根因定位指向成功收尾未同步 LiveRun.last_outcome，而协作状态将缺失结果归为 failed：[状态投影](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/collaboration.rs:58>)、[内存结果写入](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/state.rs:239>)、[收尾路径](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/run/persistence.rs:338>)。原始观察保存在 [终态事件](<C:/Users/kanzei/Documents/kanzei code/output/session-audit-p39-20261018/terminal-evidence.json>) 和 [协作快照](<C:/Users/kanzei/Documents/kanzei code/output/session-audit-p39-20261018/latest-status.json>)。

## 安全与执行边界

对应已认可的第四个问题：权限批准和实际执行隔离必须分别成立。以下“路径确认”不表示已经完成攻击或发生泄漏。

| 编号 | 优先级与现状 | 需要处理的问题 | 完成条件 |
| --- | --- | --- | --- |
| SEC-01 | P1，读取路径确认 | 默认文件读取可让工作区 .env、私钥等进入模型上下文；路径围栏未提供敏感内容策略。 | 在统一内容访问层保护敏感资源，覆盖 read、grep、预览、浏览器及子代理；明确授权能按范围放行。正反用例证明默认不暴露秘密且普通开发读取可用。 |
| SEC-02 | P1，服务范围确认，浏览器攻击待复现 | 获准打开一个本地 HTML，实际预览服务可读取登记根内其他文件；项目页面没有隔离 CSP。 | 让授权范围与实际服务范围一致，同时保留合法模块/资源加载；验证恶意页面不能读取未授权 sibling 或秘密文件，浏览器外传路径也纳入验收。 |
| SEC-03 | P2，静态路径确认 | max_steps 最后一步只移除 schema，dispatch 未硬拒 provider 继续返回的工具调用。 | 在最终执行入口强制预算与终态；恶意 mock provider 返回有副作用工具时，不产生文件、进程或网络动作。 |
| SEC-04 | P2，架构边界确认 | 普通 Bash 以宿主当前用户身份执行，宽授权不提供 OS 级隔离。 | 为不可信仓库自主运行明确文件、网络、进程树和子进程边界，选择可落地的隔离方案并实测；不能把确认框或文件快照标成沙箱。 |
| SEC-05 | P2，宽授权下的条件风险 | 第三方 Agent/IDE 启动配置缺少特殊资源策略，可能影响另一个工具之后的执行。 | 创建、改写与重命名均走同一敏感配置策略，覆盖审计列出的 Cursor、VS Code、Claude、Codex 文件；验证宽授权下的实际行为。 |
| SEC-06 | P1，信任边界需补强，可利用性待验证 | 网页正文、搜索摘要及仓库外部材料可能把指令混入证据，普通输出缺少统一的不可信内容契约。 | 外部内容携带来源和信任级别，只作为数据；与执行权限共同约束。对抗页面验证不能诱导未授权执行、泄密、换域或替换用户任务。 |
| SEC-07 | P2，调用方契约待补强 | 子代理只读不变量仍依赖调用方正确组装 snapshot 和 writable 字段。 | 收紧构造与最终执行契约；即使错误传入可执行 Bash 的 snapshot，只读子代理也不能写入或执行有副作用动作。 |

来源：[安全审计及其回归证据](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-10-21-code-agent-security-audit.md>)。已防护的路径越界、symlink、WebFetch SSRF、已审阅本地服务认证不重复列为新漏洞，修复时保留对应回归。

## 网页搜索与证据

本地 websearch/webfetch 有实际工作记录；当前问题包括路由和配置契约、Hosted 分支未贯通、证据及质量保障，不应概括成“网页搜索全部坏了”。

| 编号 | 优先级与现状 | 需要处理的问题 | 完成条件 |
| --- | --- | --- | --- |
| WEB-01 | P1，配置与代码路径确认 | search_backend 没有控制执行路由，选择 DuckDuckGo 仍可能先向 Codex 发查询；私有 alpha/search 适配器也缺少清晰的支持边界。 | 后端选择只有一个真实入口；显式 DDG 不取 Codex 凭据、不发 Codex 请求；auto、codex 的失败策略明确。用 mock 验证实际 endpoint，输出真实 backend/fallback，不将旧探针视为长期可用保证。 |
| WEB-02 | P2，接线缺口确认；权限是启用前门槛 | native_search 配置到 Runner、LlmRequest 未贯通；恢复接线还必须补 Ask/Deny、无人值守和 citation 展示契约。 | 先确保权限拒绝能关闭托管搜索，再接通或明确标成未支持；以模型能力决定可用性。验收请求、事件、正文可点击引用及历史回放，验证 offset 和无效 URL 处理。 |
| WEB-03 | P2，存证粒度确认 | finding 只证明同会话曾抓过 URL，没有绑定本任务审阅的内容版本；来源预览也缺项目/session 上下文。 | 来源绑定 topic、task、请求/最终 URL、时间、artifact 和内容 hash；finding 可回读对应版本。临时预览与研究存证有清楚契约，不产生错误归属。 |
| WEB-04 | P2，解析鲁棒性缺口，实时故障待复现 | DDG 页面改版、challenge 或结构异常可能被误报为正常零结果。 | 区分真零结果、验证页、限流、结构失配和服务失败；覆盖页面形状 fixtures，让不可解释的空结果成为可诊断结果。 |
| WEB-05 | P2，p39 抓取失败记录确认 | JS 壳、动态页面或受限页面拿不到正文时，缺少明确的后续取证路径；HTTP 200 不代表已读正文。 | 报告正文提取状态；按页面类型选择受控浏览器渲染、第一方 API 或替代来源，受权限与预算约束。保留最终正文/来源，无法获取则标成未核验。 |
| WEB-06 | P2，质量回归与覆盖缺口 | 有调用成功和解析测试，但缺搜索相关性、时间过滤、来源覆盖与回答引用正确性的可复现评价；p39 未独立覆盖 CodeBuddy。 | 固定 query/语料分别测检索和回答；低频 live smoke 观察供应商健康。研究任务跟踪每个请求对象、未覆盖项及来源等级，不能把工具零错误当成覆盖完整。 |
| WEB-07 | P3，场景扩展需求 | 本地 schema 主要有 recency 和正向域名，地区、屏蔽域、查询预算等控制不足。 | 先覆盖明确需要的地区/排除域/预算场景，形成后端可解释的能力与权限契约；不支持的控制应明确返回，不靠在 query 里拼自然语言暗示。 |

来源：[网页搜索研究与仓内设计审查](<C:/Users/kanzei/Documents/kanzei code/.kanzei/research/web-search/prior-art.md>)。本轮再次核对 [后端配置](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-harness/src/config/web.rs:1>)、[搜索路由](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-tools/src/websearch.rs:175>)、[最终请求](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/runner/drive.rs:637>)，配置脱节仍在当前基线。

## 浏览器操作与 Computer Use

| 编号 | 优先级与现状 | 需要处理的问题 | 完成条件 |
| --- | --- | --- | --- |
| BRO-01 | P2，基础已有，完整场景待验收 | 把“能输入点击”完成为可靠填表任务；补足下拉、勾选、日期、上传等常用语义动作，避免常规任务全靠任意 eval。 | 内部面板与受控 Edge 分别完成：识别字段 → 填写 → 校验报错 → 更正 → 本地提交 → 核对成功回显；覆盖 React 受控输入和异步页面，保留动作与结果证据。 |
| BRO-02 | P2，外部 Edge 接管入口缺口 | 不能把独立无头 Edge 当成用户现有 Edge；缺标签页选择、登录态复用与受控接管。 | 明确列出并选择用户授权的浏览器/标签页，支持连接、断开与登录态使用；测试切页、弹窗、新标签和 iframe，任务不误操作其他页面。 |
| BRO-03 | P2，已有路由，连续性需验收 | 面板关闭/遮挡、会话切换、后台与子代理可能切换后端；审批时的页面与执行页面必须一致。 | 页面/浏览器会话有稳定身份，状态和授权绑定同一目标；中途路由变化明确处理，不能在另一个新页面上继续旧表单动作。双会话并发、面板隐藏和服务转发均验收。 |
| BRO-04 | P3，新增能力范围 | 浏览器 DOM/CDP 操作不能覆盖所有原生窗口、系统文件选择器或纯画布界面；尚未找到通用桌面控制工具入口。 | 浏览器任务先走结构化动作；浏览器无法覆盖的原生场景再建立截图/可访问性树、目标定位、鼠标键盘和结果验证。以具体桌面任务验收，不能用浏览器截图冒充通用 Computer Use。 |

涉及文件上传和最终提交时沿用用户任务授权与现有权限系统，动作应清楚显示目标和结果；这里不新增与任务无关的确认弹窗。

## 记忆升级降级 清理与做梦

| 编号 | 优先级与现状 | 需要处理的问题 | 完成条件 |
| --- | --- | --- | --- |
| MEM-01 | P2，已有观测，效用反馈未闭合 | 召回、注入、正文读取不等于实际采用或有效；现有自动晋升偏向失败指纹复发，对偏好、决策等类型不够完整。 | 区分可检索机会、召回、注入、读取、显式采用/纠正、结果证据和未知；按记忆类型解释晋升理由，不能以多命中代替正确或有用。 |
| MEM-02 | P2，候选清退已有，active 自动降级缺口 | 长期零命中条目目前主要标记，缺按机会与有效性自动降权、退出生产注入、归档和恢复的完整闭环。 | 在现有生命周期内定义合法升降级，不另造状态分叉；记录原因、来源与转移。考虑项目活跃度和检索机会，保护明确长期偏好/关键约束；先可恢复归档，真删除遵守保留规则，误清退可撤销。 |
| MEM-03 | P2，新增“做梦”需求 | 长对话后缺会话级做梦按钮；当前 inbox 整理不能代替完整对话提炼。 | 对话菜单提供“做梦/整理本次对话”，以确定的会话和消息范围生成偏好、审美品味、项目决策、反复纠正、踩坑及成功方法；写入现有 note/manager/lifecycle，展示增改退清单和来源，重复点击不重复入库。 |
| MEM-04 | P2，复用现有去重与来源基础 | 做梦和自动整理可能把临时要求泛化为永久偏好，混淆项目/全局作用域，或重复写入、覆盖冲突事实。 | 用户明确表述与模型推断分开，保留时间、适用范围、来源对话与替代关系；推断先入候选，矛盾可追溯。口令等敏感内容不沉淀，长期偏好按证据准入且可修改/撤销。 |
| MEM-05 | P2，已有自动整理，用户可见闭环需验收 | 后台整理已存在，但完成、部分处理、失败与候选最终处置需要统一可见；做梦不能再开一套不可观测后台链。 | 复用整理服务的作业、检查点和事件；显示待处理量、处理结果、停止原因和成本，支持取消与断点恢复。自动轮末整理、手动整理和做梦在并发/崩溃后仍无重复或丢失，并遵守会话资源调度。 |

做梦应提炼能帮助后续任务的具体知识。例如“网页偏好简洁排版、不要卡片堆砌”“长实验启动前先做教师资格验证”“这个项目的 CLI 和桌面端共用某个入口”。一次性的“这张图用红色”应保留原任务范围，除非用户明确把它提升为长期偏好。

## 2026-10-18 的原实施建议（已由后续调研替代）

1. 先修当前正确性与范围问题：RUN-01/02/03、SEC-01/02/06、WEB-01；同时用 RUN-04 统一交付事实。
2. 再完成用户直接感知的流程：BRO-01/03 的内部面板和受控 Edge 填表，BRO-02 的现有 Edge 接管；MEM-01/02/04 提供记忆依据后，完成 MEM-03 做梦按钮并通过 MEM-05 展示结果。
3. 搜索按依赖接通：WEB-02 的权限门先于 Hosted 启用，WEB-03 的来源版本与 WEB-04/05 的取证状态先于 WEB-06 的质量结论；SEC-03/05/07 补执行和调用方不变量。
4. SEC-04 独立推进隔离方案；WEB-07、BRO-04 在具体场景需要时扩展。不要用这些较大工作阻塞已经有基础的表单和做梦交付。

验收按源码行为、定向测试、真实浏览器/桌面端、发布/安装分别记录。静态审查发现不能写成攻击已经复现，已有工具或状态定义也不能写成用户流程已经跑通。
