# Defects

## D-433 R-280 加列未提 SCHEMA_VERSION，存量库装机即崩在 no such column: subagents_enabled [fixing] (high)
- refs: R-280 D-373 D-297
- 复杂度: 小
- 复现: 用 build-ac637546 覆盖安装到已有 .kanzei/state.db(schema_version=16)的机器，启动后进程列表每次刷新报「读取进程注册失败: sqlite error: no such column: subagents_enabled」。新建库无此现象——新库走建表批，列是有的。
- 影响: 桌面端进程列表完全不可用，自举循环拿不到进程注册；用户 2026-08-17 11:38 装机即撞。
- 来源: 用户实测 build-ac637546 装机后报错。
- 标签: 核心
- 根因: R-280 把 subagents_enabled 加进 processes 建表批并补了幂等 ALTER，却没有 +1 SCHEMA_VERSION。migrate 在 version == SCHEMA_VERSION 时早退，存量库根本不执行 ALTER 批。D-373 立的判据只冻结**对象名集合**(SCHEMA_OBJECTS)，加列不改对象名，于是编译、clippy、全量测试、十步门禁全绿放行——与 D-297 同一条早退路径，只是粒度更细。
- 证据等级: E3(用户真机装机复现 + 定向回归在缺列的存量库上复现并修复)
- 验收: ①SCHEMA_VERSION 提到 17 且建表批里的硬编码字面量同步；②停在上一版、缺 subagents_enabled 的存量库 open 后把列补回来(回归 停在上一版的存量库open后补齐缺失的列)；③新增列级机械判据 SCHEMA_COLUMNS，加列不提版本号即红灯(回归 建表批新增列必须伴随schema版本提升)；④workspace 全量与十步门禁全绿后重新发版。
- 优先级: P0
- 批次: 1/1

## D-349 工具大输出在事实入库前不可逆截断，trace 仅留 preview 且无完整原文回读 [fixing] (high)
- refs: D-209 R-180 R-245 docs/design/deepseek_harness_upgrade.md
- 复杂度: 中
- 复现: 执行输出超过上限的 bash/git/webfetch 或后台任务：bash/git 在工具层截断，run.trace 再仅记录 preview；当前会话没有 artifact_id 或回读指引。进程退出或上下文压缩后，用户和模型均无法从会话恢复完整原文。
- 影响: 工具结果的事实在写入事件日志前已经丢失；审计、故障复盘、后续精确引用和压缩后回读只能看到片段，可能隐藏真正报错或把截断结果误认为完整结果。
- 来源: 2026-08-14 DeepSeek Harness Spill 对照审计与现行代码核查。
- 标签: 核心
- 根因: 各工具各自实现容量上限和截断文案，ToolOutput 没有 Inline/Spilled 统一结果类型，也没有“完整 artifact 写成功后再提交引用事件”的原子契约。
- 证据等级: E2(静态读码确认截断点与 preview 入库路径；本地输出分布已量化)
- 验收: ①超过阈值的 bash/git/test_record/web 类结果完整原文进入 durable artifact，事件只存 preview+artifact_id+bytes+sha256+retrieval_hint；②重启后按引用取回内容与工具原始字节 sha256 一致；③artifact 写失败时不得提交成功引用事件，事件写失败时无引用 artifact 可由整理入口识别；④UI/模型明确显示结果已外置而非已丢弃；⑤read 的原文件 offset/limit 回读不重复复制；⑥现有工具权限与错误码不变。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-349
- 批次: 1/3
- 进展: B1 已提交(ed305ae8)。此前反复被拦不是「运行态没刷新」这种含糊原因，而是记录时间单位 bug：正在跑的 kzapp 是 2026-08-09 安装版，其 last_passed 对缺「收尾」字段的记录直接 parse id 且不归一化，而 id 分配器早已推进到 13 位毫秒量级——T-1786922726036(R-285 Playwright，无收尾)折算成 1786922726036「秒」，永远压过任何带收尾(≈1.7869e9)的 Rust 记录，于是每次都拿非 Rust 覆盖面来判 Rust 提交。已双管齐下：①归一化实现随本次提交进 dev；②给仅有的两条 13 位无收尾记录 T-1786922726035/036 回填 收尾: 1786922726(= id/1000，与归一化算出的值一致)，让在跑的旧二进制也不再选中它们。提交前独立复核：staged 源码指纹 3eb82d51b113d94a 与 T-1786922726080/081/082 一致，fmt/clippy/workspace 全绿(1169 passed, 2 ignored)。下一步进入 B2(bash/webfetch/test_record 大结果边界与外置可见性)。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:907983d54aa63911
- recorded_at: 1786934918187

## D-428 D-428 归档 fixed 的 D-409 提交不在当前 dev，记忆 inbox 分批修复未接入 [fixing] (high)
- 复现: 修复前复现：当前 dev 的 crates/kanzei-app/src/memory.rs:311-374 与 crates/kanzei/src/cli/memory.rs:29-75 调用 read_inbox/整箱 consolidation，且 run 结果被忽略；全仓无 read_inbox_batch 符号。修复后共享实现位于 crates/kanzei-memory/src/memory/inbox.rs:18-122 与 crates/kanzei-tools/src/memory_consolidation.rs:1-301，调用方已迁移。
- 影响: requirements、defects、tests 与实现互相矛盾；系统仍可能无法按批消化 inbox，R-286 的 P0 事实恢复被错误 fixed 状态掩盖。
- 期望: 在当前 dev 原子接入分批读取、checkpoint、错误可见和 CLI/桌面共用服务；重新跑定向测试并把 D-409/R-286/tests/实现证据绑定到当前 dev 提交。
- 来源: self-found：R-283 Wave 0 事实复核
- 标签: 核心
- 根因: D-409 的修复提交来自另一条线/历史观察点，归档状态先于实现进入当前 dev，缺少当前分支提交存在性门禁。
- refs: D-409 R-286 R-283
- 优先级: P0
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-428
- 批次: 3/3
- 进展: 批次: 3/3。B1/B2 实现已接入；B3 已修复历史测试 ID 时间归一，并进一步修复 crates/kanzei-tools/src/test_record.rs:801-840 与 crates/kanzei-tools/src/git.rs:772-781：存在源码指纹时，last_passed 与提交门禁均优先选择最新源码指纹组，不再被无指纹历史前端记录遮蔽；新增 last_passed_prefers_fingerprinted_group_over_newer_legacy_record 与 source_test_gate_prefers_matching_fingerprint_over_newer_legacy_record 回归。T-1786922726066 两条回归及 fmt 通过；T-1786922726067：新 staged 代码下 fmt、kanzei-tools 320 passed/1 ignored，以及六 crate 覆盖全部通过（kanzei 37、app 196、core 214、harness 150、memory 142/1 ignored、tools 321/1 ignored）。B1/B2/B3 实现已随 ed305ae8 进入 dev(inbox.rs、memory_consolidation.rs 及 app/CLI 调用方均在该提交内)。此前挡住提交的不是运行态刷新问题，而是旧 kzapp(2026-08-09 安装版)把 13 位毫秒 id 当秒比较，详见 D-349 进展。下一步完成 D-409/R-286/tests/实现对账并关闭 D-428。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:b71f5d240160cea5
- recorded_at: 1786929654689
