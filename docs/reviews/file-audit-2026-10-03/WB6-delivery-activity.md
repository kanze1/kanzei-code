# WB6：附件、交付物与活动状态

日期：2026-10-04。基线 HEAD 26acb7f0，保留 WB1–WB5 未提交改动。本包新增全文 4 文件、协同修复 2 个 caller 切片。修复 2 个 P1、4 个 P2，无新增依赖、无提交或发布。

## 依赖地图与状态边界

1. 附件：资源分类/compose → PromptAttachment → state::prompt_attachment_parts → attachments::document_part → run::assembly → 模型输入。解析失败在进入 writer/provider 前收尾当前输入，不把二进制或旧内容当正文继续。
2. 交付：SessionStore file.delivered + 当前文件 metadata → delivered_files → 06-deliveries 项目缓存 → 05-chat-render/Softwire/Markdown 卡片。子代理折叠过程里的旧 display 是回放副本，不能覆盖刷新后的当前事实。
3. 侧栏：05-subagents/06-activity 的任务事实 → 06-side-policy → 06-agent-panel 单一 DOM 写入者。物理指针/焦点属于面板；运行压制/失败/计时属于会话。
4. 活动表现：sessionStates/transitionSession 是运行真源；Rust experience_events → 01-core delta/fact 队列 → neuralFlowEmit → visual-runtime/constellation → 22-activity-state。动画缓存不回写运行状态；同会话前序进度先于终态投递。

并发边界是异步 IPC 快照、折叠详情延迟渲染、浏览器帧队列和会话切换。附件为纯解析，无锁和持久写入。此次没有变更存储格式、文件布局或后端执行协议。

设计依据：[子代理呈现](../../design/subagent_presentation.md) §5.6/§7、[背景表现](../../design/ui_chat_backdrop.md) §4，以及源码 R-086/R-206 会话状态合同。沿用既有设计，不恢复被移出计划的手机/语音专项；共享活动投影仅检查这里的通用状态合同。

## crates/kanzei-app/src/attachments.rs

### 职责
将用户上传的文字/电子表格转换为提供给模型的文本，保留单元格坐标、保存时数值和公式，并显式标注读取上限。

### 判断
P1

### 确切问题
- Excel 使用 `[h]:mm:ss` 等累计时长格式时，calamine 将其标记为 `ExcelDateTimeType::TimeDelta`；当前 `cell_text` 对所有 `DateTime` 调用 `as_datetime()`，把 36 小时变成 1900 年日期，工时等数据含义错误。

### 修改
- 在单元格唯一格式化入口先识别 duration，以已有 calamine/chrono 输出 ISO 时长；普通日期保持原行为。公式标注与字符上限继续复用原逻辑。
- 新增真实 `duration.xlsx` 与两项回归，覆盖大于 24 小时、负数、秒小数、零、普通日期、公式和 1900/1904 日期系统。

### 影响范围
- 依赖：base64 + calamine（已有 chrono feature）→ 私有 decode_text / column_label / cell_text / spreadsheet_text → document_part。
- callers：08-compose-runtime.js / 04-resource-types.js → PromptAttachment（state.rs）→ prompt_attachment_parts → document_part → run/assembly.rs 生成 typed user parts → provider。
- 持久状态拥有者在上层 conversation/store；此文件仅同步内存解析，没有锁、全局状态、文件写入或异步回调。
- public API、IPC、持久化 schema 和工作表读取上限均不变；只改时长的文本表示，不重新计算公式。

### 验证
- Fixture XML 与 SHA256：output/audit-WB6/attachments/fixture.json；`tests/fixtures/attachments/duration.xlsx` 为 5,011 bytes，SHA256 `5db75cf54eb686a28dfc1ada3bc0be183ea18ca3c5f7e0792d9cfe0e86ae4ad7`。
- 新增 `spreadsheet_durations_keep_their_meaning_in_model_text` 与 `duration_cells_preserve_formula_and_ignore_calendar_epoch`，旧代码负对照：3 原有测试通过、2 新回归失败，退出 101，旧实现实际输出 1900 年日期；修复后两项通过。
- 原有 3 tests 覆盖多表格数值/公式、损坏工作簿与不支持格式失败、UTF16 BOM 与输出截断。`cargo test -p kanzei-app attachments::tests:: -- --nocapture` 已通过全部 5 项。
- 直接 caller `prompt_attachments_become_image_and_document_parts`、失败恢复 `startup_pre_writer_failures_only_finalize_current_input` 均通过。已核对错误经 collect/finish_startup_failure 返回，未吞错输出空正文。
- 全文检查其余 MAX_BYTES/base64 上界、UTF8/UTF16 完整性、值与公式范围并集、行列/表数上限和截断标注，未发现实际问题。

- 本分支已运行 `rustfmt --check crates/kanzei-app/src/attachments.rs` 与该文件 `git diff --check`，均通过。

## crates/kanzei-app/ui/06-deliveries.js

### 职责
拥有项目交付回执缓存，协调刷新与实时登记，匹配会话内的交付文件并装饰回复。

### 判断
P1

### 确切问题
- P1：展开旧子代理过程会把相同 id 的 available 旧状态写回刚刷新为 unavailable/changed 的缓存，也会让较早回执覆盖同一会话/路径的较新回执；交付页和工具卡因此显示旧状态。
- P2：回复卡片签名仅含 id/path/status；文件持续变化、current_bytes 变化但状态仍为 changed 时，卡片保留旧尺寸。

### 修改
- registerDelivery 对相同 id 保留当前 owner row，对有完整数字时间且现有时间更晚的旧回执保留新 row；返回实际选择的 row。拒绝重放时不增加 revision，避免假冒新的实时写入。
- 签名纳入 renderFileCard 使用的 name/caption/bytes/current_bytes/project_dir，使相同回执的新大小可以刷新。

### 影响范围
- registerDelivery 唯一生产 caller 05-chat-render.fillToolBlock 改为使用返回 row；间接影响子代理详情、回复卡片、交付页。
- deliveryState/onDeliveriesChanged/loadDeliveredFiles/matchDeliveredFile/deliveredFileFor/decorateDeliveredReply 的外部参数保持不变。
- 无后端、schema、文件布局变更；同时间不同 id 的有效新回执不被拒绝。

### 验证
- scripts/ui-delivery-contract-smoke.mjs：真实 Edge 和生产 ESM，17/17 PASS；精确 git 26acb7f0 的 06-deliveries.js 与 05-chat-render.js 旧对照为 12 PASS / 5 assertion FAIL。
- 使用真实 subagentStart/subagentProgress/createSubagentView/renderSubagentTimeline；仅后端 IPC 返回值为可控夹具，未替换缓存合并、子代理/卡片渲染函数。
- 验证：同 id 新状态不倒退、旧回执不覆盖较新回执、工具卡选中同一 owner row、尺寸20→40 B更新、同毫秒不同 id 可登记、相同名字跨项目/会话不串、歧义不选、相对路径和 Windows 别名匹配、在途 load 去重及新实时记录保留、读取失败保留记录并可重试。
- 既有 scripts/ui-delivery-browser-smoke.mjs：13项 PASS；三个变更文件 ESLint PASS；git diff --check PASS。

## crates/kanzei-app/ui/05-chat-render.js（caller 切片）

### 职责
工具结果收尾时将交付回执登记，并绘制工具卡；本轮只检查此调用切片。

### 判断
P1（同一根因配套修复，不重复计问题）

### 确切问题
- owner 拒绝旧回执后，若仍用传入 display 渲染，工具卡仍显示已失效旧状态。

### 修改
- 使用 registerDelivery 返回的实际 row 绘制；未能登记的缺上下文 display 保留原有显示 fallback。

### 影响范围
- 顶层工具结果与子代理时间线共享 fillToolBlock；无新增 IPC、无工具执行语义变化。

### 验证
- 上述真实子代理展开回归同时断言 owner 与工具卡状态、最新 caption 一致；旧卡失败、修复后通过。

## crates/kanzei-app/ui/06-side-policy.js

### 职责
纯函数维护侧栏开合策略：共享的固定/交互状态，以及按会话区分的运行、失败保留和自动收起状态。

### 判断
P2

### 确切问题
- 鼠标/焦点位于同一个面板时，异步切换活动会话不会重新触发 enter；原实现将 hover 留在旧会话。新会话会在鼠标下自动收起，切回旧会话又可能一直不收起。

### 修改
- hover 移到共享 model，与 06-agent-panel 单一面板的 pointer/focus owner 一致；运行压制、失败 hold、收起计时仍属于会话。

### 影响范围
- createSideModel/sideEvent/sideDecide 的生产 caller 仅 06-agent-panel；已查全部引用，没有 caller 直接读取 line.hover。
- 参数与返回合同不变，无持久化变化；用户固定打开、手动关闭压制、抽屉和失败徽标保持原规则。

### 验证
- 真实 Edge：原生鼠标进入面板 → 切换 activeSessionId 并执行 agentPanelSync → 真实 subagentEnd → 注入既有时钟，验证鼠标下保持打开、移出后恢复收起。
- 当前 12 PASS；精确 26acb7f0 旧 policy 为 9 PASS / 3 FAIL。纯策略回归同时验证旧会话不留 hover、pinned、suppressed、新运行、失败徽标、抽屉和非对话视图；浏览器错误为零。


## crates/kanzei-app/ui/22-activity-state.js

### 职责
按会话缓存动画细节，并以运行真源决定当前活动状态。

### 判断
P2

### 确切问题
- 用户或自动续跑已进入 starting 时，旧 blocked、1800ms 内的 complete 或背景中未读到终态的 executing 缓存仍会优先显示，直到新后端 turn 到达。

### 修改
- current() 遇到 starting 清除该会话表现缓存并返回 thinking，避免旧轮状态压过新启动意图。正常 done 到 idle 之间的完成效果仍保留。

### 影响范围
- 直接 caller：22-constellation；只影响欢迎页/语音舞台启用复杂视觉时的反馈。无 IPC、持久化、业务状态或 public API 变化。

### 验证
- ui-activity-contract-smoke.mjs：生产模块直接 ESM 导入，精确 26acb7f0 旧文件 6/10 通过、4 个实际断言失败；修改后 10/10 通过。
- 验证 failed/completed/stopped 到 starting、未结束工具缓存、后台切换、并行工具、完成期限、runtime 终态优先、runtime 不被修改。
- 原 ui-activity-state-smoke.mjs 通过。

## crates/kanzei-app/ui/01-core.js（协同切片）

### 职责
将后端体验事件归并并投递到表现层，保证会话归属及事件顺序。

### 判断
P2

### 确切问题
- 同帧工具 progress 被缓存，completed fact 立即投递；下一帧旧 progress 重建已完成工具，使光效继续 executing。旧文本同理会在 run_started 后重新将等待轮标成 replying；legacy kz:done 后尚未 idle 的窗口也会被旧 progress 冲掉 complete。

### 修改
- flushExperienceDeltas(sessionId = null) 支持按会话排空；无参数帧回调仍全量排空并解除 scheduled 标记。
- 当前会话非 delta 体验事件分发前先排空其前序 delta；legacy done/idle/stopped/终态 error 处理前也排空对应会话。其他会话保留原帧调度，避免造第二套工具生命周期状态。

### 影响范围
- 01-core 的体验事件和 legacy on 路由；22-neural-flow →22-activity-state 表现投影。保持既有 WB2 终态 updateSessionStage 修改。
- 无持久化或 IPC 格式变化，无参 public flush 兼容。后台 delta 仍不向前台发动画；文本合并内容/数量保留。

### 验证
- ui-experience-contract-smoke.mjs：真实 Edge、生产 ESM、原 handleExperienceEvent、真实 on(kz:done/idle) 经 mock transport 投递；精确 26acb7f0 的 01-core 旧文件 8/17 通过、9 失败；修改后 17/17 通过。
- 覆盖 progress→completed、文本→新 turn、legacy done、partial flush 后 scheduled flag、后续同帧文本、背景终态只清自身且不抢前景文本、并行工具保留，以及零 browser error。
- 四个生产/测试文件 ESLint 通过；针对修改 git diff --check 通过。

## 整合验证与进度

- 新增回归共 **58 项**：附件 2、交付 17、侧栏 12、活动状态 10、事件顺序 17；当前全部专项通过。每组均有精确旧代码负对照。
- Rust：附件 5 项、模型输入 caller 1 项、启动失败收尾 1 项通过；cargo check -p kanzei-app、Clippy all-targets -D warnings、cargo fmt --all -- --check 通过。
- 完整 UI runtime：exit 0，90 个模块、3606 次初始化 invoke、10 个主视图、0 运行时错误，深浅主题 Harness 操作通过。新增 56 项 UI 回归均接入该 gate。
- UI lint smoke：exit 0，190 文件、1646 导出 ESM 守卫及布局/弹层/背景浏览器检查通过；npm lint、四个新脚本 ESLint 通过。
- 证据目录：output/audit-WB6/，包含各组 before/current、Rust 日志、runtime.log 和 ui-lint.log。
- 完成本包后新增全文累计 27 文件；四块 88/268 已完成或可复用、180 待处理；工作台 40/51。全仓历史全文 163/514，caller 切片不计全文。
- 下一包 WB7：workspace.rs → workspace/tests.rs → 26-project-conversations / 28-async-workspace，先核实项目概览快照与身份，再核对前端异步接管和回复目标。

# Module Summary

## 已修复
- P0：0。
- P1：2，表格时长语义错误、旧子代理交付回执覆盖当前状态。
- P2：4，交付大小不刷新、侧栏 hover 会话错位、新轮继承旧活动表现、前序 delta 越过 fact/终态。

## PASS 文件
- 无独立 PASS 文件；4 个全文文件均有已复现问题，其余未发现实际影响的分支保持不动。

## 仍需人工判断
- 无。

## 依赖影响
- registerDelivery 返回实际选中记录，唯一 caller 同步使用；flushExperienceDeltas 新增可选 sessionId，无参合同保留。运行真源、持久格式和 IPC 不变。

## 剩余风险
- 未运行原生 Tauri/WebView2 + 真实模型的端到端联调；本轮证据为 Rust 实际解析/运行失败路径、生产 ESM 与真实 Edge 配合隔离 IPC。
- 未重跑 Rust 全工作区；本次只修改 app 的纯附件转换分支，验证覆盖该包及直接 caller。
- C7 原有 3 个 P1 仍在第二块排队，未计入本包已修复。
