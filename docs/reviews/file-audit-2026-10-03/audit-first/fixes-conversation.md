# 对话链路修复报告

整合状态：已在 dev 分组提交，未发布；提交与独立快照验证见 [修复汇总](fixes-summary.md)。

审计基线：`55eaca24750ac431ebe7f577c2f4b7b8cbded6f5`。8项已修复，通过下述回归；未提交、未发布。原审计报告保留历史。

## 修复内容

- **CONV-AF-01 / P1 — 插话原子交付**：新增 typed writer 的 take_pending_steers；同一 Immediate 事务完成整批提升、事实追加、完成回执，提交后才更新内存 invariant 和返回消息。
  验证目标：C7 第二条事实拒绝、input completion 拒绝：整批零交付、零事实前缀、输入保持 pending。
- **CONV-AF-02 / P1 — 当前交付结果与历史诊断分离**：协调器只使用本次 take_pending_steers 的 Result；旧 errors 保留供诊断，不再用它拒绝后续成功交付。
  验证目标：C7 已恢复 draft 拒绝：历史错误仍保留，三条插话正常到达真实 mock provider。
- **CONV-AF-03 / P1 — 停止隔离到本轮令牌**：inbox 捕获本轮 halt token；生命周期锁内先检查本轮取消。core 建流边界取消优先，覆盖步首检查之后发生的停止及建流等待。
  验证目标：C7 StopBeforeTake：旧 slot 清空也不吃新输入；仅1次 provider 请求，新 steer-after-stop 保持 pending。
- **CONV-AF-04 / P1 — 历史修改复用执行 owner**：clear、delete、rewind/fork/preview 在状态库+session 上获取已有 session_execution owner，并持有到操作结束。
  验证目标：真实 App clear/delete 命令在 runtime 空闲但 owner 已占用时拒绝且零 reset；释放 owner 后 clear 正常。
- **CONV-AF-05 / P1 — 批次归属不吸收无关修改**：专用写入路径解析收为单一 helper；observe 只允许本次成功专用写入更新所属内容，其他变化永久失去 owned 标记。
  验证目标：外部修改→无关 read→closing stage/finalize 均拒绝；失败写入造成变化也不能声明归属。
- **CONV-AF-07 / P1 — 自动 push 有界且可取消**：60秒上限、禁交互认证、本轮取消、kill_on_drop；独立清理任务持有 child，外层 watchdog abort 也触发进程树清理。复用现有 kill_tree。
  验证目标：真实 Windows 父/子进程分别验证取消、8秒测试超时、caller abort 后均死亡；本地 remote push 成功回归。
- **CONV-AF-08 / P2 — 删除错误的免测建议**：不再将 Git 输出当源码指纹，不再声称全量测试可省；Git 重复提醒只报告输出相同。
  验证目标：git status→test→edit→相同 status→test，不出现免测提示。
- **CONV-AF-09 / P2 — 按运行恢复增量回放**：新增 replay 查询按 run_id 聚合全部 chunk，limit 按轮数；legacy 无 run_id 独立保留。parser 跳过不相关残缺事件、按 started 顺序配对、去重补写。
  验证目标：分离 started/completed、补写重复、混入其他 run、limit=1/0、反序 completion、无 kind metadata 回归。

## crates/kanzei-core/src/store/inbox.rs

### 职责
输入准入及状态转换

### 判断
P1（修复 CONV-AF-01）

### 确切问题
插话原子交付

### 修改
提升原语可参与调用者事务，原 public promote API 保持。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
C7 第二条事实拒绝、input completion 拒绝：整批零交付、零事实前缀、输入保持 pending。

## crates/kanzei-core/src/store/typed.rs

### 职责
typed 事实与内存 invariant 的写入 owner

### 判断
P1（修复 CONV-AF-01）

### 确切问题
插话原子交付

### 修改
新增原子插话交付 API；诊断与当前 Result 分离。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
C7 第二条事实拒绝、input completion 拒绝：整批零交付、零事实前缀、输入保持 pending。

## crates/kanzei-core/src/store/events.rs

### 职责
事件查询与日志边界

### 判断
P2（修复 CONV-AF-09）

### 确切问题
按运行恢复增量回放

### 修改
新增分组回放查询；原 raw trace 查询未改。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
分离 started/completed、补写重复、混入其他 run、limit=1/0、反序 completion、无 kind metadata 回归。

## crates/kanzei-core/src/replay.rs

### 职责
将轨迹转成无副作用回放步骤

### 判断
P2（修复 CONV-AF-09）

### 确切问题
按运行恢复增量回放

### 修改
按 started 顺序恢复并容忍无关缺字段事件，去重补写。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
分离 started/completed、补写重复、混入其他 run、limit=1/0、反序 completion、无 kind metadata 回归。

## crates/kanzei-core/src/runner/drive/batch.rs

### 职责
实现批次的文件归属与交付窗口

### 判断
P1（修复 CONV-AF-05）

### 确切问题
批次归属不吸收无关修改

### 修改
跟踪本次成功写入集合，其他内容变化撤销归属。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
外部修改→无关 read→closing stage/finalize 均拒绝；失败写入造成变化也不能声明归属。

## crates/kanzei-core/src/runner/redundancy.rs

### 职责
工具结果重复行为提示

### 判断
P2（修复 CONV-AF-08）

### 确切问题
删除错误的免测建议

### 修改
删除无法成立的测试可省提示。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
git status→test→edit→相同 status→test，不出现免测提示。

## crates/kanzei-core/src/runner/drive.rs

### 职责
runner 步进及 provider 请求边界

### 判断
P1（修复 CONV-AF-03）

### 确切问题
停止隔离到本轮令牌

### 修改
建流与重试等待响应取消，已取消时 provider future 不先被轮询。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
C7 StopBeforeTake：旧 slot 清空也不吃新输入；仅1次 provider 请求，新 steer-after-stop 保持 pending。

## crates/kanzei-app/src/conversation.rs

### 职责
清空与删除对话历史的桌面入口

### 判断
P1（修复 CONV-AF-04）

### 确切问题
历史修改复用执行 owner

### 修改
新增跨进程 session owner 护栏。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
真实 App clear/delete 命令在 runtime 空闲但 owner 已占用时拒绝且零 reset；释放 owner 后 clear 正常。

## crates/kanzei-app/src/conversation_actions.rs

### 职责
回溯、代码恢复与分叉入口

### 判断
P1（修复 CONV-AF-04）

### 确切问题
历史修改复用执行 owner

### 修改
同一会话 owner 覆盖预览、计划验证与修改全程。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
真实 App clear/delete 命令在 runtime 空闲但 owner 已占用时拒绝且零 reset；释放 owner 后 clear 正常。

## crates/kanzei-app/src/run/coordinator.rs

### 职责
运行装配、插话投递与收尾协调

### 判断
P1（修复 CONV-AF-01）

### 确切问题
插话原子交付

### 修改
复用原子交付 API 和本轮令牌，不扫描历史 errors 判断当前结果。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
C7 第二条事实拒绝、input completion 拒绝：整批零交付、零事实前缀、输入保持 pending。

## crates/kanzei-app/src/run/mod.rs

### 职责
运行公用入口与自动 push

### 判断
P1（修复 CONV-AF-07）

### 确切问题
自动 push 有界且可取消

### 修改
有界 push 生命周期与真实 Windows 子树回归。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
真实 Windows 父/子进程分别验证取消、8秒测试超时、caller abort 后均死亡；本地 remote push 成功回归。

## crates/kanzei-app/src/run/coordinator/steering_tests.rs

### 职责
C7 真实 runner/provider/SQLite 故障注入回归

### 判断
PASS（必要 caller / 回归配套）

### 确切问题
未发现有实际影响的问题；此处只承接已确认根因修复。

### 修改
迁入已有五个 C7 正向合同断言，仅适配新 inbox helper。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
C7 第二条事实拒绝、input completion 拒绝：整批零交付、零事实前缀、输入保持 pending。

## crates/kanzei-app/src/conversation_tests.rs

### 职责
历史入口回归

### 判断
PASS（必要 caller / 回归配套）

### 确切问题
未发现有实际影响的问题；此处只承接已确认根因修复。

### 修改
补 runtime 空闲但跨进程 owner 占用的拒绝与释放测试。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
真实 App clear/delete 命令在 runtime 空闲但 owner 已占用时拒绝且零 reset；释放 owner 后 clear 正常。

## crates/kanzei/src/cli/eval.rs

### 职责
历史工具轨迹评估入口

### 判断
P2（修复 CONV-AF-09）

### 确切问题
按运行恢复增量回放

### 修改
消费按轮聚合后的 trace。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
分离 started/completed、补写重复、混入其他 run、limit=1/0、反序 completion、无 kind metadata 回归。

## crates/kanzei-tools/src/lib.rs

### 职责
工具 crate 公共导出

### 判断
PASS（必要 caller / 回归配套）

### 确切问题
未发现有实际影响的问题；此处只承接已确认根因修复。

### 修改
仅再导出既有 kill_tree，未复制进程终止原语。

### 影响范围
直接 caller 已检查；不改数据库 schema/持久化字段。跨模块新增 API 见上文。

### 验证
真实 Windows 父/子进程分别验证取消、8秒测试超时、caller abort 后均死亡；本地 remote push 成功回归。

# Module Summary

## 已修复
- P0：0
- P1：6（AF-01/02/03/04/05/07）
- P2：2（AF-08/09）

## PASS 文件
原审计 42 个 PASS 文件没有因风格被修改；必要 caller 与回归在上文列出。

## 仍需人工判断
无。

## 依赖影响
- typed writer 新增原子 take_pending_steers；desktop 调用。
- events 新增 list_replay_trace_payloads；CLI eval 调用，旧 raw API 保持。
- tools 再导出既有 kill_tree 供 desktop push 清理。
- core provider 建流边界响应已有取消令牌。

## 验证
core 最终共享源码重跑 424 passed / 0 failed（automation 日志）；app 全量 624 passed / 0 failed；C7 单独复跑 5 passed / 0 failed。CLI 测试目标编译成功（含 eval caller），quarantine 定向 2 passed。各自改动的 rustfmt --check、git diff --check 均通过。最终 workspace check 由 automation 统一执行。

后续工具代理仅调整 preview owner guard；其增量验证由 automation 执行，不影响本报告文件的已验证 hash。

日志：`output/audit-first/conversation/fix-core-tests.log`、`fix-app-tests.log`、`fix-c7-proof.log`；精简状态证据：`fix-c7-proof-summary.json`。

## 剩余风险
Windows 真实进程树已验证；Linux 现有 kill_tree 尚未实现子树终止，本轮未验证 Linux。
