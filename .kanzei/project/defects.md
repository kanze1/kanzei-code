# Defects

## D-349 工具大输出在事实入库前不可逆截断，trace 仅留 preview 且无完整原文回读 [open] (high)
- refs: D-209 R-180 R-245 docs/design/deepseek_harness_upgrade.md
- 复杂度: 中
- 复现: 执行输出超过上限的 bash/git/webfetch 或后台任务：bash/git 在工具层截断，run.trace 再仅记录 preview；当前会话没有 artifact_id 或回读指引。进程退出或上下文压缩后，用户和模型均无法从会话恢复完整原文。
- 影响: 工具结果的事实在写入事件日志前已经丢失；审计、故障复盘、后续精确引用和压缩后回读只能看到片段，可能隐藏真正报错或把截断结果误认为完整结果。
- 来源: 2026-08-14 DeepSeek Harness Spill 对照审计与现行代码核查。
- 标签: 核心
- 根因: 各工具各自实现容量上限和截断文案，ToolOutput 没有 Inline/Spilled 统一结果类型，也没有“完整 artifact 写成功后再提交引用事件”的原子契约。
- 证据等级: E2(静态读码确认截断点与 preview 入库路径；本地输出分布已量化)
- 阻塞: 2026-08-16 复核收窄:**R-244 已 done 并归档**(Tool Pipeline 结果阶段已稳定),原阻塞的前半已解除;只剩等 R-245 实施,而 R-245 自身只剩等 R-242(见该两条)。当前仍作为事实丢失缺陷登记(high),不单独修——在 R-245 的 Result Policy 与 spill 落点上一并解决。解除人: 依赖自然解除。
- 验收: ①超过阈值的 bash/git/test_record/web 类结果完整原文进入 durable artifact，事件只存 preview+artifact_id+bytes+sha256+retrieval_hint；②重启后按引用取回内容与工具原始字节 sha256 一致；③artifact 写失败时不得提交成功引用事件，事件写失败时无引用 artifact 可由整理入口识别；④UI/模型明确显示结果已外置而非已丢弃；⑤read 的原文件 offset/limit 回读不重复复制；⑥现有工具权限与错误码不变。
- 优先级: P1

## D-428 D-428 归档 fixed 的 D-409 提交不在当前 dev，记忆 inbox 分批修复未接入 [open] (high)
- 复现: 当前 dev 中 crates/kanzei-app/src/memory.rs:311-374 与 crates/kanzei/src/cli/memory.rs:29-75 仍调用 read_inbox/整箱 consolidation，且 run 结果被忽略；全仓无 read_inbox_batch 符号。D-409 归档进展引用的 5a15cdc/b4245f6c 不在当前 dev 历史。
- 影响: requirements、defects、tests 与实现互相矛盾；系统仍可能无法按批消化 inbox，R-286 的 P0 事实恢复被错误 fixed 状态掩盖。
- 期望: 在当前 dev 原子接入分批读取、checkpoint、错误可见和 CLI/桌面共用服务；重新跑定向测试并把 D-409/R-286/tests/实现证据绑定到当前 dev 提交。
- 来源: self-found：R-283 Wave 0 事实复核
- 标签: 核心
- 根因: D-409 的修复提交来自另一条线/历史观察点，归档状态先于实现进入当前 dev，缺少当前分支提交存在性门禁。
- refs: D-409 R-286 R-283
- 优先级: P0
