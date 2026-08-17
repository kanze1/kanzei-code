# Defects

## D-486 R-242 shadow 比较器将压缩后 legacy surface 误判为 unknown mismatch [fixing] (medium)
- 复现: 真实项目执行 `cargo run -p kanzei -- shadow --project-root (Get-Location).Path --mismatches`：最新窗口出现 `typed_write_errors=[]` 但 `projected_messages=151`、`legacy_messages=13`、`first_mismatch=1`、`expected_mismatch=false`；该窗口在事件日志中包含多轮 typed facts 与一次 `conversation.updated`，legacy 是压缩后的短 surface。现有 `classify_mismatch` 只识别 legacy 为空、legacy 为 projection 前缀和失败 diagnostics，不识别压缩后的 legacy surface。
- 影响: R-242 的 shadow gate 将可解释的 surface compaction/快照重建差异计为 unknown mismatch，真实窗口无法区分投影写入错误与 compaction 尚未事件化，阻碍建立有效的 30 turn typed_write_errors=0 统计窗口。
- 来源: self-found：R-242 真实 shadow 诊断；项目 state.db 最新 shadow 事件与 `crates/kanzei-core/src/store/typed.rs:1453-1483` 代码对照。
- 标签: 核心
- 验收: 新增回归覆盖 legacy 是 projection 的有效尾部/压缩后 surface 时标为 expected_mismatch（compacted_snapshot），仍保留真正中间内容不一致为 unknown；`cargo test -p kanzei-core` 通过；真实 shadow 输出不再把该类差异计入 unknown。
- refs: R-242
- 优先级: P1
- 状态: fixing
- 进展: 已实现并验证分类修复：`crates/kanzei-core/src/store/typed.rs:1478-1488` 在 legacy 精确等于 projection 尾部时标记 `compacted_snapshot`，中间不一致与 legacy 反超仍返回 unknown；回归位于 `typed.rs:2239-2252`，T-1786922726218（kanzei-core 222 passed）通过。真实 state.db 诊断已确认触发场景：最新 shadow 事件 projected=151、legacy=13、typed_write_errors=[]，但历史事件不会自动重写；待下一次真实 shadow turn 产生新事件后复核统计，再满足“真实 shadow 输出不再计入 unknown”后关闭。
- observed_head: dcf6e11c4a0557ad9283234084a431bf61f3e083
- observed_worktree_hash: fnv1a64:b5a0bda6129c84a4
- recorded_at: 1786996867134
- 停车: 代码修复与 `cargo test -p kanzei-core` 已完成；本轮先让位给 R-242 建立真实 shadow 验证窗口，待新 shadow 事件产生后恢复并复核 unknown 统计。

## D-487 typed writer 在失败收尾后继续接收迟到回调并产生 terminal invariant 错误 [fixing] (high)
- 复现: 新构建真实 CLI 的最新 `session.shadow_compared` 事件中，`typed_write_errors` 出现 `turn ... already terminal`，并出现 `assistant commit source step 18 != active source step Some(17)`、`tool results source step 19 != active source step Some(17)`；发生在模型传输失败/`process_restarted` 收尾后仍有回调写入的场景。
- 影响: 失败或重启后的 runner 回调继续向已 terminal 的 typed session writer 写事实，产生 writer 错误并污染 R-242 的 shadow gate；会话事实恢复与验收②⑤无法可靠判定。
- 来源: self-found：R-242 新构建真实 state.db 的 `session.shadow_compared` payload 与 `crates/kanzei-core/src/store/typed.rs:886-1143` writer 生命周期对照。
- 标签: 核心
- 验收: terminal 后任何迟到的 TurnStart/assistant/tool/text 回调均被安全忽略且不新增 typed_write_errors；失败/重启收尾只产生一个 terminal 事实；新增回归覆盖 terminal 后迟到回调；kanzei-core 定向测试通过。
- refs: R-242
- 优先级: P1
- 进展: 实现与定向验证完成，待提交后关闭：`crates/kanzei-core/src/store/typed.rs:920-923` 的 turn_started、`932-935` 的 push_text、`945-948` 的 flush_draft、`970-976` 的 flush_due、`981-984` 的 stream_restarted、`1005-1008` 的 assistant_committed、`1045-1048` 的 tool_results_committed、`1081-1084` 的 finish 均在 terminal 后短路；finish 首次收尾仍在 `1112-1116` 设置 terminal。回归 `typed.rs:1669-1728` 覆盖迟到 TurnStart/文本/stream restart/assistant/tool/flush/finish 且断言无错误、仅一个 TurnFailed；T-1786922726222（kanzei-core 223 passed）通过。下一步提交代码与 tracker/test archive，再关闭 D-487。
- observed_head: 7f77b8ffa4acd1556c893d05cdc61bd59a5773a5
- observed_worktree_hash: fnv1a64:71806aa445e9fad3
- recorded_at: 1786997535538
