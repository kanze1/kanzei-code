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
- 验收: ①超过阈值的 bash/git/test_record/web 类结果完整原文进入 durable artifact，事件只存 preview+artifact_id+bytes+sha256+retrieval_hint；②重启后按引用取回内容与工具原始字节 sha256 一致；③artifact 写失败时不得提交成功引用事件，事件写失败时无引用 artifact 可由整理入口识别；④UI/模型明确显示结果已外置而非已丢弃；⑤read 的原文件 offset/limit 回读不重复复制；⑥现有工具权限与错误码不变。
- 优先级: P1
- 取活依据: engine:唯一可执行 WIP 是 D-349，必须先恢复它
- 批次: 2/3
- 进展: B1 已提交(ed305ae8)。B2 已落地并验证：①统一出口 crates/kanzei-core/src/runner/tool_exec.rs:107-174 实现大结果 durable artifact（原文 bytes/sha256/artifact_id/retrieval_hint），写失败返回 TOOL_RESULT_SPILL_FAILED 且不生成引用；②并行路径 tool_exec.rs:182-260、串行路径 crates/kanzei-core/src/runner/drive.rs:1390-1451、前台/后台 task drive.rs:761-875 均在 ToolEnd/模型回喂前调用 materialize_tool_output；③事件契约 event.rs:62-78 与 TaskTrace:9-28 携带 artifact，app 转发/落盘位于 crates/kanzei-app/src/run/events/mod.rs:266-289、354-416，UI 收到 preview+artifact 元数据，外置后 display.full 被清除；④回归 T-1786922726083、T-1786922726084：kanzei-core 221 passed，kanzei-app 196 passed，artifact 原文回读字节一致、写失败无成功引用、既有权限/错误码路径全绿。B3 待做：重启后引用回读/整理入口、read offset/limit 不重复复制的专项证据，以及逐条验收收口。
- observed_head: 851ca72c842f9b38ef6ae9304c523ab911a1aae8
- observed_worktree_hash: fnv1a64:b41df30c6416d398
- recorded_at: 1786941072509
