# 四块通用系统：审计完成与集中修复计划

核对时间：2026-10-03T18:22:00.015Z。基线及 HEAD：`55eaca24750ac431ebe7f577c2f4b7b8cbded6f5`。本轮只审计，未修改产品代码、未提交、未发布。

**剩余 181 个文件已全文审完，当前范围累计审阅覆盖 269/269；待审 0。** 原有 88 个复用之前结论。审阅覆盖不等于问题已修复，也不代表另一任务正在修改的版本已完成整体验证。

| 功能块 | 范围 | 之前审过/可复用 | 本轮审完 | 待审 |
|---|---:|---:|---:|---:|
| 工作台与交互控制台 | 51 | 40 | 11 | 0 |
| 对话执行与恢复 | 79 | 30 | 49 | 0 |
| 工具与代码操作 | 90 | 7 | 83 | 0 |
| 多任务与自动运行 | 49 | 11 | 38 | 0 |
| 合计 | 269 | 88 | 181 | 0 |

本轮 **138 文件 PASS，43 文件涉及已确认问题**；按根因去重 **39 项：P0 2、P1 24、P2 13**。其中 C7 的 3 项是复核既有问题，新增识别 36 项。本轮修复数 0。

## 对日常使用影响最大的链路

- 多会话之间可能串重试内容、截图或浏览器操作。需要统一记录操作发起时的会话及资源 owner。
- 插话与 Stop 存在旧执行继续接新输入、部分成功却整体报错的问题；历史清理也未统一遵守跨进程执行 owner。
- Git 大批量指纹存在双管道互等；跨工作树合并的声明写范围与实际目标不一致。
- 后台注册可能丢更新，失败也可能被当成功；消息队列可能确认模型根本没有收到的消息。
- 工具存在确定的错误结果：读取主树而非当前树的符号、截断 PDF 报成功、合法结构化结果解析失败。

## 依赖地图与状态拥有者

| 依赖链 | 单一事实源 / 状态 owner | 跨边界检查 |
|---|---|---|
| base/harness → core runner → tools → app → UI | 沿用基础层文件锁/CAS，runner 持有一次执行的身份与取消状态 | 工具声明资源范围必须等于实际写目标 |
| session execution → coordinator/history → UI | SQLite 会话事实、执行 owner、writer ACK | CLI/桌面跨进程，不只看当前进程 running 标志 |
| 文件写收据 → batch → Git/跨树验证 | 文件版本与真实写者收据 | 观察新内容不能等同于拥有新内容 |
| background registry / queues → scheduler/team → panels | 持久登记与已交付消息集合 | 多 writer 串行化，持久失败不得先更新运行态 |
| browser helper / preview pane → UI | 每个执行会话的 browser、pane generation、composer 附件 owner | await/锁等待之后复核身份 |
| 定义/协议 parser → tool/API → UI | 规范化路径、原始结构化结果、完整文件 | 解析与编辑共用协议；展示不能复用过期成功状态 |

没有提出新框架、持久模型迁移或无收益的依赖改造。基于现有合同即可修复。

## 按根因合并后的修复顺序

相邻范围一起修，先修共享 owner 再跟进 caller；每包独立回归和提交，最后统一门禁。下面是待执行矩阵，本轮没有启动修复。

| 包 | 范围 / owner → caller | 问题 | 必须验证 |
|---|---|---|---|
| F1 Git 进程与目标树写互斥 | git.rs / git/tool.rs → plan/finalize → runner 调度 | AF-T02, AF-T11 | 5000 路径双管道无挂起；A 操作 B 时与 B writer 互斥；取消释放子进程与租约 |
| F2 持久状态与失败原子性 | background persistent/registration → processes/lifecycle | AUTO-001, AUTO-002, AUTO-003, AUTO-006 | 并发注册/退出不丢记录；损坏或写失败不伪成功；单条日志定时落盘；落库失败内存不先提交 |
| F3 会话执行、插话与停止 | session execution owner / coordinator → history actions / team inbox / push | CONV-AF-01, CONV-AF-02, CONV-AF-03, CONV-AF-04, CONV-AF-07, AUTO-007 | 复用 C7 三个负例并转回归；CLI 与桌面并发历史操作互斥；停止后无旧执行；只确认实际交付的消息 |
| F4 代码版本归属与工作树身份 | runner batch → cross_tree → local_validation / symbols | CONV-AF-05, CONV-AF-08, AF-T03, AF-T04, AF-T05, AF-T08 | 外部编辑不能被无关 read 认领；合法新增文件收据可匹配；留证失败明确返回；工作树 edition/API 图均来自当前树 |
| F5 浏览器与界面会话归属 | headless helper / preview pane → chat error / async workspace / composer | AF-T09, AF-R09, AF-R01, AF-R02, AF-R07 | A→B→A 浏览操作、重试、截图各归原会话；面板锁等待换线重新验 owner；旧行不使用新 scope |
| F6 清理与定时任务精确操作 | quarantine CLI → schedules definition / hosts | AF-T01, AUTO-004, AUTO-005 | 互斥参数两种顺序都拒绝；仅修改 frontmatter enabled；check 与 check-long cron 各自保留 |
| F7 解析与文件结果完整性 | schema / conventions / ASE / grep / webfetch / replay | AF-R05, AF-R10, AF-T06, AF-T07, AF-T10, CONV-AF-09 | 外层 JSON 数组完整；路径字面量原样；标准 ASE；multiline count 一致；大 PDF 不以截断文件报成功；增量事件回放 |
| F8 检查结果与状态展示一致性 | gate → harvest UI；telemetry / architecture identity → UI；route options / collaboration | AF-R04, AF-R03, AF-R06, AF-R08, AF-R11, AUTO-010, AUTO-011 | 重跑 gate 撤销旧绿色；新版行号；成功正文不产生失败诊断；索引统一身份与可见性；窗口随路由；并行活动按 call id 配对 |

## 全部确认问题

| ID | 等级 | 根因所在文件 | 确切影响 |
|---|---|---|---|
| AF-R01 | P1 | [crates/kanzei-app/ui/05-chat-render.js](root.md) | 错误卡重试读取当前全局 lastRequest |
| AF-R02 | P2 | [crates/kanzei-app/ui/28-async-workspace.js](root.md) | 异步工作台旧行使用新 scope |
| AF-R03 | P2 | [crates/kanzei-app/ui/06-activity.js](root.md) | 分栏 diff 右侧上下文行号取旧版本 |
| AF-R04 | P1 | [crates/kanzei-app/ui/20-lines.js](root.md) | 重新检查失败沿用旧的合并通过状态 |
| AF-R05 | P1 | [crates/kanzei-core/src/runner/schema_check.rs](root.md) | JSON 提取优先内层对象而非外层数组 |
| AF-R06 | P2 | [crates/kanzei-core/src/runner/tool_failure_telemetry.rs](root.md) | 成功工具正文含 required 仍留下错误的缺参数诊断 |
| AF-R07 | P1 | [crates/kanzei-app/ui/24-preview.js](root.md) | 异步截图进入后来切换的对话附件 |
| AF-R08 | P2 | [crates/kanzei-app/ui/19-arch.js](root.md) | 合法架构索引链接被隐藏 |
| AF-R09 | P1 | [crates/kanzei-app/src/preview/agent.rs](root.md) | 等到面板动作锁后未重查绑定会话 |
| AF-R10 | P1 | [crates/kanzei-tools/src/conventions.rs](root.md) | 已解码 JSON 字符串被再次解码转义 |
| AF-R11 | P2 | [crates/kanzei-tools/src/architecture.rs](root.md) | 同一索引目标的点段别名逃过重复检查 |
| CONV-AF-01 | P1 | [crates/kanzei-app/src/run/coordinator.rs](conversation.md) | 插话批次在多个事务中部分完成，却整体返回失败 |
| CONV-AF-02 | P1 | [crates/kanzei-app/src/run/coordinator.rs](conversation.md) | 历史 writer errors 被误用为当前插话 ACK |
| CONV-AF-03 | P1 | [crates/kanzei-app/src/run/coordinator.rs](conversation.md) | 停止后旧 runner 通过空 halt 槽误收新插话 |
| CONV-AF-04 | P1 | [crates/kanzei-app/src/conversation.rs](conversation.md) | 历史变更绕过跨进程 session 执行 owner |
| CONV-AF-05 | P1 | [crates/kanzei-core/src/runner/drive/batch.rs](conversation.md) | 无关工具观察把外部修改重新归属为本批 |
| CONV-AF-07 | P1 | [crates/kanzei-app/src/run/mod.rs](conversation.md) | 自动 push 没有等待上限，也不参与本轮取消 |
| CONV-AF-08 | P2 | [crates/kanzei-core/src/runner/redundancy.rs](conversation.md) | 测试冗余提醒使用写入前的陈旧 Git 指纹 |
| CONV-AF-09 | P2 | [crates/kanzei-core/src/replay.rs](conversation.md) | 回放解析要求整包配对，但运行侧已逐事件落库 |
| AUTO-001 | P1 | [crates/kanzei-tools/src/background/persistent.rs](automation.md) | 后台持久登记读改写丢更新 |
| AUTO-002 | P1 | [crates/kanzei-tools/src/background/persistent.rs](automation.md) | 后台登记读写失败被当成功 |
| AUTO-003 | P2 | [crates/kanzei-tools/src/background/registration.rs](automation.md) | 小输出长驻服务日志没有按两秒落盘 |
| AUTO-004 | P1 | [crates/kanzei-tools/src/schedules/mod.rs](automation.md) | 启停编辑不遵守自身定义解析规则 |
| AUTO-005 | P1 | [crates/kanzei-tools/src/schedules/hosts.rs](automation.md) | 服务端 cron 更新误删名称有前缀关系的任务 |
| AUTO-006 | P1 | [crates/kanzei-app/src/processes/lifecycle.rs](automation.md) | 对话设置落库失败后内存仍采用新设置 |
| AUTO-007 | P1 | [crates/kanzei-tools/src/team/tools.rs](automation.md) | 子任务收件箱确认了未送入模型的消息 |
| AUTO-010 | P2 | [crates/kanzei-app/src/phase_pipeline.rs](automation.md) | 指定勘察模型后沿用另一个模型的上下文上限 |
| AUTO-011 | P2 | [crates/kanzei-app/src/collaboration.rs](automation.md) | 并发工具尚在执行却报告没有当前工具 |
| AF-T01 | P1 | [crates/kanzei/src/cli/quarantine.rs](tools.md) | 互斥参数仅单向检查，dry-run 命令实际进入 apply 并删除隔离恢复材料。 |
| AF-T02 | P0 | [crates/kanzei-tools/src/git.rs](tools.md) | 父进程写满 git stdin，git 写满 stdout；父进程等写完才 wait_with_output 排空 stdout，形成永久相互等待，命令无法完成。 |
| AF-T03 | P1 | [crates/kanzei-tools/src/cross_tree.rs](tools.md) | 新建文件一律被设为 Fingerprint，日志匹配逻辑对该状态直接 false；A 的正常命令误报跨树失败，且小文件被误称超过 4 MiB。 |
| AF-T04 | P2 | [crates/kanzei-tools/src/cross_tree.rs](tools.md) | create_dir_all 和 write 的错误全部丢弃，报告仍称已隔离且可取回；用户按照报告寻找恢复副本时实际不存在。 |
| AF-T05 | P2 | [crates/kanzei-tools/src/local_validation.rs](tools.md) | nearest_manifest 第一层因不在主树内即停止，误用 Rust 2015；合法 async fn 被错误报告语法失败，UI/模型收到错误修复建议。写入本身保留。 |
| AF-T06 | P1 | [crates/kanzei-tools/src/palette.rs](tools.md) | 错误头部偏移、块类型、名称位置及编码使标准 ASE 无法导入；既有 sample.ase 同样采用错误自造布局，掩盖协议不兼容。 |
| AF-T07 | P2 | [crates/kanzei-tools/src/grep.rs](tools.md) | count 的 Searcher 未启用 multiline，返回 no matches；同输入的普通检索命中，统计结果与检索结果矛盾。 |
| AF-T08 | P1 | [crates/kanzei-tools/src/symbols.rs](tools.md) | crate_dirs 从 ctx.project_root 建立，带 crate 查询直接扫主树；只带 module 时当前树文件无法匹配主树目录，可能得到空地图。返回的依赖/API 地图不代表当前工作树。 |
| AF-T09 | P1 | [crates/kanzei-tools/src/browser_tool/headless.rs](tools.md) | 全局 helper/page 无会话或工作树 owner，A 的后续操作落到 B 页面；共享 CURRENT_URLS 还把错误页面作为权限资源。 |
| AF-T10 | P1 | [crates/kanzei-tools/src/webfetch.rs](tools.md) | 读取达到上限后保存截断 PDF；PDF 分支不携带 truncated 信息，仍返回成功并让用户用 read pages 打开，缓存/来源标记也将其视为已抓取。 |
| AF-T11 | P0 | [crates/kanzei-tools/src/git/tool.rs](tools.md) | 工具声明的排他范围是 A 的 ctx.cwd，但 merge_ff 实际改 B 的 HEAD、index 和文件；A/B 两个内部 run 可同时写 B，绕过一树一写者语义。 |

## 逐文件报告与验证证据

- [root 逐文件报告](root.md)：44 文件；11 个根因。
- [conversation 逐文件报告](conversation.md)：49 文件；8 个根因。
- [automation 逐文件报告](automation.md)：38 文件；9 个根因。
- [tools 逐文件报告](tools.md)：50 文件；11 个根因。
- [完整问题、批次和源码指纹](summary.json)。四份主报告去重后与 manifest 的 181 文件精确一致，协审记录不重复计数。

验证包含真实 Git 管道对照、隔离 CLI 文件/HTTP、真实无头 Edge 会话、SQLite owner/历史、Rust 原函数和 Node UI 函数探针。各项明确区分原模块执行、边界桩化和静态推导。C7 使用已有有效证据并核对现实现未变；没有重跑共享 Cargo 或全桌面 E2E。

## 并行进展与范围处理

另一条用户任务在同一 checkout 修改需求关闭、工具结果分类和相关 UI。已检查与本轮重叠的差异：

- crates/kanzei-app/ui/05-chat-render.js
- crates/kanzei-app/ui/06-activity.js
- crates/kanzei-app/ui/26-project-conversations.js
- crates/kanzei-core/src/runner/tool_failure_telemetry.rs
- crates/kanzei-tools/src/read.rs
- crates/kanzei-tools/src/symbols.rs

AF-R06 已更新为当前影响：成功正文仍会产生错误的 missing_parameter 诊断，但对方已使它不再提高 failure_count。本报告不把旧影响重复算作当前未修问题。read/symbols 的结果分类变化没有修复当前树身份错误。其他重叠 UI 改动没有修复本轮重试、行号、截图等根因。旧审阅文件的并行新改动清单保存在 summary.json，后续整合前还需核对最终版本。

AUTO-008/009 只影响快速需求捕获与想法拆解，已从当前问题计数和修复计划删除，仅保留范围外观察。CLI 同步 stdin 候选未证明目标平台 Ctrl+C 失效，不计 bug。没有重新加入用户排除的功能专项。

# Module Summary

## 已修复
- P0: 0
- P1: 0
- P2: 0

## PASS 文件
- 本轮 138 个，完整清单见四份逐文件报告。PASS 表示在审阅合同与证据内未发现实际问题。

## 仍需人工判断
- 无需产品决策；按既有合同修复。

## 依赖影响
- 本轮未改 cross-module contract。修复需统一执行/资源 owner、版本归属、消息确认和失败提交语义，按 F1–F8 分批处理。

## 剩余风险
- 面板锁等待换线、部分注册并发与落库失败路径为静态可达交错，修复时需加受控调度/失败注入回归。
- 桌面交互、原生进程取消、跨进程集成尚需在修复后验证。
- 另一任务仍在编辑，最终整合必须复核其完成版本；当前报告只对应记录时点，未代为验证或发布对方改动。
