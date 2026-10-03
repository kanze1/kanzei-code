# 自动运行与底层 owner 修复记录

整合状态：已在 dev 分组提交，未发布；提交与独立快照验证见 [修复汇总](fixes-summary.md)。

10 项已修复并验证：P1 7 项、P2 3 项。无未解决项。代理交付后由root完成分组提交，未发布；历史审计记录保持不变。

验证：tools 821 passed / 0 failed / 3 原有忽略；app 624 passed；core 424 passed；CLI quarantine 2 passed；最新 preview owner 1 passed；全工作区 check、14 个修改文件格式检查通过。

## crates/kanzei-tools/src/background/persistent.rs — AUTO-001

### 职责
持有后台服务登记状态，串行执行注册表读改写。

### 判断
P1（已修复并验证）

### 确切问题
同项目已有 A，启动 B 与 A 自然退出并发：两个路径分别读取旧 [A]，启动写 [A,B]，退出随后写 []。 仍在运行的 B 从持久登记消失，应用重启后无法发现/接管；相反顺序也可复活已结束的登记。

### 修改
项目 registry 的读取、变更、原子替换由同一系统排他锁覆盖，读方共享锁。

### 影响范围
crates/kanzei-tools/src/background/persistent.rs, crates/kanzei-tools/src/background/registration.rs

### 验证
回归：registry_transaction_serializes_read_modify_write。tools --lib: 821 passed; 0 failed; regression present and passed

## crates/kanzei-tools/src/background/persistent.rs — AUTO-002

### 职责
定义后台服务持久登记和恢复操作的失败合同。

### 判断
P1（已修复并验证）

### 确切问题
registry.json 非法 JSON/UTF-8 或读取失败时仍启动新的 persistent 服务；或 registry 替换因占用/路径错误失败。 旧登记可被空表覆盖；服务启动/清理返回成功但持久登记未更新，重启恢复与运行态分叉。

### 修改
只有 NotFound 是空登记；损坏/IO/写入失败返回 Result；后台注册失败完成进程终止，停止/发现/接管 caller 传播失败。

### 影响范围
crates/kanzei-tools/src/background/persistent.rs, crates/kanzei-tools/src/background/registration.rs

### 验证
回归：registry_corruption_and_io_errors_do_not_become_empty_success, persistent_registration_failure_stops_the_spawned_child_and_preserves_evidence。tools --lib: 821 passed; 0 failed; regression present and passed

## crates/kanzei-tools/src/background/registration.rs — AUTO-003

### 职责
收集后台 stdout/stderr，定期持久化日志。

### 判断
P2（已修复并验证）

### 确切问题
服务输出一次 READY（小于64KiB），随后一直运行且没有新输出。 输出仅留内存；强杀应用后缺失该段日志，重启接管回看不完整。当前运行中的内存读取仍可见。

### 修改
读流 select 增加 2 秒独立刷新定时器，小日志在服务空闲期间也写盘。

### 影响范围
crates/kanzei-tools/src/background/registration.rs

### 验证
回归：persistent_idle_output_flushes_before_process_exit。tools --lib: 821 passed; 0 failed; regression present and passed

## crates/kanzei-tools/src/schedules/mod.rs — AUTO-004

### 职责
将开关状态写回定时任务定义头。

### 判断
P1（已修复并验证）

### 确切问题
合法定义使用缩进 enabled 字段，或省略头部 enabled 且正文包含行首 enabled: 示例。 前者 toggle 产生重复字段，合法操作被拒绝；后者启用只改正文，真实 enabled 仍 false。执行器自动禁用也可能写坏合法定义。

### 修改
仅更新 parser 接受的定义头 enabled；跳过步骤 block scalar，缺失字段在头部补入，保留正文。

### 影响范围
crates/kanzei-tools/src/schedules/mod.rs

### 验证
回归：toggle_only_updates_the_parsed_frontmatter_enabled_field。tools --lib: 821 passed; 0 failed; regression present and passed

## crates/kanzei-tools/src/schedules/hosts.rs — AUTO-005

### 职责
登记和移除服务器 crontab 的单个任务。

### 判断
P1（已修复并验证）

### 确切问题
同项目在同一服务器登记 check 与 check-long，再保存、禁用或删除 check。 check-long 的 cron 行也被删除，定义仍启用但不再被唤醒。

### 修改
远端 crontab 过滤匹配完整行尾 marker，禁止 check 误伤 check-long；实际 shell 回归使用私有 crontab 函数夹具。

### 影响范围
crates/kanzei-tools/src/schedules/hosts.rs

### 验证
回归：cron_update_preserves_tasks_whose_names_share_a_prefix。tools --lib: 821 passed; 0 failed; regression present and passed

## crates/kanzei-app/src/processes/lifecycle.rs — AUTO-006

### 职责
更新并持久化对话进程设置。

### 判断
P1（已修复并验证）

### 确切问题
process_update 改模型/权限开关后，state.db 打开或 upsert 失败（例如数据库被占用或磁盘写入错误）。 RPC 报失败，但后续运行读到已经改变的模型/子代理/tracker 开关；重启又恢复旧值。

### 修改
持有设置锁，在分离 Arc 的候选设置上应用更新；成功落库后才发布到原 handle，失败保持内存原值。

### 影响范围
crates/kanzei-app/src/processes/lifecycle.rs

### 验证
回归：settings_write_failure_preserves_runtime_and_success_publishes。app --bin kzapp 624 passed; targeted regression present and passed

## crates/kanzei-tools/src/team/tools.rs — AUTO-007

### 职责
为子任务生成团队收件上下文并确认消息交付。

### 判断
P1（已修复并验证）

### 确切问题
子任务运行中、下一次模型请求前积累13条补充消息；或 store.get 后、update 前又收到新消息。 未出现在模型上下文里的消息也变 received，轮末被标 processed，后续 worker 不会再以 queued 消费，用户方向可丢失。

### 修改
保留最近 12 条历史并加入全部 queued 消息；只将当前上下文快照中存在的 ID 确认为 received，晚到消息保持 queued。

### 影响范围
crates/kanzei-tools/src/team/tools.rs

### 验证
回归：inbox_delivers_all_queued_and_does_not_acknowledge_later_arrivals。tools --lib: 821 passed; 0 failed; regression present and passed

## crates/kanzei-app/src/phase_pipeline.rs — AUTO-010

### 职责
按勘察/复核角色装配子代理模型运行时。

### 判断
P2（已修复并验证）

### 确切问题
配置 models.scout 指向 context_limit 与 fast/primary 不同的 provider。 勘察按旧窗口做上下文预算，可提前压缩/拒绝，或向较小窗口模型发送过大的请求。未指定 scout 时不受影响。

### 修改
ScoutRoute 携带 provider context_limit，同时替换 runtime fast/primary 限制；None 清除旧限制，review 不被覆盖。

### 影响范围
crates/kanzei-app/src/phase_pipeline.rs

### 验证
回归：scout_route_replaces_context_limits_without_changing_review_runtime。app --bin kzapp 624 passed; targeted regression present and passed

## crates/kanzei-app/src/collaboration.rs — AUTO-011

### 职责
从执行事件推导当前仍活动的工具。

### 判断
P2（已修复并验证）

### 确切问题
A start、B start、B completed，而 A 仍运行（允许并行的只读工具批次）。 协作快照/模型协作上下文显示 current_tool=None，其他线路看不到仍在进行的工具。仅影响当前活动信息。

### 修改
反向遍历以 tool call id 配对完成事件，某个并行工具结束后仍显示其他活动工具。

### 影响范围
crates/kanzei-app/src/collaboration.rs

### 验证
回归：parallel_tool_completion_keeps_the_other_tool_visible。app --bin kzapp 624 passed; targeted regression present and passed

## crates/kanzei/src/cli/quarantine.rs — AF-T01

### 职责
解析清理命令的演练与实际应用参数。

### 判断
P1（已修复并验证）

### 确切问题
执行 kz quarantine --dry-run --apply --type bg；参数顺序与 --apply --dry-run 相反。 互斥参数仅单向检查，dry-run 命令实际进入 apply 并删除隔离恢复材料。

### 修改
dry-run/apply 两种出现顺序均拒绝，避免后置 apply 悄悄覆盖演练语义。

### 影响范围
crates/kanzei/src/cli/quarantine.rs, crates/kanzei-tools/src/quarantine.rs

### 验证
回归：apply与dry_run互斥且apply允许解析。CLI quarantine 2 passed

# Module Summary

## 已修复
- P0：0。
- P1：AUTO-001/002/004/005/006/007，AF-T01。
- P2：AUTO-003/010/011。

## PASS 文件
- 本轮只改上述问题及必要直接 caller；全文 PASS 清单仍见历史 automation.md。

## 仍需人工判断
- 无。AUTO-008/009 仍为范围外观察，未恢复到本轮计划。

## 依赖影响
Persistent registry JSON fields/layout unchanged; Rust background APIs return Result and async registration awaits cleanup; callers migrated. UI IPC settings schema unchanged.

设置锁复查：process_info、persist_process 和 register_process 都先获取 model，再依次 profile/research_topic/reasoning/manual_models；新更新事务持 model→profile→reasoning→manual_models。普通 clone 读取语句结束即释放；general_chat::enforce 只持 profile→research_topic，不再取 model/reasoning，因此没有新增反序环。候选只复制设置 Arc，未改其他状态 owner。

registry 的 write_atomic 不自取 FileLock，读改写事务由外层排他锁持有，不存在重复取锁。

必要 caller 文件：crates/kanzei-tools/src/background.rs, crates/kanzei-tools/src/background/persistent.rs, crates/kanzei-tools/src/background/registration.rs, crates/kanzei-tools/src/background/lifecycle.rs, crates/kanzei-tools/src/bash.rs, crates/kanzei-tools/src/process.rs, crates/kanzei-tools/src/schedules/mod.rs, crates/kanzei-tools/src/schedules/hosts.rs, crates/kanzei-tools/src/team/tools.rs, crates/kanzei-tools/src/team/tests.rs, crates/kanzei-app/src/processes/lifecycle.rs, crates/kanzei-app/src/collaboration.rs, crates/kanzei-app/src/phase_pipeline.rs, crates/kanzei/src/cli/quarantine.rs

另一个用户任务新增 tracker 字段后，按 root 指令只将其测试计数 27→29；此更改不进入本轮提交。

## 验证证据
- cargo test -p kanzei-app --bin kzapp：624 passed; 0 failed; 0 ignored。日志 `output/audit-first/conversation/fix-app-tests.log`。
- cargo test -p kanzei --bin kz cli::quarantine::tests：2 passed; 0 failed。日志 `output/audit-first/automation/fix-cli-tests.log`。
- cargo test -p kanzei-core --lib：424 passed; 0 failed。日志 `output/audit-first/automation/fix-core-tests.log`。
- cargo check --workspace：PASS。日志 `output/audit-first/automation/fix-workspace-check.log`。
- cargo test -p kanzei-app --bin kzapp preview::agent::ownership_tests：1 passed; 0 failed。日志 `output/audit-first/automation/fix-preview-owner-tests.log`。
- cargo test -p kanzei-tools --lib：817 passed; 4 failed; 3 ignored; corrected 2 previous discovery assertions, shell PATH fixture, and foreign field-count assertion; final rerun queued。日志 `output/audit-first/automation/fix-tools-tests.log`。
- cargo test -p kanzei-tools --lib：821 passed; 0 failed; 3 existing ignored。日志 `output/audit-first/automation/fix-tools-tests-final.log`。
- cargo check --workspace：PASS。日志 `output/audit-first/automation/fix-workspace-check-final.log`。
- rustfmt --check --edition 2021 --config skip_children=true <14 owned Rust files>：PASS。日志 `output/audit-first/automation/fix-format-check.log`。

首轮工具测试的 4 个失败已如实保留：两个旧发现接口测试仍断言成功文案，改为断言显式回滚失败；cron shell 夹具缺 /usr/bin，补齐私有夹具 PATH；第四个为其他任务新字段的旧计数断言。最终全部通过。

## 剩余风险
- 无已知未修复项。验证边界：Windows 本地实际测试；cron 使用 Git Bash 中隔离的 crontab 函数夹具，未连接真实远端服务器。 未执行真实 LLM 或桌面交互 E2E；此次对应状态合同由纯逻辑/SQLite/真实进程回归验证。
