# Defects

## D-434 停车没有一等机制，只能写进「阻塞」字段，下一轮复核就被当失效自阻塞清掉 [fixing] (high)
- refs: D-354 D-242 R-247
- 复杂度: 小
- 复现: 单 WIP 槽满时想让出一条，引擎只认「阻塞」字段判非可执行(work.rs 无 parked 概念，docstore 状态枚举只有 todo/doing/done/dropped)，于是停车只能伪装成阻塞；下一轮复核阻塞时看到「解除人是 agent 自己」判为失效自阻塞清掉，多个条目同时转为可执行，work next 返回 wip_violation 拒绝取活。2026-08-17 实测：R-221/R-216/R-281/D-349 四个可执行 WIP，取活停摆。
- 影响: 单 WIP 纪律与阻塞复核纪律互相拆台，自举循环在「清阻塞 → 撞 wip_violation → 再补阻塞」之间来回，无法稳定取活。
- 来源: 用户 2026-08-17「还是卡住，说 wip 被占用了」→「parked 修复呢」。
- 标签: 核心
- 根因: 「不可执行」被压成单一维度。阻塞(等外部前提，复核前提是否仍成立)与停车(主动让出单槽，需显式恢复)处置方式相反，却共用一个字段，谁复核谁清错。
- 证据等级: E3(真实 tracker 上复现 wip_violation，修复后 kz work next 由 wip_violation 变 resume)
- 验收: ①新增「停车」字段，被引擎识别为不可执行且不占 WIP 槽；②停车条目落在 parked_items 而非 blocked_items；③全员不可执行时裁决理由把停车与阻塞分开陈述并点名停车条目；④kanzei-memory 的 workable_titles 同步不把停车条目当可干的活；⑤dev system prompt 教「停车写 停车: 不写 阻塞:」「复核阻塞时不要动停车」，并有守护测试；⑥R-221/R-216/R-281 由 阻塞 迁到 停车 后 work next 仍 resume 到 D-349。
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
