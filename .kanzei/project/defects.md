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
- 批次: 3/3
- 进展: B1 已提交(ed305ae8)，B2 已提交(a1e27bdb)。B3 已完成并逐条对账：① bash/git/test_record/web 等工具统一经 crates/kanzei-core/src/runner/tool_exec.rs:107-174 与 drive.rs:761-875、1390-1451 外置；事件仅落 preview+artifact 元数据于 crates/kanzei-app/src/run/events/mod.rs:266-289，T-1786922726086/T-1786922726088/T-1786922726089 覆盖 app/tools/core 回归；② durable 文件不依赖进程内状态，tool_exec.rs:453-487 在新 ToolCtx 下重新读取 artifact.relative_path 并断言原文 bytes 一致，sha256 同时由 tool_exec.rs:152-158 写入；③ artifact 写失败无引用由 tool_exec.rs:486-510 覆盖，事件写失败由 crates/kanzei-app/src/run/events/mod.rs:130-146 调用 state.rs:327-362 生成 `.orphan.json` 整理标记，state.rs:840-875 有回归；④ UI/模型看到 `tool_result_externalized` preview 与 artifact 元数据：app events/mod.rs:266-289，前端 ui/07-events.js:217-230；⑤既有 read 流式 offset/limit 实现在 crates/kanzei-tools/src/read.rs:205-238，新增 read.rs:518-541 回归确认只返回请求区间、不复制整文件；⑥权限 gate 与既有错误码路径未改动，drive.rs:1256-1381，T-1786922726086/T-1786922726088/T-1786922726089 全绿。失败记录 T-1786922726087 仅为新增测试对子串的误断言，已收窄整行匹配并由 T-1786922726088 通过。
- observed_head: a1e27bdbca57bf69603f22c2f89ec7851056b1e5
- observed_worktree_hash: fnv1a64:49137dd9fe24f12e
- recorded_at: 1786941647508
