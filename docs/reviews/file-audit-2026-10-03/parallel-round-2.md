# Sol 并行第二轮整合记录

日期：2026-10-03。代码已整合到 dev `c9b83757`；本轮完成后本地 main/dev 快进到同一提交。没有推送或发布，原有未跟踪 `问题.MD` 未修改、未提交。

## 已完成

| 包 | 新增全文文件 | 已修根因 | 已整合提交 | 逐文件记录 |
| --- | ---: | --- | --- | --- |
| A1 | 3 | P1 ×3：未提交归档被 GC 删除；纯 L0 压缩未持久化/缓存先发布；旧来源覆盖新事实 | `a8c778f3` | [A1](parallel/A1.md) |
| B1 | 4 | P1 ×4：旧回复跨 stop 唤醒主对话；callback 冒充明确续做；停止漏掉准入中的 worker；日志立即重订阅复用关闭邮箱 | `a962248c` | [B1](parallel/B1.md) |
| C1 | 4 | P1 ×3：旧进程快照复活退役身份；合法路径别名读/注销失败；旧投递批次倒退游标 | `76aa80f7` | [C1](parallel/C1.md) |
| M1 | 4 | P0 ×1：通知使用 query 身份，导致跨设备游标污染和撤销失效 | `c9b83757` | [M1](parallel/M1.md) |

本轮新增全文审查 **15 个**，累计 **65/512**，剩余 **447 个**尚未全文审查。索引增加了 A1 的 artifact_liveness.rs；没有靠删队列降低待办。30 个索引内调用链切片不计全文完成，脚本切片另列于 coverage.json 的 outside_inventory_contract_slice_reviews。

| 依赖层 | 全文完成 / 索引文件 | 状态 |
| --- | ---: | --- |
| base | 9 / 9 | 全量完成；真实新证据才重开 |
| harness | 6 / 32 | 尚余 26 |
| llm | 15 / 16 | 尚余 1；A 家专属扩展暂停 |
| core | 14 / 65 | 尚余 51；store 13/26，runtime 1/39 |
| memory | 0 / 28 | 尚余 28 |
| tools | 0 / 126 | 尚余 126；已有关键切片不能冒充全文 |
| CLI/app 服务 | 19 / 145 | 尚余 126 |
| UI（入口层展示） | 2 / 91 | 尚余 89 |

原七层口径把 CLI/app/UI 合并为入口层：**1 层全部完成，6 层仍有未审支线**。这不是完成了六个或七个全层。

## 状态与依赖为什么这样修

- SQLite session_events 是会话真源。压缩必须证明候选来自原始消息，并在同一提交窗口校验 sequence；缓存只能在提交后发布。手动压缩仍要求空闲，没有因为运行内 CAS 的新增入口弱化手动门禁。
- 归档正文与 SQLite 引用跨两个边界。共享 publication 锁保护“已写正文、尚未引用”的完整生命周期；GC 在 SQLite 事务之前尝试同键独占，忙时明确拒绝。异步守卫不迁移 !Send 的系统文件锁，专属 blocking 线程从拿锁到释放始终拥有它。
- stop 关闭原 actor 的 mailbox；旧回复绑定原 generation，不能在 await 后改用新 actor。子任务明确续做与 durable callback 使用不同准入语义，callback 不获得用户续做权限。
- retired_processes 的退役事实优先于运行快照；Windows 既有路径形式复用唯一 normalization owner。设备/线程 cursor 只取最大值，显式请求的补发 cursor 不改变。
- 手机通知身份来自 bearer token，query 只能确认相同身份。标准 PWA 字段保持，伪造身份返回 403；SSE 撤销检查和 cursor 更新都使用同一个认证 ID。

没有新增第三方依赖、数据库 schema 或序列化字段；base 仍为零第三方依赖。A1 增加 store 的 checked run-compaction/publication API，所有直接 caller 已搜索适配；M1 的 SSE 身份参数为私有接口。

## 验证

- 整合后的 Windows 全工作区：**2275 passed / 0 failed / 5 原有 ignored**，16 个 suite，包含 CLI 集成与 doctest。不是把重复定向测试相加得出的数字。
- `cargo check --workspace --all-targets`、全工作区 all-targets Clippy `-D warnings`、全工作区格式检查与 diff 检查通过。
- IPC：184 个前端 invoke 均已注册；后端 emit 与前端订阅各 36 个，差集为空。完整 UI 冒烟执行 90 个模块、4014 次 invoke，0 运行时错误；其链式浏览器回归通过。
- A1 的 GC/纯 L0、B1 的五个 stop/旧回复等价旧逻辑，以及 M1 的三个真实 TCP 身份回归，都有修改前实际失败断言。C1 使用双连接、别名/退役和乱序 cursor 回归。详细 invariant 与命令见各包验证 JSON。
- 子树提交与 root 内容逐项归一化核对，共 28 项全文/切片证据；CRLF/LF 差异不算代码变化。A1 persistence 新增测试的五处断言改用已有 lock_or_recover，生产内容保持相同；此整合修正记录在 coverage.json。
- 共享 Cargo target 只由一个执行者使用；root 刷新 Rust 源 mtime 后全量重编，避免别树缓存被误当作本树验证。各包原始日志已复制到主目录 output/parallel-A1、B1、C1、M1。

两次验证命令问题已解决并保留日志：既有 D506 全文件文本检查命中新测试的 unwrap，修正断言后重新完整测试；Node 首次缺 vm-modules 参数，修正命令后完整 UI 通过。没有把这些命令/测试约束问题计成业务 bug，也没有隐藏首跑失败。

## 已确认的下一轮问题

以下已找到实际调用路径；尚未纳入本轮已验证提交，也不是产品决策：

| 包 | 文件/边界 | 真实问题与处理顺序 |
| --- | --- | --- |
| C2 | typed.rs → typed/projection.rs | 终态预检、恢复及 seed 来源在事务外，跨 writer 可使用旧状态；中断消息使两份历史索引不同，第二工具结果落错消息。先收口数据库事务与投影 |
| B2 | tools/team | 并发 attach 产生两个状态 owner；旧 worker 清理与 stop/合法续跑交错；旧 ask、adopt 与状态变更窗口；launch SQL 失败留下不存在的 active worker。接 C2 后验证 |
| A2 | runner/item_context → event → app/CLI → typed writer | archive 等待期间收到的新手机事实被旧 work-context surface 隐藏；必须携带真实 source，再做 source match 与 sequence CAS |
| M2 | mobile → devices/inbox → cache/UI | 配对码可被并发消费；凭据持久化失败仍报成功，撤销跨重启失效；手机 typed 写入失败仍 202/发布缓存；并发 start 留下无 owner 服务 |
| 后续 owner/执行链 | 多进程 team / typed commit → runner tools | 进程内 registry 不能替代跨进程 owner；独立 recovery 可以闭合活跃 turn，typed 拒绝 tool_called 后 runner 无 ack 仍执行副作用。须沿真实入口/执行合同继续修 |

后续仍按底层稳定后再改 caller；不同时编辑 typed owner，也不在多个入口堆补丁。没有需要用户逐批批准的事项。

## 当前验证边界

- 未重跑 Linux，未做 Android 真机 LAN 或真实 Tauri 窗口 E2E；相关边界不能由 loopback TCP/浏览器夹具替代。
- 本轮没有扫描用户真实数据库，不能宣称历史状态已修复或未曾受影响。
- A1 对来源不匹配的 pipeline 压缩保留原持久事实，候选不发布；不宣称这些压缩候选成功写入。
- 全库逐文件审查尚未结束；各层剩余范围可逐项查 coverage.json/parallel-queue.json。
