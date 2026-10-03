# 对话执行与恢复：先审后改审计

基线 `55eaca24750ac431ebe7f577c2f4b7b8cbded6f5`。分配的 **49/49 文件已逐行全文审读**，并与基线逐文件 LF 内容比对。只写审计材料和隔离探针，产品代码、持久格式、schema、API 均未修改。

确认 **P0 0 / P1 6 / P2 2**；均待统一修复，C7 的三个既有问题计入总数但不重复作为新发现。

## 依赖地图与所有权

底层 base 锁/路径/hash → core store(session/events/checkpoint/rewind) → core context/compaction/metrics/drive → app runtime/coordinator/events → commands 与 CLI → UI。

- 持久事实单一真源：SessionStore 的 typed facts、输入队列和完成的 compaction；内存 conversation/live 只是缓存/运行视图。
- 跨进程会话互斥：session_execution owner；进程内状态互斥：SessionRuntime.lifecycle。两者不可相互替代。
- 文件持久边界：checkpoint blob 与逐路径 CAS；交付窗口必须独立维护本批真实写入归属。
- 事件合同：typed ACK 是当前写入返回值；errors 是历史诊断；run.trace 是增量行，消费者要按 run 汇合。
- 设计参照：D-342 协作停止、D-773 batch 交付窗口、R-242 typed projection、R-163 replay；原 C7 证据只读复用。

## 确认问题

### CONV-AF-01 · P1 · 插话批次在多个事务中部分完成，却整体返回失败

- 根因 owner：[crates/kanzei-app/src/run/coordinator.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/run/coordinator.rs>)，位置 `118-144`。
- 触发：A/B/C 同批插话，第二条 typed append 或 input finish 被 SQLite 拒绝。
- 影响：较早输入已 completed、事实已提交，但 Result::Err 丢掉整批 notices；剩余 promoted 输入在恢复中变 failed，provider 没收到对应输入。
- 验证：复用 C7 真实 SQLite trigger + HTTP runner 负例 SecondSteer/InputFinish；本轮核对当前闭包仍逐条提交，未重跑 Cargo。
- 状态：未修，审计阶段。

### CONV-AF-02 · P1 · 历史 writer errors 被误用为当前插话 ACK

- 根因 owner：[crates/kanzei-app/src/run/coordinator.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/run/coordinator.rs>)，位置 `124-134`。
- 触发：旧 draft SQL 写失败后已成功重试；writer.errors 保留诊断；随后合法插话提交成功。
- 影响：成功插话被判失败，整轮终止，合法插话不能正常喂给 provider。
- 验证：C7 RecoveredDraft 真实负例；当前源码仍以 !writer.errors().is_empty() 判此次失败。
- 状态：未修，审计阶段。

### CONV-AF-03 · P1 · 停止后旧 runner 通过空 halt 槽误收新插话

- 根因 owner：[crates/kanzei-app/src/run/coordinator.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/run/coordinator.rs>)，位置 `104-116`。
- 触发：旧 runner 到 inbox 安全点前 Stop 先取得 lifecycle 并 take/cancel 旧令牌；用户随后加入插话；旧 runner 再进入 inbox。
- 影响：共享 halt 槽为 None 被当作未停止，旧 run 消费新消息、标 completed 并发下一条 provider 请求。
- 验证：C7 Stop 负例证明 own token cancelled + slot empty + 第二 POST；本轮核对当前调度/停止合同。
- 状态：未修，审计阶段。

### CONV-AF-04 · P1 · 历史变更绕过跨进程 session 执行 owner

- 根因 owner：[crates/kanzei-app/src/conversation.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/conversation.rs>)，位置 `31-63,546-560`。
- 触发：CLI 或另一桌面进程正持有相同数据库/session 的执行 owner；本桌面 runtime.running 为 false，执行 clear/delete/rewind。
- 影响：允许在活跃 writer 脚下切段或删除 typed facts；旧轮继续写入会跨段或因历史被删而拒绝，已提交输入可从历史消失。
- 验证：独立 rustc 探针使用现有 core rlib/真实 SQLite：第二 owner 被拒，reset/delete 仍成功，活跃 facts 2→0。相关 owner/events/replay 最后源码提交 746b69a8 21:11:38，rlib 21:12:07；当前源码逐段一致性复核。 未启动两个真实桌面进程；IPC 本地 guard 到公共存储写入口已静态追通。
- 状态：未修，审计阶段。

### CONV-AF-05 · P1 · 无关工具观察把外部修改重新归属为本批

- 根因 owner：[crates/kanzei-core/src/runner/drive/batch.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/runner/drive/batch.rs>)，位置 `156-168,245-260`。
- 触发：本批已写 own.rs；用户编辑器修改 own.rs；随后工具只 read other.rs；observe 仍刷新 own.rs.after。
- 影响：owned 保持 true，新 after 吸收外部内容；closing stage/finalize 闸从拒绝变允许，混合改动可被当成本批提交。
- 验证：从当前 batch.rs 原样提取全部生产实现，用有效 owned 前置状态和真实文件重现：拒绝 true→false，owned=true。只增加探针函数，未改生产控制流。
- 状态：未修，审计阶段。

### CONV-AF-07 · P1 · 自动 push 没有等待上限，也不参与本轮取消

- 根因 owner：[crates/kanzei-app/src/run/mod.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/run/mod.rs>)，位置 `72-91`。
- 触发：成功 commit 后自动 push 遇到不结束的认证、远端传输或 pre-push hook。
- 影响：finalize_round 尚未执行，运行与写租约被长期占住；Stop 的 30秒 watchdog 只 abort Future，Command 未 kill_on_drop，子 git 可以在界面已停止后继续完成远端写入。
- 验证：静态追踪 Command.output().await 无 timeout/取消/kill_on_drop，到 finalize_round 与 watchdog；未访问真实远端，未做原生子进程树取消实测。
- 状态：未修，审计阶段。

### CONV-AF-08 · P2 · 测试冗余提醒使用写入前的陈旧 Git 指纹

- 根因 owner：[crates/kanzei-core/src/runner/redundancy.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/runner/redundancy.rs>)，位置 `63-105`。
- 触发：git status → 全量测试成功 → edit/write → 再次全量测试；中间不再刷新 Git 查询。
- 影响：把必要回归测试说成工作树无变化、这次可省；错误文本进入模型上下文，并污染 redundant_test 指标。
- 验证：当前 redundancy.rs 生产代码原样提取并执行，四步结果直接出现错误提醒；工具仍真实执行，所以定 P2。
- 状态：未修，审计阶段。

### CONV-AF-09 · P2 · 回放解析要求整包配对，但运行侧已逐事件落库

- 根因 owner：[crates/kanzei-core/src/replay.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/replay.rs>)，位置 `58-101`。
- 触发：当前桌面正常成功持久化 tool.started 和 tool.completed，形成两个 partial run.trace 行。
- 影响：replay-eval 对每行独立解析，pending 不跨行，所有步骤都为 0；真实失败轨迹被判无可回放案例。整包兜底中 TaskProgress 缺 kind 还会使整包 None；并行完成顺序也偏离声明的调用顺序。
- 验证：真实 core parser 分别解析当前写入形状，start/end 均 steps=0；静态追到唯一产品 caller CLI replay_eval。后两细节为同一输入契约修复范围，不重复计根因。
- 状态：未修，审计阶段。

## 文件记录

## crates/kanzei-core/src/store/session.rs

### 职责
管理 SQLite 会话、迁移、数据库路径和存储清理。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- SessionStore::open/create_session/cleanup_storage → app、CLI、store 子模块。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 1899 行；LF SHA-256 见 JSON，内容与基线一致。
- 检查 Immediate 事务、WAL、备份失败、GC 引用复核和 identity，现有清理测试全文审读。

## crates/kanzei-core/src/store/events.rs

### 职责
拥有事件追加、序号、压缩事务与对话段删除。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- append_event/append_compaction_transaction/delete_conversation_segment → conversation、writer、CLI。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 1602 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对序号不复用、先审计后删除、压缩 CAS、pending 输入保留；跨进程历史修改缺口归入口 AF-04。

## crates/kanzei-core/src/store/file_checkpoints.rs

### 职责
记录每轮文件首触前镜像和可恢复 blob。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- capture/checkpoint helpers → edit/write 与 rewind。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 677 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 run/path 唯一键、大小边界、不可恢复哨兵、blob hash、写前失败语义。

## crates/kanzei-core/src/store/rewind.rs

### 职责
依据对话事实生成回退计划并恢复选中文件。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- rewind_plan/apply_rewind → conversation_action。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 571 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 source hash、路径归一化、文件 CAS、blob 校验、force 备份及部分失败报告；上层互斥缺口归 AF-04。

## crates/kanzei-core/src/assemble.rs

### 职责
按配置装配模型路由及鉴权。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- build_route → tools/run、app、CLI。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 99 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 route/proxy/auth 输入和错误传播；不扩展已放弃的模型支持范围。

## crates/kanzei-core/src/runner/context.rs

### 职责
维护模型上下文预算、token 估计及安全历史过滤。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- context helpers → drive、compaction。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 812 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对真实 usage 校准、预算不变量、工具调用/结果成对过滤及边界测试。

## crates/kanzei-core/src/runner/compaction.rs

### 职责
压缩历史并管理压缩归档与溢出恢复。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- prune/compact/overflow helpers → drive、app/CLI finalize。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 1321 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对摘要超时、二阶段恢复、当前用户保留、无收益回滚、原始证据归档。

## crates/kanzei-core/src/runner/metrics.rs

### 职责
从本轮消息归纳工具、失败与完成条目指标。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- summarize_metrics/summarize_tools → app/CLI finalize、memory。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 897 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 ToolOutcome 分类、工具结果匹配、completed entry 先动作后完成；未把启发式本身当 bug。

## crates/kanzei-core/src/runner/redundancy.rs

### 职责
在工具结果中加入本轮冗余提醒。

### 判断
P2

### 确切问题
- CONV-AF-08：测试冗余提醒使用写入前的陈旧 Git 指纹。把必要回归测试说成工作树无变化、这次可省；错误文本进入模型上下文，并污染 redundant_test 指标。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- RedundancyWatch::note_step → drive 结果回喂。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 419 行；LF SHA-256 见 JSON，内容与基线一致。
- 执行 AF-08 原样源码四步探针，确认陈旧指纹。

## crates/kanzei-core/src/replay.rs

### 职责
将持久运行轨迹转为六臂回放案例及统计。

### 判断
P2

### 确切问题
- CONV-AF-09：回放解析要求整包配对，但运行侧已逐事件落库。replay-eval 对每行独立解析，pending 不跨行，所有步骤都为 0；真实失败轨迹被判无可回放案例。整包兜底中 TaskProgress 缺 kind 还会使整包 None；并行完成顺序也偏离声明的调用顺序。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- parse_trace_payload/run_arms → cli/eval、memory replay。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 861 行；LF SHA-256 见 JSON，内容与基线一致。
- 执行 AF-09 current-shape parser 探针；核对回放不调用真实工具。

## crates/kanzei-core/src/runner/line_runtime.rs

### 职责
按线保存取消、子任务和生命周期资源句柄。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- LineRuntime → app/runner/subagent。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 356 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 Drop/取消资源所有权；后台 process 占位方法只有测试 caller，实际进程由 tools registry 管，未记虚构泄漏。

## crates/kanzei-core/src/runner/mod.rs

### 职责
定义 runner 公共配置、事件和运行入口。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- run_once/RunEvent/RunnerConfig → app、CLI、integration tests。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 460 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对入口参数、round_messages、receipt/终态约定及子模块职责，不调整 API。

## crates/kanzei-core/src/runner/drive/assembly.rs

### 职责
装配单轮 system/context、工具目录和 prior。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- assembly helpers → drive。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 248 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 hints/scout 仅进 system、readonly 权限、工具与 agent 步数边界。

## crates/kanzei-core/src/runner/drive/context_budget.rs

### 职责
在 provider 调用前执行预算预警和压缩。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- context budget helpers → drive。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 135 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 token 基线、预压缩阈值、overflow 与当前输入保留。

## crates/kanzei-core/src/runner/drive/halt.rs

### 职责
实现 runner 协作式取消检查点。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- halt helpers → drive。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 28 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对取消令牌与 halted_by_user 正常返回合同；CLI 阻塞源归 AF-06。

## crates/kanzei-core/src/runner/drive/permissions.rs

### 职责
把规则、会话授权和 ASK 转为工具门禁结果。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- resolve_permission_gate → drive 串行工具执行。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 129 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 normalize action、deny 顺序、noninteractive、ASK 结果/PermissionResolved；同步回调问题归 CLI 实现。

## crates/kanzei-core/src/runner/drive/question.rs

### 职责
执行交互问题与自主决定记录。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- question helpers → drive。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 311 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对答案、取消、多选、自动决策及 missing fact 的分流。

## crates/kanzei-core/src/runner/drive/task_results.rs

### 职责
将子任务输出转换为 runner 工具结果。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- task result helpers → drive。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 35 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对错误/成功与 call id 配对，不存在第二份持久任务状态。

## crates/kanzei-core/src/runner/drive/batch.rs

### 职责
维护长轮次交付窗口、文件归属及恢复 checkpoint。

### 判断
P1

### 确切问题
- CONV-AF-05：无关工具观察把外部修改重新归属为本批。owned 保持 true，新 after 吸收外部内容；closing stage/finalize 闸从拒绝变允许，混合改动可被当成本批提交。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- before_calls/observe/reject → drive；git stage/finalize caller。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 570 行；LF SHA-256 见 JSON，内容与基线一致。
- 执行 AF-05 原样源码外部编辑探针，并与 tools 审计核对 Git 层没有 ownership 兜底。

## crates/kanzei-tools/src/run.rs

### 职责
共享桌面与 CLI 的 harness、runner 和子代理装配。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- build_harness/build_runner_config/build_subagent_runtime → app/CLI。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 235 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 profile 顺序、配置优先级、digest fallback、取消/只读子代理注入。

## crates/kanzei-app/src/projection_gate.rs

### 职责
定义 typed projection 的兼容开关。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- gate_enabled → conversation、coordinator、subagent。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 84 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对白名单与默认启用路径；回滚开关为明确设计而非重复 source of truth。

## crates/kanzei-app/src/typed_events.rs

### 职责
把运行事件映射到 typed writer。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- typed event handler → run/events。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 113 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 delta、commit 与终态职责，保证投影消费持久事实。

## crates/kanzei-app/src/state.rs

### 职责
拥有本进程 SessionRuntime、实时 trace 与 stop 生命周期。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- runtime_for/stop_runtime_and_finalize/record_live_trace → app commands、coordinator。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 1043 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 lifecycle 锁顺序、代次 watchdog、共享槽 take；AF-03 根因是 caller 误把共享槽当本轮身份。trace 失败补写重复候选未确认用户影响，不单列 bug。

## crates/kanzei-app/src/state_tests.rs

### 职责
验证 runtime 停止、身份、回收和 trace 状态。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- test-only → state。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 757 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对测试前提与断言、stale watchdog 不停新代次、持久恢复边界；本轮未重跑。

## crates/kanzei-app/src/permission_tests.rs

### 职责
验证权限 IPC 和规则持久化语义。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- test-only → permission commands。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 194 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对 always/once/deny、保存失败路径和规则范围，未把缺测试单列问题。

## crates/kanzei-app/src/run/assembly/components.rs

### 职责
装配桌面专用权限和 tracker 边界。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- components → run assembly。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 96 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 tail hard deny、readonly/discussion 和 tracker 写入策略顺序。

## crates/kanzei-app/src/run/baseline.rs

### 职责
有界获取 Git 起始状态供运行画像和差异归因。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- capture baseline → coordinator/assembly。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 434 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 timeout、kill_on_drop、输出限额、rename 解析、HEAD 一致性及 unknown 语义。

## crates/kanzei-app/src/run/coordinator.rs

### 职责
协调输入、runner 执行、持久结果和轮末收尾。

### 判断
P1

### 确切问题
- CONV-AF-01：插话批次在多个事务中部分完成，却整体返回失败。较早输入已 completed、事实已提交，但 Result::Err 丢掉整批 notices；剩余 promoted 输入在恢复中变 failed，provider 没收到对应输入。
- CONV-AF-02：历史 writer errors 被误用为当前插话 ACK。成功插话被判失败，整轮终止，合法插话不能正常喂给 provider。
- CONV-AF-03：停止后旧 runner 通过空 halt 槽误收新插话。共享 halt 槽为 None 被当作未停止，旧 run 消费新消息、标 completed 并发下一条 provider 请求。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- run_task → commands/run；InputInbox → runner。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 763 行；LF SHA-256 见 JSON，内容与基线一致。
- 复核 3 个 C7 已证实根因与当前闭包；同时追自动 push 在 finalize 前的位置。

## crates/kanzei-app/src/run/events/mod.rs

### 职责
分发 runner 事件到 typed facts、trace 和 UI/ASK。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- event handlers/TraceSink → coordinator/execution。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 1648 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 receipt bool、typed fail-closed、ASK guard 取消、TaskProgress 形状与 realtime trace；AF-09 缺口在消费者不支持增量。

## crates/kanzei-app/src/run/execution.rs

### 职责
装配并驱动执行及 review/fixup 阶段。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- execution loop → coordinator。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 374 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 lease 后 claim、runner callbacks、review/fixup 消息衔接、readonly phase 边界。

## crates/kanzei-app/src/run/mod.rs

### 职责
提供运行模块入口、阶段通知和自动推送辅助。

### 判断
P1

### 确切问题
- CONV-AF-07：自动 push 没有等待上限，也不参与本轮取消。finalize_round 尚未执行，运行与写租约被长期占住；Stop 的 30秒 watchdog 只 abort Future，Command 未 kill_on_drop，子 git 可以在界面已停止后继续完成远端写入。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- maybe_push_after_commit → coordinator。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 700 行；LF SHA-256 见 JSON，内容与基线一致。
- 静态确认 AF-07 子进程等待与停止收尾顺序，原有成功/失败 Git tests 全文核对。

## crates/kanzei-app/src/commands/run.rs

### 职责
接收运行、队列、停止、权限和指标 IPC。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- schedule_run/stop_run/answer_ask → UI、durable questions、auto run。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 1475 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 lifecycle 与跨进程 execution owner、队列 FIFO/保存输入、generation、只读 profile、失败收尾；stop 全局空 project 无 UI caller，未单列。

## crates/kanzei-app/src/commands/run/handoff.rs

### 职责
冻结来源对话快照并附到目标输入。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- attach_snapshot → run_prompt。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 103 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对来源身份、idle lifecycle、当前 segment、附件占位、排队时不重读来源。

## crates/kanzei-app/src/commands/run/handoff/tests.rs

### 职责
验证交接快照隔离与持久队列。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- test-only → handoff/conversation/inbox。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 339 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对 typed 优先、reset、来源缺失/运行中拒绝、附件与 reasoning 隔离；未重跑。

## crates/kanzei-app/src/commands/summarize.rs

### 职责
按需生成对话纪要并保存文件。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- summarize_chat/fast_summarize → UI。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 67 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对模型失败、空输出、目录/写入错误可见；秒级文件名并发覆盖未确认当前 UI 可达复用，不登记推测。

## crates/kanzei-app/src/conversation.rs

### 职责
提供历史读取、清空、分段删除和 prior 恢复。

### 判断
P1

### 确切问题
- CONV-AF-04：历史变更绕过跨进程 session 执行 owner。允许在活跃 writer 脚下切段或删除 typed facts；旧轮继续写入会跨段或因历史被删而拒绝，已提交输入可从历史消失。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- conversation_get/list/clear/delete → UI、runner、handoff。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 851 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文覆盖投影/legacy/compaction/reset/delete；AF-04 交叉核对 CLI owner 与本地 guard。

## crates/kanzei-app/src/conversation_actions.rs

### 职责
提供对话/代码回退与 fork IPC。

### 判断
P1

### 确切问题
- CONV-AF-04：历史变更绕过跨进程 session 执行 owner。允许在活跃 writer 脚下切段或删除 typed facts；旧轮继续写入会跨段或因历史被删而拒绝，已提交输入可从历史消失。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- conversation_action → UI；store rewind → 文件 CAS。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 170 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对预览 hash、pending queue、mutation guard、fork seed 与文件失败语义；跨进程互斥归 AF-04。

## crates/kanzei-app/src/conversation_tests.rs

### 职责
验证 typed/legacy 历史、段删除、重启和 prior 边界。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- test-only → conversation/store。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 1423 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文 1423 行核对，重点 reset 地板、删除后不复活、当前缓存清理与原始事实存续；未重跑。

## crates/kanzei-app/src/general_chat.rs

### 职责
拥有无项目对话存储与关联项目的复制流程。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- open/link_to_project/GeneralChatBoundary → UI/processes。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 375 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 global root 身份、profile 限制、source current surface、创建失败关闭孤儿及 compaction guard。

## crates/kanzei-app/src/side_question.rs

### 职责
在有界快照上提供临时无工具问答。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- side_question_send/list/stop → UI。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 510 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对 8并发/128保留、id 幂等、180s timeout、halt、durable child 状态优先和历史工具 run_id。

## crates/kanzei/src/cli/run.rs

### 职责
装配 CLI 对话执行、输入准入和中断处理。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- run_cli → CLI dispatch；run_once/finalize → core。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 532 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对跨进程 owner、new/reset、FIFO 输入、typed user ACK、Ctrl+C select；AF-06 对同任务同步 ASK 可达。

## crates/kanzei/src/cli/run/finalize.rs

### 职责
完成 CLI typed 终态、压缩 surface、episode 与输入收尾。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- finish_run → run_cli。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 579 行；LF SHA-256 见 JSON，内容与基线一致。
- 核对终态拒绝报错、压缩 CAS 不覆盖新事实、失败输入收尾、memory 工作用原始 round_messages。

## crates/kanzei/src/cli/run/permissions.rs

### 职责
把 CLI 权限与问题转换为用户答案。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- make_ask → run_cli → drive。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 94 行；LF SHA-256 见 JSON，内容与基线一致。
- AF-06 原样源码探针确认同步 stdin 阻塞；核对非交互权限策略和保存失败 deny。

## crates/kanzei/tests/integration/agent_step_budget.rs

### 职责
验证 真实 provider 超32步/子代理32步与 batch 交付窗口。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- integration tests → CLI/core/harness/llm。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 279 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对 真实 provider 超32步/子代理32步与 batch 交付窗口 的夹具、状态转移与断言；本轮未重跑集成测试。

## crates/kanzei/tests/integration/context_overflow_recovery.rs

### 职责
验证 真实 CLI 两级 overflow、HTTP 请求与持久 surface/归档。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- integration tests → CLI/core/harness/llm。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 301 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对 真实 CLI 两级 overflow、HTTP 请求与持久 surface/归档 的夹具、状态转移与断言；本轮未重跑集成测试。

## crates/kanzei/tests/integration/cooperative_halt.rs

### 职责
验证 步首不发请求、中途取消子代理、占位结果配对。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- integration tests → CLI/core/harness/llm。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 333 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对 步首不发请求、中途取消子代理、占位结果配对 的夹具、状态转移与断言；本轮未重跑集成测试。

## crates/kanzei/tests/integration/ctrl_c_finalize.rs

### 职责
验证 真实 SessionStore 中断收尾重开；测试没有原生 signal 覆盖，不夸大。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- integration tests → CLI/core/harness/llm。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 60 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对 真实 SessionStore 中断收尾重开；测试没有原生 signal 覆盖，不夸大 的夹具、状态转移与断言；本轮未重跑集成测试。

## crates/kanzei/tests/integration/item_context_boundary.rs

### 职责
验证 真实 CLI/WorkTool 换条目缩减请求，同时保留原始 transcript。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- integration tests → CLI/core/harness/llm。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 131 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对 真实 CLI/WorkTool 换条目缩减请求，同时保留原始 transcript 的夹具、状态转移与断言；本轮未重跑集成测试。

## crates/kanzei/tests/integration/memory_hints_not_persisted.rs

### 职责
验证 system 一次性 hints/scout，不进入 User/history，context report 有账单。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未改，审计阶段；统一完成相邻范围审计后再按根因修复。

### 影响范围
- integration tests → CLI/core/harness/llm。
- 本轮未变更兼容性、API 或持久格式。

### 验证
- 全文 362 行；LF SHA-256 见 JSON，内容与基线一致。
- 全文核对 system 一次性 hints/scout，不进入 User/history，context report 有账单 的夹具、状态转移与断言；本轮未重跑集成测试。

# Module Summary

## 已修复
- P0：0。
- P1：0；确认 6 个未修根因，其中 3 个复用 C7 证据。
- P2：0；确认 2 个未修根因。

## PASS 文件
- crates/kanzei-app/src/commands/run.rs
- crates/kanzei-app/src/commands/run/handoff.rs
- crates/kanzei-app/src/commands/run/handoff/tests.rs
- crates/kanzei-app/src/commands/summarize.rs
- crates/kanzei-app/src/conversation_tests.rs
- crates/kanzei-app/src/general_chat.rs
- crates/kanzei-app/src/permission_tests.rs
- crates/kanzei-app/src/projection_gate.rs
- crates/kanzei-app/src/run/assembly/components.rs
- crates/kanzei-app/src/run/baseline.rs
- crates/kanzei-app/src/run/events/mod.rs
- crates/kanzei-app/src/run/execution.rs
- crates/kanzei-app/src/side_question.rs
- crates/kanzei-app/src/state_tests.rs
- crates/kanzei-app/src/state.rs
- crates/kanzei-app/src/typed_events.rs
- crates/kanzei-core/src/assemble.rs
- crates/kanzei-core/src/runner/compaction.rs
- crates/kanzei-core/src/runner/context.rs
- crates/kanzei-core/src/runner/drive/assembly.rs
- crates/kanzei-core/src/runner/drive/context_budget.rs
- crates/kanzei-core/src/runner/drive/halt.rs
- crates/kanzei-core/src/runner/drive/permissions.rs
- crates/kanzei-core/src/runner/drive/question.rs
- crates/kanzei-core/src/runner/drive/task_results.rs
- crates/kanzei-core/src/runner/line_runtime.rs
- crates/kanzei-core/src/runner/metrics.rs
- crates/kanzei-core/src/runner/mod.rs
- crates/kanzei-core/src/store/events.rs
- crates/kanzei-core/src/store/file_checkpoints.rs
- crates/kanzei-core/src/store/rewind.rs
- crates/kanzei-core/src/store/session.rs
- crates/kanzei-tools/src/run.rs
- crates/kanzei/src/cli/run.rs
- crates/kanzei/src/cli/run/finalize.rs
- crates/kanzei/src/cli/run/permissions.rs
- crates/kanzei/tests/integration/agent_step_budget.rs
- crates/kanzei/tests/integration/context_overflow_recovery.rs
- crates/kanzei/tests/integration/cooperative_halt.rs
- crates/kanzei/tests/integration/ctrl_c_finalize.rs
- crates/kanzei/tests/integration/item_context_boundary.rs
- crates/kanzei/tests/integration/memory_hints_not_persisted.rs

## 仍需人工判断
- 无产品决策阻塞；以上均属于已有合同的修复。

## 依赖影响
- 本轮无 cross-module contract 修改。后续优先统一 steering 的事务/ACK/本轮身份，其次补历史修改的 execution owner。
- batch ownership 应在状态 owner 修复；replay 应支持当前增量写入，不回退写入侧。

## 剩余风险
- 本轮未做全仓构建、完整单测或桌面原生 E2E；PASS 表示全文和必要 caller 审计未发现实际问题，不代表已执行这些门禁。
- AF-07 的远端/认证挂起与进程树取消仅做源码调用链验证，未实际推送。
- AF-04 的 owner/storage 行为用现有二进制库复现，未启动第二个桌面窗口；当前文件与基线已比对。
- CLI 同步 stdin 的 select 计时饥饿已复现，但原生 Windows Ctrl+C 是否解除 stdin 未验证；AF-06 留在 JSON 候选区、不计 bug。
- 原 C7 负例来自独立 audit-c0 树，当前三处根因已复核，但本轮未重跑原 Cargo 探针。

## 独立验证材料

- [output/audit-first/conversation/core-probe.log](<C:/Users/kanzei/Documents/kanzei code/output/audit-first/conversation/core-probe.log>)
- [output/audit-first/conversation/batch-probe.log](<C:/Users/kanzei/Documents/kanzei code/output/audit-first/conversation/batch-probe.log>)
- [output/audit-first/conversation/redundancy-probe.log](<C:/Users/kanzei/Documents/kanzei code/output/audit-first/conversation/redundancy-probe.log>)
- [output/audit-first/conversation/stdin-probe.json](<C:/Users/kanzei/Documents/kanzei code/output/audit-first/conversation/stdin-probe.json>)
- [C7 验证记录](<C:/Users/kanzei/.codex/worktrees/audit-c0/kanzei code/docs/reviews/file-audit-2026-10-03/parallel/C7-verification.json>)
- [C7 原始负例](<C:/Users/kanzei/.codex/worktrees/audit-c0/kanzei code/output/C7/old-probes-valid-proof.json>)
