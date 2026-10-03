# Automation audit-first 全文审查

基线 `55eaca24750ac431ebe7f577c2f4b7b8cbded6f5`。本组 38/38 文件全文完成；另接审 worktree_tests.rs 的记录见 root-assist-automation.md/json，不重复计入本组。只写本报告及隔离探针，未修改产品、未提交、未运行共享 Cargo 或全量门禁。

## 依赖地图

`core notification/delegation/spec → MemoryCoordinator/phase → tools background(owner/registry/reader/monitor) 与 schedules(parse/slot/executor/host) → app process registry/naming/lifecycle → auto_run/PhasePipeline/subagents/collaboration → 子代理 UI/CLI`。

状态真源：后台 registry.json 负责重启发现、内存 BackgroundProcess 负责当前资源；定时 Markdown 定义 + SQLite slot/history；对话 state.db 保存模型和开关、runtime 持有运行状态；MemoryCoordinator 只仲裁同一实例 write_scope；AgentTeam store 拥有消息/任务终态，UI 仅投影。文件原子替换不能代替上层事务，主根 tracker 文件锁不能证明写入来自哪次调用。

## 本轮范围与计数

本轮保留 9 项修复候选（6 P1、3 P2）；AUTO-008/009 属于已排除的业务快速捕获/想法拆解，仅作为全文审阅附带的范围外观察，不纳入修复计划。未发现两项影响通用 task、AgentTeam 或 PhasePipeline 普通任务执行路径。

## 确认问题

### AUTO-001 P1 后台持久登记读改写丢更新

- 根因：`crates/kanzei-tools/src/background/persistent.rs`:38-60; registration.rs:144-158,269-287。
- 触发：同项目已有 A，启动 B 与 A 自然退出并发：两个路径分别读取旧 [A]，启动写 [A,B]，退出随后写 []。
- 影响：仍在运行的 B 从持久登记消失，应用重启后无法发现/接管；相反顺序也可复活已结束的登记。
- 证据：load_registry/save_registry/remove_registry_entry 没有覆盖读改写的锁；register 的持久登记先于内存 registry 锁。base/atomic_file.rs:3 明确原子替换不负责 caller 读改写串行化。自然退出 waiter 独立于新启动路径，时序可达。
- 验证：全文及 bash/register/exit/stop/discover caller 检查；明确给出可达交错，未运行整进程竞争测试。
- 状态：已确认，审计阶段未修复。

### AUTO-002 P1 后台登记读写失败被当成功

- 根因：`crates/kanzei-tools/src/background/persistent.rs`:38-50; registration.rs:144-158。
- 触发：registry.json 非法 JSON/UTF-8 或读取失败时仍启动新的 persistent 服务；或 registry 替换因占用/路径错误失败。
- 影响：旧登记可被空表覆盖；服务启动/清理返回成功但持久登记未更新，重启恢复与运行态分叉。
- 证据：load_registry 对所有 I/O 错误返回空、JSON 错误 unwrap_or_default；save_registry 丢弃 write_atomic Result。register、mark_registry_failed、kill_registered_result 均不能传播该失败。
- 验证：全文＋所有 registry 读写 caller 与 R-180 恢复合同检查；尚未做真实 IO 故障注入。
- 状态：已确认，审计阶段未修复。

### AUTO-003 P2 小输出长驻服务日志没有按两秒落盘

- 根因：`crates/kanzei-tools/src/background/registration.rs`:174-215。
- 触发：服务输出一次 READY（小于64KiB），随后一直运行且没有新输出。
- 影响：输出仅留内存；强杀应用后缺失该段日志，重启接管回看不完整。当前运行中的内存读取仍可见。
- 证据：elapsed>=2s 仅在 read 返回字节后检查，下一次 read 一直 pending 就永不触发；EOF 才最终 flush。
- 验证：隔离编译实际 reader/append 函数体，tokio duplex 输入 READY；确认内存已接收，2.2 秒日志不存在；EOF 后完整字节落盘为正对照。output/audit-first/automation/probe-results.json。
- 状态：已确认，审计阶段未修复。

### AUTO-004 P1 启停编辑不遵守自身定义解析规则

- 根因：`crates/kanzei-tools/src/schedules/mod.rs`:143-205,326-338。
- 触发：合法定义使用缩进 enabled 字段，或省略头部 enabled 且正文包含行首 enabled: 示例。
- 影响：前者 toggle 产生重复字段，合法操作被拒绝；后者启用只改正文，真实 enabled 仍 false。执行器自动禁用也可能写坏合法定义。
- 证据：parse 对字段 trim 且拒绝重复字段；set_enabled 在全文中只匹配行首 enabled:，未限 front matter/未处理合法缩进。app/schedules.rs:198-207、executor.rs:248 消费此函数。
- 验证：隔离编译当前 parse/scalar/valid_name/set_enabled/When::parse 实际函数：普通启停正对照通过，两种反例均复现；仅固定 local UTC offset，省略无关序列化 derive。
- 状态：已确认，审计阶段未修复。

### AUTO-005 P1 服务端 cron 更新误删名称有前缀关系的任务

- 根因：`crates/kanzei-tools/src/schedules/hosts.rs`:4-11,161,206。
- 触发：同项目在同一服务器登记 check 与 check-long，再保存、禁用或删除 check。
- 影响：check-long 的 cron 行也被删除，定义仍启用但不再被唤醒。
- 证据：task_name 为 Kanzei-<root hash>-<name>；grep -F -v marker 是子串过滤，check marker 必然匹配 check-long 行。两个名称都通过 valid_name。
- 验证：本地 Git grep 对真实格式两行输入运行同样 -F -v 参数，结果两行全被过滤；未连接 SSH 或操作任何 crontab。app/schedules host 与 CLI register caller 已核对。
- 状态：已确认，审计阶段未修复。

### AUTO-006 P1 对话设置落库失败后内存仍采用新设置

- 根因：`crates/kanzei-app/src/processes/lifecycle.rs`:716-748。
- 触发：process_update 改模型/权限开关后，state.db 打开或 upsert 失败（例如数据库被占用或磁盘写入错误）。
- 影响：RPC 报失败，但后续运行读到已经改变的模型/子代理/tracker 开关；重启又恢复旧值。
- 证据：process_update 先修改共享 Arc<Mutex>/AtomicBool，再 persist_process(...)?；错误分支没有回滚。registry.rs 的 persist_process/open/upsert 是实际 fallible 调用。
- 验证：完整审阅生命周期及 registry 落库/恢复、运行时设置读取合同；仅确认失败顺序，不宣称并发 RPC lost update 或已执行数据库注入。
- 状态：已确认，审计阶段未修复。

### AUTO-007 P1 子任务收件箱确认了未送入模型的消息

- 根因：`crates/kanzei-tools/src/team/tools.rs`:50-55。
- 触发：子任务运行中、下一次模型请求前积累13条补充消息；或 store.get 后、update 前又收到新消息。
- 影响：未出现在模型上下文里的消息也变 received，轮末被标 processed，后续 worker 不会再以 queued 消费，用户方向可丢失。
- 证据：注入只取快照最后12条，但 update 遍历当前全部 queued；team/mod.rs:877-899 消息不限12条、:1316-1324 仅处理 queued、:1543-1546 将全部 received 标 processed。初次启动的 queued prompt 不兜底运行中后来到达的消息。
- 验证：逐段审阅 inbox 注入与 queue_message/worker loop/terminal caller，确认运行中送消息入口可达；未运行模型/真实 IPC 队列探针。
- 状态：已确认，审计阶段未修复。

### AUTO-010 P2 指定勘察模型后沿用另一个模型的上下文上限

- 根因：`crates/kanzei-app/src/phase_pipeline.rs`:199-225,626-657。
- 触发：配置 models.scout 指向 context_limit 与 fast/primary 不同的 provider。
- 影响：勘察按旧窗口做上下文预算，可提前压缩/拒绝，或向较小窗口模型发送过大的请求。未指定 scout 时不受影响。
- 证据：ScoutRoute 只保存 route/model/service_tier；runtime_as 克隆模板后替换两档 route，不替换 options.fast_context_limit/primary_context_limit；core runner/subagent.rs:579-583 正是读取这两项。
- 验证：完整审阅路由与 runner options caller；尚未执行不同窗口的 provider mock。
- 状态：已确认，审计阶段未修复。

### AUTO-011 P2 并发工具尚在执行却报告没有当前工具

- 根因：`crates/kanzei-app/src/collaboration.rs`:290-300。
- 触发：A start、B start、B completed，而 A 仍运行（允许并行的只读工具批次）。
- 影响：协作快照/模型协作上下文显示 current_tool=None，其他线路看不到仍在进行的工具。仅影响当前活动信息。
- 证据：current_tool 只找最后一条 started/completed；最后是 completed 就无条件 None，未按 call id 配对还未结束的调用。
- 验证：全文审阅快照构造与工具并发语义；精确三事件序列即可推导，未运行 UI E2E。
- 状态：已确认，审计阶段未修复。

## crates/kanzei-app/src/agent_directory.rs

### 职责
读取并合并内置与项目子代理目录配置。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- agent catalog/派发选择。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对名称解析、缺省值及项目覆盖边界。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/auto_run.rs

### 职责
计算自动运行下一步及同步工作项状态。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- run finish/自动推进与 tracker。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对运行结论、用户等待、重复目标及 terminal gate。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/collaboration.rs

### 职责
汇总其他线路的实际活动、工作树与修改文件。

### 判断
P2

### 确切问题
- AUTO-011：并发工具尚在执行却报告没有当前工具。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- 协作快照 IPC/协作 context 工具。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对进程过滤、文件 owner、Git 失败与当前工具配对。 AUTO-011：全文审阅快照构造与工具并发语义；精确三事件序列即可推导，未运行 UI E2E。

## crates/kanzei-app/src/orchestration_trace.rs

### 职责
将编排生命周期观察事件转为可回放轨迹。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- MemoryCoordinator/PhasePipeline observer。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对观察者失败不改变执行状态、阶段事件映射。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/phase_pipeline_tests.rs

### 职责
验证阶段、读槽、写租约和复核修正流程。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- phase_pipeline/run_review_and_fixup。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对mock 请求、超时/失败终态、写租约释放与复验结果断言。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/phase_pipeline.rs

### 职责
协调勘察、实现、复核、修正各阶段的权限和模型运行时。

### 判断
P2

### 确切问题
- AUTO-010：指定勘察模型后沿用另一个模型的上下文上限。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- run setup/execution/core subagent。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对阶段转换、writer/read owner、取消与 scout 路由。 AUTO-010：完整审阅路由与 runner options caller；尚未执行不同窗口的 provider mock。

## crates/kanzei-app/src/phase_pipeline/prompts.rs

### 职责
渲染勘察/复核/修正输入和实际条目上下文。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- PhasePipeline。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对无条目/空发现/失败结果及只读职责提示。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/process_tests.rs

### 职责
验证对话设置、持久字段恢复与默认值。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- process register/restore。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对实际 store 往返和不同开关互不误覆盖。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/processes/conversation_tests.rs

### 职责
验证关闭会话后重启不会复活及按需创建。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- process registry/lifecycle。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对最后会话关闭、列举只读、显式创建合同。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/processes/lifecycle.rs

### 职责
管理对话建立、更新、关闭与资源回收。

### 判断
P1

### 确切问题
- AUTO-006：对话设置落库失败后内存仍采用新设置。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- process IPC/run/worktree。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对预检、落库、运行停止、prefs 失败、收尾顺序。 AUTO-006：完整审阅生命周期及 registry 落库/恢复、运行时设置读取合同；仅确认失败顺序，不宣称并发 RPC lost update 或已执行数据库注入。

## crates/kanzei-app/src/processes/mod.rs

### 职责
声明 process 子模块与既有接口再导出。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- app commands/state。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对再导出与唯一实现边界。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/processes/naming.rs

### 职责
分配和渲染对话名称、序号与持久命名元数据。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- process_info/register/conversation views。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对序号下界、退休 ID、名称覆盖与数据库失败。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/processes/registry.rs

### 职责
注册/恢复对话元数据并维护一树一线身份。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- lifecycle/state/process_list。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对内存/库所有权、退休 ID、失效工作树、持久字段恢复。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/subagents_capture_tests.rs

### 职责
验证快速捕获在 provider 失败后的回退与真实收据。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- quick_req。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对真实 mock 响应、一次创建、错误后已有写入。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/src/subagents.rs

### 职责
执行快速捕获、想法拆解与只读缺陷审查。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题（指本轮四块通用系统合同）。
- 全文审阅顺带发现 AUTO-008/009，限业务捕获/想法拆解，详见范围外观察；不进入本轮修复计数。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- quick_req/idea_split IPC、tracker。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对条目收据所有权、写租约、终态前置条件及 fallback。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/ui/05-subagents.js

### 职责
拥有子代理运行模型并统一实时/回放/卡片状态。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- 07-events/15-views/agent panel/27-agent-team。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对session+id、重派历史、终态粘滞、managed 生命周期、取消 owner。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-app/ui/27-agent-team.js

### 职责
提供子任务消息、差异检查、采纳与重新派发 UI。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- 05-subagents/agent_team_command。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对owner 校验、操作等待、head 审阅、消息回执。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-core/src/notification.rs

### 职责
定义子代理通知数据合同。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- 后台任务通知 sink。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对序列化字段和状态含义。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-core/src/orchestration.rs

### 职责
持有项目 writer 队列与并行 reader 槽。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- phase/app/team coordinator。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对锁序、oneshot 交接、取消排队、RAII 和观察者锁外执行。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-core/src/phase.rs

### 职责
管理阶段状态与并发角色屏障结果。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- PhasePipeline/observer。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对状态转换、屏障超时、重复 terminal 与 Drop 中止。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-core/src/runner/delegation.rs

### 职责
按 host 与任务动作选择委派执行路线。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- runner drive/task tool。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对托管 host 优先级、兼容输入及 action 分类。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-core/src/runner/subagent.rs

### 职责
执行只读/兼容子代理、取消、配额和 transcript。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- task drive/PhasePipeline/AgentTeam。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对host分流、模型选择、只读槽、timeout/cancel、历史恢复与稳定错误码。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-core/src/runner/subagent/spec.rs

### 职责
构造可见任务工具的输入 schema。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- runner subagent tool注册。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对roster 选项、可选字段和实际解析合同。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-tools/src/background.rs

### 职责
定义后台进程、完成收据与对外门面，包含回归测试。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- BashTool/ProcessTool/monitor。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对资源 owner、退出收据、guard 等待与持久恢复调用。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-tools/src/background/monitor.rs

### 职责
订阅后台日志并向会话邮箱发送匹配通知。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- ProcessTool/terminal_monitor。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对订阅 owner、游标、取消、配额、替换旧订阅的 Arc 身份。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-tools/src/background/persistent.rs

### 职责
管理跨运行后台服务登记、发现、接管与清理。

### 判断
P1

### 确切问题
- AUTO-001：后台持久登记读改写丢更新。
- AUTO-002：后台登记读写失败被当成功。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- registration/ProcessTool/lifecycle。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对磁盘 owner、RMW、读写失败与恢复一致性。 AUTO-001：全文及 bash/register/exit/stop/discover caller 检查；明确给出可达交错，未运行整进程竞争测试。 AUTO-002：全文＋所有 registry 读写 caller 与 R-180 恢复合同检查；尚未做真实 IO 故障注入。

## crates/kanzei-tools/src/background/registration.rs

### 职责
发布后台进程并收集输出、等待退出及追加日志。

### 判断
P1

### 确切问题
- AUTO-001：后台持久登记读改写丢更新。
- AUTO-002：后台登记读写失败被当成功。
- AUTO-003：小输出长驻服务日志没有按两秒落盘。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- BashTool/background registry。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对发布顺序、reader drain、日志持久化与退出清理。 AUTO-001：全文及 bash/register/exit/stop/discover caller 检查；明确给出可达交错，未运行整进程竞争测试。 AUTO-002：全文＋所有 registry 读写 caller 与 R-180 恢复合同检查；尚未做真实 IO 故障注入。 AUTO-003：隔离编译实际 reader/append 函数体，tokio duplex 输入 READY；确认内存已接收，2.2 秒日志不存在；EOF 后完整字节落盘为正对照。output/audit-first/automation/probe-results.json。

## crates/kanzei-tools/src/schedules/executor.rs

### 职责
按定义执行定时任务、记录结果并回写。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- app schedules/CLI schedule。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对slot claim、命令超时、文件边界、终态回执与自动禁用。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-tools/src/schedules/hosts.rs

### 职责
显式登记系统/服务器定时唤醒并导入远端收据。

### 判断
P1

### 确切问题
- AUTO-005：服务端 cron 更新误删名称有前缀关系的任务。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- app schedules/CLI schedule。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对任务身份、host 失败、导入认领及命令引用。 AUTO-005：本地 Git grep 对真实格式两行输入运行同样 -F -v 参数，结果两行全被过滤；未连接 SSH 或操作任何 crontab。app/schedules host 与 CLI register caller 已核对。

## crates/kanzei-tools/src/schedules/mod.rs

### 职责
拥有定时定义格式、校验、加载、时间槽认领与启停编辑。

### 判断
P1

### 确切问题
- AUTO-004：启停编辑不遵守自身定义解析规则。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- app schedules/executor/CLI。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对格式与编辑一致性、slot/SQLite claim及诊断。 AUTO-004：隔离编译当前 parse/scalar/valid_name/set_enabled/When::parse 实际函数：普通启停正对照通过，两种反例均复现；仅固定 local UTC offset，省略无关序列化 derive。

## crates/kanzei-tools/src/schedules/when.rs

### 职责
解析可支持的频率并计算最近/下一时间槽。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- schedules parse/slot。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对合法间隔、周日/工作日、时区与边界分钟。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-tools/src/subagent.rs

### 职责
组装只读与可写子代理工具快照和权限。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- core subagent/app pipeline/AgentTeam。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对工具可达能力、权限缺省和只读白名单。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei-tools/src/team/tools.rs

### 职责
向托管子任务注入消息、记忆与限定工具。

### 判断
P1

### 确切问题
- AUTO-007：子任务收件箱确认了未送入模型的消息。

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- AgentTeam worker harness。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对消息接收语义、实际 tree/owner、工具包装。 AUTO-007：逐段审阅 inbox 注入与 queue_message/worker loop/terminal caller，确认运行中送消息入口可达；未运行模型/真实 IPC 队列探针。

## crates/kanzei/src/cli/schedule.rs

### 职责
提供定时定义检查、运行、注册及导入 CLI。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- kz CLI/schedules公共API。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对参数错误、注册路由与失败退出。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei/tests/integration/background_subagent_dispatch.rs

### 职责
验证后台派发、真实结果、通知、历史与取消恢复。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- core runner background/transcript。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对后台占位与实际结果区分、failure/timeout读槽清理。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei/tests/integration/max_tasks_parallel_dispatch.rs

### 职责
验证任务并发额度内执行与超额拒绝。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- runner task batching/coordinator。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对20个实际子任务、21号唯一溢出与读槽成对。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei/tests/integration/parallel_scouting_under_serial_writer.rs

### 职责
验证 writer 策略下只读子任务可并发。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- runner readonly task route。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对只读工具白名单、writer不释放、reader重叠及回收。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

## crates/kanzei/tests/integration/task_cancel_parallel.rs

### 职责
验证只停单任务时主轮继续。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题

### 修改
- 未改，审计阶段；先统一根因与相邻 caller 合同，再进入修复。

### 影响范围
- TaskCancellations/run_subagent。当前未改变 API、持久格式或兼容性。

### 验证
- 全文（含现有测试）审阅；核对取消码、terminal、reader RAII、主轮收尾。 本轮未执行此文件相关完整测试，不将静态 PASS 写成运行验收通过。

# Module Summary

## 已修复
- P0: 0
- P1: 0（已确认待修 6 项）
- P2: 0（已确认待修 3 项）

## PASS 文件
- `crates/kanzei-app/src/agent_directory.rs`
- `crates/kanzei-app/src/auto_run.rs`
- `crates/kanzei-app/src/orchestration_trace.rs`
- `crates/kanzei-app/src/phase_pipeline_tests.rs`
- `crates/kanzei-app/src/phase_pipeline/prompts.rs`
- `crates/kanzei-app/src/process_tests.rs`
- `crates/kanzei-app/src/processes/conversation_tests.rs`
- `crates/kanzei-app/src/processes/mod.rs`
- `crates/kanzei-app/src/processes/naming.rs`
- `crates/kanzei-app/src/processes/registry.rs`
- `crates/kanzei-app/src/subagents_capture_tests.rs`
- `crates/kanzei-app/src/subagents.rs`
- `crates/kanzei-app/ui/05-subagents.js`
- `crates/kanzei-app/ui/27-agent-team.js`
- `crates/kanzei-core/src/notification.rs`
- `crates/kanzei-core/src/orchestration.rs`
- `crates/kanzei-core/src/phase.rs`
- `crates/kanzei-core/src/runner/delegation.rs`
- `crates/kanzei-core/src/runner/subagent.rs`
- `crates/kanzei-core/src/runner/subagent/spec.rs`
- `crates/kanzei-tools/src/background.rs`
- `crates/kanzei-tools/src/background/monitor.rs`
- `crates/kanzei-tools/src/schedules/executor.rs`
- `crates/kanzei-tools/src/schedules/when.rs`
- `crates/kanzei-tools/src/subagent.rs`
- `crates/kanzei/src/cli/schedule.rs`
- `crates/kanzei/tests/integration/background_subagent_dispatch.rs`
- `crates/kanzei/tests/integration/max_tasks_parallel_dispatch.rs`
- `crates/kanzei/tests/integration/parallel_scouting_under_serial_writer.rs`
- `crates/kanzei/tests/integration/task_cancel_parallel.rs`

## 仍需人工判断
- 无需产品决策的问题；以上均为现有合同的正确性修复。

## 依赖影响
- 尚未修改 cross-module contract。后续优先统一 persistent registry 事务及 Result 传播，再改 registration/BashTool/ProcessTool/lifecycle caller。
- schedule 原文编辑应与 parse 共用字段边界；host cron 必须精确匹配任务身份。
- team 收件确认应限定实际快照中送达的消息；业务捕获/拆解不进入本轮修复。
- process_update 应先取得成功持久收据再发布设置；scout 模型须携带自己的 context limit。

## 剩余风险
- 未运行共享 Cargo、全套 Rust 测试或真实桌面 E2E；本报告不替代修复后的编译与调用方回归。
- registry 竞争/故障、process_update 数据库失败、team 运行中消息已按当前可达代码路径确认，但未执行完整集成故障探针。
- SSH cron 仅以相同 grep 参数作本地过滤复现，没有修改远端。

## 历史依据与排除项
- background R-180（跨 run 恢复/日志）、R-246（接管现有生命周期）；交叉核对 audit-a0/parallel/B6-map.md 的3项旧候选，本报告为当前源码独立确认，未沿用其测试声明。
- D-725 明确同名复审角色结束后新建卡；05-subagents:401 已处理此设计，不把 run_id:role 重复本身报成 BUG。
- 旧 SubagentChangeLog 在实际生产装配均为 None，未把未接线旧路径计入问题；未把哈希理论碰撞、缺测试、代码风格列为缺陷。
- 后台测试 fence_guard 生命周期疑点未证明会影响当前隔离夹具，不列确认缺陷。

## 范围外观察（不进入本轮修复计数）

仅影响 quick_req/idea_split 的业务条目捕获与想法拆解；未发现通用 task/AgentTeam/PhasePipeline 普通任务路径消费这些收据或前置状态。focused-scope.json 将 docs/tracker 业务列为仅接口参考或排除范围；subagents.rs 全文纳入不恢复该业务专项。仅保留审阅观察，不进入本轮修复计划或计数，无需新增产品决策。

### AUTO-008 P1 捕获与拆解把别的写入当成本次结果

- 根因：`crates/kanzei-app/src/subagents.rs`:101-106,194-210,307-317,378-415。
- 触发：quick_req/idea_split 等模型返回期间，UI docs_update(add) 或其他工作树 tracker 在同一个主根新增条目；本次模型失败或新增别的条目。
- 影响：quick_req 可返回别人的 ID 并跳过自己的 fallback；idea_split 可把无关新增项写进 idea.refs，产生错误依赖。
- 证据：用全量 before/after ID 差集推断本次收据；docs.rs:509-581 直接 TrackerTool.execute，不取 MemoryCoordinator writer；worktree_tests.rs:933-999 证明分支工具写同一 project_root。文件级锁只串行单次修改，不能证明条目属于本次 LLM。
- 验证：完整阅读 subagents 捕获/拆解及现有测试，检查 docs_update 与主根 tracker caller；所有权交错已静态证明，未运行双入口端到端探针。
- 范围：仅影响 quick_req/idea_split 的业务条目捕获与想法拆解；未发现通用 task/AgentTeam/PhasePipeline 普通任务路径消费这些收据或前置状态。focused-scope.json 将 docs/tracker 业务列为仅接口参考或排除范围；subagents.rs 全文纳入不恢复该业务专项。仅保留审阅观察，不进入本轮修复计划或计数，无需新增产品决策。
- 状态：范围外观察；未修复，不自动排入计划。

### AUTO-009 P1 想法拆解排队后使用旧的 inbox 前置状态

- 根因：`crates/kanzei-app/src/subagents.rs`:255-278,402-420。
- 触发：拆解调用在 writer 租约排队时，另一次拆解完成，或 docs_update 把想法转 dropped；之后该调用获取租约继续执行。
- 影响：本应拒绝的终态想法仍调用模型、新增需求/缺陷；最终转 split 即使被 tracker 状态门禁拒绝，也已经留下不该创建的条目。
- 证据：inbox 检查在 .acquire_writer_lease().await 之前；获得租约后未重读，后续产生条目才调用 update 转 split。
- 验证：完整阅读状态检查、租约等待、最终 refs 更新和 docs_update 状态入口；以实际异步等待边界证明 TOCTOU，未执行并发拆解探针。
- 范围：仅影响 quick_req/idea_split 的业务条目捕获与想法拆解；未发现通用 task/AgentTeam/PhasePipeline 普通任务路径消费这些收据或前置状态。focused-scope.json 将 docs/tracker 业务列为仅接口参考或排除范围；subagents.rs 全文纳入不恢复该业务专项。仅保留审阅观察，不进入本轮修复计划或计数，无需新增产品决策。
- 状态：范围外观察；未修复，不自动排入计划。
