# C5 补包：停止后的通知与成功后处理

基线 `19d95cfa47519262f8519f0c63c7d55068c8c238`；仅 persistence 必要切片，不重计 C5 全文数和已记录根因。真实正负例及最终编译验证已完成：persistence 18项通过，旧通知发布块使两条实际Stopped回归断言失败；正常完成控制通过。证据独立位于 `output/C5-stop-notification`，原 C5 leaf 与日志不改。

## crates/kanzei-app/src/run/persistence.rs（必要切片）

### 职责
根据已经提交的轮末结果发布通知与可选后处理。

### 判断
P1

### 确切问题
- 实际 provider 成功后 Stop 先赢，typed/input 正确提交 Stopped/cancelled，原后处理仍写 succeeded 通知、手机“任务完成”并执行 after_success，向用户与后处理宣布不存在的成功。

### 修改
- 根据已经确定的 halted 结果发布 stopped/任务已停止及手机停止说明。
- Stopped 不执行 after_success；保留实际部分轨迹 harvest 与 halted episode。
- 正常成功保持 succeeded/任务完成和原成功后处理。

### 影响范围
- 唯一生产 caller coordinator；影响 agent_notifications、SSE/PWA展示、手机提醒与 memory inbox/candidate 成功后处理。
- 已有 AgentNotification String 状态、PWA stopped 字形与 UI halted 合同直接复用，无 public API/schema/持久格式变化。
- budget_result 没有找到实际控制消费，不单独修改；原原子事务和 lifecycle 锁边界保持。

### 验证
- 写前核对上述直接 caller、状态存储/回放与 PWA/UI 消费者。
- 实际 PWA formatNotice 的 Node 检查通过：stopped 显示 ■/任务已停止，succeeded 显示 ✓/任务完成。
- standalone rustfmt 与 diff 检查通过。
- 已扩展真实 200→Stop/合作停止夹具，检查持久通知 stopped、无 succeeded、手机停止正文、after_success 零次；正常 200 和完成先赢保持成功控制。
- 初次与精确恢复后的 persistence 全部18项定向通过，含真实200→Stop、合作停止、完成先赢、正常完成及既有失败/压缩控制；Stopped持久通知、手机正文、after_success零次均实际断言。
- 只还原原leaf通知/手机/after_success生产块，保留新夹具；编译成功后exit101，两条真实停止回归读到 succeeded（预期stopped）而失败，8项其它控制通过，包含正常200和完成先赢。
- 逐字恢复并核对SHA256 `d949743ab58b72e5c58f2a3adedea1fd9da88847bf47100b0654a008358ae29a`；刷新本树Rust mtime后重编译最终正例。app all-targets check、Clippy -D warnings、cargo fmt、diff检查通过。
- 本补包按授权只运行相关定向与编译检查，未重复全workspace/core/app全包/native UI E2E。

# Module Summary

## 已修复
- P0：0。
- P1：C5 已记录 outcome / Stop 根因的通知遗漏已修复并通过真实正负验证；新增独立根因 0。
- P2：0。

## PASS 文件
- 无新增全文审查或全文 PASS；只重开 persistence 的必要切片。

## 仍需人工判断
- 无产品/架构决策项。

## 依赖影响
- 无 cross-module public contract 变化；使用已有 stopped 通知与 halted UI 语义。
- 成功后处理只在真实正常成功时执行。

## 剩余风险
- C6启动失败链仍由下一包收口；本补包没有修改assembly或提前增加其全文审查数。
- 本补包证明实际持久函数/SQLite/provider/effect调用和PWA formatter；真实桌面/手机外部发送未另做native E2E，主树整合验证单独记录。
