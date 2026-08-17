# Defects

## D-349 工具大输出在事实入库前不可逆截断，trace 仅留 preview 且无完整原文回读 [fixing] (high)
- refs: D-209 R-180 R-245 docs/design/deepseek_harness_upgrade.md
- 复杂度: 中
- 复现: 执行输出超过上限的 bash/git/webfetch 或后台任务：bash/git 在工具层截断，run.trace 再仅记录 preview；当前会话没有 artifact_id 或回读指引。进程退出或上下文压缩后，用户和模型均无法从会话恢复完整原文。
- 影响: 工具结果的事实在写入事件日志前已经丢失；审计、故障复盘、后续精确引用和压缩后回读只能看到片段，可能隐藏真正报错或把截断结果误认为完整结果。
- 来源: 2026-08-14 DeepSeek Harness Spill 对照审计与现行代码核查。
- 标签: 核心
- 根因: 各工具各自实现容量上限和截断文案，ToolOutput 没有 Inline/Spilled 统一结果类型，也没有“完整 artifact 写成功后再提交引用事件”的原子契约。
- 证据等级: E2(静态读码确认截断点与 preview 入库路径；本地输出分布已量化)
- 阻塞: 
- 验收: ①超过阈值的 bash/git/test_record/web 类结果完整原文进入 durable artifact，事件只存 preview+artifact_id+bytes+sha256+retrieval_hint；②重启后按引用取回内容与工具原始字节 sha256 一致；③artifact 写失败时不得提交成功引用事件，事件写失败时无引用 artifact 可由整理入口识别；④UI/模型明确显示结果已外置而非已丢弃；⑤read 的原文件 offset/limit 回读不重复复制；⑥现有工具权限与错误码不变。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-349
- 批次: 1/3
- 进展: B1 已落地并通过 T-1786922726078：crates/kanzei-harness/src/tool.rs 新增 ToolArtifact 与 ToolOutput.artifact；crates/kanzei-harness/src/lib.rs re-export；crates/kanzei-core/Cargo.toml 引入 kanzei-base；crates/kanzei-core/src/runner/tool_exec.rs 在统一 ToolEnd 前对超限结果原子写入 .kanzei/artifacts/tool-results，生成 artifact_id/bytes/sha256/retrieval_hint，写失败转 TOOL_RESULT_SPILL_FAILED，新增成功与失败单测；crates/kanzei-tools/src/git.rs 移除 run_git_owned 的 1 MiB 前置截断。验证：fmt、core 219、harness 150、tools 320（1 ignored）全通过。下一步 B2：在统一物化出口接入 bash、webfetch、test_record 的大结果边界，并补 UI/模型外置提示与失败可见测试。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:7f345376c1799baf
- recorded_at: 1786934111695

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
- 进展: 批次: 3/3。B1/B2 实现已接入；B3 已修复历史测试 ID 时间归一，并进一步修复 crates/kanzei-tools/src/test_record.rs:801-840 与 crates/kanzei-tools/src/git.rs:772-781：存在源码指纹时，last_passed 与提交门禁均优先选择最新源码指纹组，不再被无指纹历史前端记录遮蔽；新增 last_passed_prefers_fingerprinted_group_over_newer_legacy_record 与 source_test_gate_prefers_matching_fingerprint_over_newer_legacy_record 回归。T-1786922726066 两条回归及 fmt 通过；T-1786922726067：新 staged 代码下 fmt、kanzei-tools 320 passed/1 ignored，以及六 crate 覆盖全部通过（kanzei 37、app 196、core 214、harness 150、memory 142/1 ignored、tools 321/1 ignored）。下一步提交 16 个明确文件，提交后完成 D-409/R-286/tests/实现对账关闭 D-428。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:b71f5d240160cea5
- recorded_at: 1786929654689
- 阻塞: 结构化 Git 门禁运行时维护者：刷新/重启提交门禁运行态，使其加载当前 dev 的 crates/kanzei-tools/src/test_record.rs 与 crates/kanzei-tools/src/git.rs；刷新后本线使用 staged hash d3a9ae694ac0ae7a 重试 commit。
