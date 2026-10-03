# B5 ProcessTool ID 合同地图

基线 `3f8244d9baf0534032a0729ba0763696d3434921`；复用 A 树分支 `kanzei/audit-b5-process-ids`。本包 process.rs 全文（含测试）；background.rs 仅 root 批准的 test-only 私有边界切片，lifecycle/registration/bash/base/team/core caller 只读必要切片，不新增公共覆盖计数。root 正式授独占 Cargo 后，真实正例/旧判定负例和相关验证已完成，生产源码仍为准备时两份冻结内容。

## 依赖、入口与 owner

kanzei-tools Cargo 依赖 harness（Tool/ToolOutput/ToolCtx/并发合同）、tokio/async-trait（异步执行）、serde/schemars（输入）、regex（wait 匹配）。process 为 lib.rs 私有模块，公开工具对象通过 BaseComponent 与 writing TeamTools 注册进 Harness 工具表；真实工具执行按已有 gate/resources 调用，不新增入口或权限资源。

实际资源消费者：harness Tool::resources_with_ctx 默认转发 ProcessTool::resources；core runner/drive/permissions.rs:resolve_permission_gate 与 parallel_tools.rs 的允许并行筛选/授权路径调用该方法。Team ScopedTool 的 resources 转发到内部工具。read_receipt_tests 与 background/tests 中直接 Tool::execute 是已有实际回归 caller，本包只读切片。

process.rs 完整职责：输入解析与 project 身份检查；input 限制 actor owner；watch/unwatch/subscriptions 委托 monitor；list/output 读当前 registry/缓冲；stop 委托 B4 Result 停止；wait 正则/退出/时间预算和尾部；discover/adopt/kill 委托持久 registry。私有 managed_process_id 仅在 stop 的 get(None) 分支被调用，判断已结束/被历史回收的 ID 是否保幂等。活跃记录先经过 project 身份检查，不经此格式判据。

## 分配器全 caller

- lifecycle::next_id：唯一生产分配器，now_ms + 进程内 AtomicU64 SeqCst，自增序号，格式 `bg<ms>-<seq>`。
- registration::register_with_mailbox：唯一生产 next_id caller；创建 BackgroundProcess.id，并进入当前 registry；persistent=true 同 ID 写入既有 registry.json。
- bash::bash_body 的 background/interactive 分支：唯一生产 register_with_mailbox caller，真实 Child/owner/baseline/mailbox登记；content `process_id` 与 display.processId 都返回该真实 ID。
- 其它 next_id 均是 background/monitor 测试 fixture；team tests 直接 register_with_mailbox 是测试，不是第二生产 allocator。adopt 复用 PersistentEntry.id，不重新分配。
- 历史回收：registration::prune_finished 原 keep127，只移除 exit 已结束且 guard 成功/Recovered 的 finished entries；ID 随之前合法输出仍可留在会话历史。持久 adopt watcher/explicit kill 使用既有删除合同。本包不改 allocator、retention、JSON 或 owner。

## 当前静态问题与最小方案

真实路径：BashTool 返回 `bg<ms>-<seq>` → 完成/清理并被原历史 prune 移除 → ProcessTool stop get(None) → managed_process_id 仅接受旧 bgdigits → PROCESS_STOP_BAD_ID，误把真实返回过的合法 ID 当输入错误。原幂等测试仅造旧 bg999999999，没有覆盖当前生产 allocator。此为已由实际 ProcessTool 输出/旧断言对照确认的合法操作拒绝 P1。

最小修复仅扩私有格式谓词：保旧 bgdigits，另接受非空 ASCII digits `-` 非空 ASCII digits；多连字符、空字段、符号/字母继续拒绝。BAD_ID 指引同时说明两种格式；公共签名、stop/kill 行为、permission resources、allocator 不变。

## root 批准的测试边界

额外 background.rs 仅 `#[cfg(test)] pub(crate) prune_finished_process_for_test(id)->bool`：取本测试自己真实 registered Arc，单项历史 map 通过同 production prune_finished(keep0) 判据，只有 eligible 才移除该 ID。未完成/失败不移除，不影响其它 registry entries，不改生产 keep127。由 process.rs 回归访问，避免 fork128进程或假造 allocator ID。

回归：真实 BashTool echo background→拿真实 display.processId/registry record→ProcessTool wait 真实自然退出→stop_result 等 joined→原 prune 判据移除自己的记录→get(None)→实际 ProcessTool stop，断言合法幂等 output、无 BAD_ID。另保已有旧 bgdigits 正常控制与 malformed current multi-hyphen 等八种实际 stop 拒绝。负对照仅恢复旧 managed_process_id 方法，保新真实 fixture、指引与 test-only bridge；真实单断言 exit101，合法控制单 PASS，finally 两源码 exact bytes/raw/LF SHA/mtime 恢复。不是完整旧源码回退。

## 历史合同与边界

R-097（requirements-archive.md）明确 background 返回句柄、list/stop 最小流程和实际进程树关闭；R-330 明确 wait 的真实输出/匹配/退出语义，均未规定“只有纯数字旧 ID 才合法”。既有 stop 幂等测试就是本次应保持的合同。B4 Result/成功清理收据已合入，不倒退它；wait/资源/持久格式没实际新证据的猜测不列问题。

## 验证状态

真实两定向正例 PASS；旧判定单断言失败/合法控制 PASS；恢复刷新后 process 7 PASS、tools 799 PASS/0 FAIL/3 IGNORE、tools/core/app all-target check/Clippy-D warnings/fmt/diff 全 0。首轮 tools 两 browser 失败是 A 树无 playwright-core；保存原日志/JSON，用 root 已验证 runtime 后 browser 六项控制和全 tools 通过。工具 profile 为已初始化 B2 隔离 profile，TEMP/TMP 位于 Git 树外；工具缓存/浏览器路径与实际环境写入 checks JSON。不新增依赖/框架/持久格式或公共 coverage。原 B4 的 94 份证据已由 root 原字节保存，本包不动；M7 ignored 草稿不入 B5 leaf。本包基线验证不冒充 root 此前 17 项新源码证书。
