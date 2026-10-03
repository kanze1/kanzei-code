# Sol 并行第三轮整合记录

本轮已完成并验证 C2、B2、A2、M2、D0。新增 **15 个全文审查文件**，累计 **80/512**，尚有 **432** 个未完成全文审查。30 个调用链切片单独记录，没有计为全文。代码修复提交到本地 dev，报告提交完成后将 main 同步到同一提交；不推送、不发布。

## 七层进度

| 层 | 全文已审 / 文件总数 | 状态 |
| --- | ---: | --- |
| primitive / base | 9 / 9 | 全量完成 |
| harness / 配置与准入 | 11 / 32 | 本轮稳定根身份与 agent 注册；配置链进入下一轮 |
| LLM / 协议与客户端 | 15 / 16 | A 家专属剩余范围暂缓 |
| core / 状态与运行 | 19 / 65 | store 15/26，runtime 4/39；提交确认进入下一轮 |
| memory / 文档与索引 | 0 / 28 | 只检查过必要 caller，尚无全文完成记录 |
| tools / 编排与子任务 | 3 / 126 | team 三文件完成，其余逐依赖推进 |
| CLI、服务与 UI | 23 / 236 | CLI/app 21/145，UI 2/91；权限界面进入下一轮 |

七层只有 base 全量结项。其他层已完成的主链和未审支线同时存在，不用测试数量代替审查覆盖。词法文件清单为 512 文件、272466 行，包含测试；它不是已解析的完整调用图，真实调用与状态边界证据在逐包报告。

## 文件级完成清单

| 包 | 新增全文文件 | 判断与实际修复 | 逐文件记录 |
| --- | --- | --- | --- |
| C2 | store/typed.rs、typed/projection.rs | P1：事务内检查权威终态、恢复和 seed；修复投影索引及来源身份/边界 | [C2.md](parallel/C2.md) |
| B2 | tools/team/mod.rs、store.rs、tests.rs | mod P1；store/tests PASS。统一 owner，收口停止、迟到回复、续做、采纳与持久失败 | [B2.md](parallel/B2.md) |
| A2 | runner/item_context.rs、item_context/tests.rs、runner/event.rs、CLI/run/events.rs | 来源合同三个文件 P1；tests PASS。拒绝基于旧历史的 work-context 覆盖，保留手机新输入 | [A2.md](parallel/A2.md) |
| M2 | app/mobile.rs | P0/P1：凭据持久成功后发布，单次配对，原子消息，唯一服务 owner，停服准入与端口释放 | [M2.md](parallel/M2.md) |
| D0 | harness/home.rs、project_root.rs、markdown.rs、defs.rs、refs.rs | 三个 P1：目录身份、gitfile 根发现、无效 agent 配置；defs/refs PASS | [D0.md](parallel/D0.md) |

重开切片包括 typed writer、store/events、store/inbox、app event sink、typed reexport、agent_team、MobileService 定义。这些没有重复增加全文数量。对应报告均给出职责、确切问题、修改、caller/兼容性和实际验证。

## 状态与依赖收口

```text
规范目录身份 → 项目/无项目根 → 配置与工具作用域
SQLite Immediate → 权威 touched-turn facts / seed 来源 → canonical projection
canonical source + sequence → work-context CAS → app/CLI candidate publication
mobile credentials commit → cache/鉴权状态
mobile receipt + user fact commit → canonical conversation cache → 成功响应
team registry owner → worker token/attempt → lifecycle → 持久状态与显式续做
MobileService slot → pair → devices → listener 停止与端口释放
```

新增必要合同：`SessionStore::append_mobile_message` 统一输入提交；`team::store_for_inspection` 消除检查与注册间的恢复窗口；`WorkContextPrepared`/writer 携带真实 source。全部直接 caller 已搜索和适配。没有 schema、持久字段、文件布局或第三方依赖变化。

## 验证

- Windows 全工作区：**2315 passed / 0 failed / 5 原有 ignored**，16 个 suite，含现有集成和 doctest。
- workspace all-targets check、Clippy `-D warnings`、Cargo fmt、diff 检查通过。
- IPC：184 个 frontend invoke 全部注册（总计 201 个命令）；36 个后端事件与 36 个前端订阅一致。
- 完整 UI VM 与浏览器链通过：90 个 UI 模块、4014 次 invoke、0 运行时错误；包括活动、问题回复、工作台、文档/决策、子任务、文件编辑等实际浏览器回归。
- 分包旧行为负例是真实状态/请求/effect 断言失败；错误源码恢复后才执行正向验证。M2 7 项、D0 3 项均 exit101，原字节与 SHA256 恢复一致；其他分包的数量和完整历史/等价旧逻辑区别见各 verification JSON。
- UI 首次整合因隔离 profile 缺 ffmpeg 失败；指定机器已有 Playwright 工具缓存后完整重跑通过，没有安装依赖或更改产品代码。原失败与最终日志均保留。

主目录原始日志与内容对照保留在 `output/parallel-{C2,B2,A2,M2,D0}`；每包 verification JSON 记录路径、命令、退出码、源码和日志 hash。单独分包的较少测试数不与整合总数混用。

## 本地提交

| 包 | 原树提交 | dev 整合提交 |
| --- | --- | --- |
| C2 | b9f3ae24 | b42b284f |
| B2 | 441e0c29 | f0dd9d57 |
| A2 | 95b7b99d | 10591a49 |
| M2 | 主目录直接修复 | e76d2449 |
| D0 | 主目录直接修复 | 65421388 |

报告提交 ID 可由本文件所属 Git 提交核查，main 与 dev 在本批完成后同步。仍是四条工作树：主目录，以及供 D1、A3、C3 继续工作的三条隔离树；没有为了分包增加树。原有未跟踪 `问题.MD` 未修改、未提交。

# Module Summary

## 已修复

- P0：1 个根因，撤销持久失败仍宣称成功。
- P1：19 个根因，按分包去重。C2 5、B2 6、A2 1、M2 4、D0 3。
- P2：0。没有把风格或缺测试记为 bug。

## PASS 文件

- team/store.rs、team/tests.rs、item_context/tests.rs、harness/defs.rs、harness/refs.rs。测试文件有必要的 regression 追加，PASS 不代表需要改生产。

## 仍需人工判断

- 本轮无产品决策项。下一轮确认的系统 bug 继续直接修。

## 依赖影响

- 仅上面列出的内部输入、恢复检查、候选来源合同。正常外部 IPC/HTTP 成功形状不变；持久失败正确返回错误。

## 剩余风险

- 本轮未复跑 Linux、实体手机 LAN、真实 Tauri 窗口 E2E。
- C3 正在修工具副作用前的 durable commit ack；恢复活跃运行的 owner 排他还未结项，不能宣称完整运行链已安全。
- 跨 portable/profile 的共享项目 owner 仍等待原生隔离复现。普通同 executable/home 的桌面窗口已有唯一 runtime service；第二轮曾将“多进程恢复”写成已确认，现纠正为待实测，未将其计为本轮修复。
- C2 四个负例测试临时目录保留：自动审批拒绝删除，原因 `blocked by policy`。只含测试库，不影响代码或验证；未再次尝试绕过。

## 下一轮

按下层到 caller 继续：D1 配置文档事务与显式 cadence → M3 settings/model 保存、规则删除/查询和 xhigh 保持 → A3 权限 UI 项目/请求身份。C3 并行处理提交确认 → drive → 工具；B3 等新二进制验证真实跨 profile owner。未合入且未整合验证的包不计进当前 80 文件。
