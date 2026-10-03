# 第四轮并行审查与整合

本轮完成 D1 配置底层、C3 消息提交确认、M3 配置命令与 A3 权限 UI 的统一整合。新增 **15 个全文审查文件**，累计 **95/512**；剩余全文 **417**，其中28个已查必要合同切片、389个尚未审查。全文审查、切片、测试通过分别记录，没有把测试数当 coverage。

四个代码提交按依赖顺序合入 dev：D1 `04b33f61`、C3 `aa14eb0e`、A3 `0e5355c6`、M3 `b2983292`。原始子树提交分别为 `585cde3b`、`3a36b0de`、`9db202a2`；全部相关源码与子树提交在 CRLF 规范化后逐字相等，M3按本树验证/提交内容记录。main 在本轮报告提交后同步至同一验证快照，未推送或发布。

## 修复结果

按确切根因去重，本轮 **P0 0 / P1 7 / P2 1**：

1. **P1 配置共同事务**：lower、settings、项目模型、权限追加/删除、bootstrap 共享同路径锁，读改写与原子提交在一把锁内。旧双读/重试与重复裸写 helper 删除。
2. **P1 显式 cadence**：用户明确写入的 verify_every_n 现在进入 overlay 与运行配置，默认争议没有改动。
3. **P1 权限规则表示**：合法内联 rules 数组和 permissions 内联表可以精确追加允许规则。
4. **P1 权限身份**：删除携完整 expectedRule，事务内核对；UI 列表/确认/重试/撤销绑定项目，后台记住使用真实事件主根。D1、M3、A3按同一根因计一次。
5. **P1 持久提交确认**：assistant/工具声明拒绝时不执行工具；工具结果拒绝后不继续请求模型。desktop、CLI、scheduler 三个同步持久消费者返回实际接受结果，stateless 成功语义保留。
6. **P1 合法内联配置编辑**：模型和全局各节原本能加载却无法编辑；现在沿用 TableLike 支持两种表示，不改变值或字段。
7. **P1 最终 provider/模型一致性**：旧校验认可已被表单删掉的 provider；现在校验同事务内将提交的真实配置，失败零提交。
8. **P2 查询失败语义**：规则读取/解析失败不再伪装空列表，只有 NotFound 返回空。

`xhigh` 原实现已支持，未算问题。没有因缺少测试、理论碰撞、样式或未来扩展修改代码。

## 文件级记录

| 包 | 全文 | 必要切片及 caller | 记录 |
|---|---:|---|---|
| D1 | 8 | config loader、全部配置 writer 与实际 cadence 消费者 | [D1.md](parallel/D1.md)、[地图](parallel/D1-map.md)、[验证](parallel/D1-verification.json) |
| C3 | 3 | event/typed 两提交方法、desktop/CLI/scheduler sinks | [C3.md](parallel/C3.md)、[地图](parallel/C3-map.md)、[验证](parallel/C3-verification.json) |
| M3 | 2 | coordinator权限 emit、state_tests 两直接 caller | [M3.md](parallel/M3.md)、[地图](parallel/M3-map.md)、[验证](parallel/M3-verification.json) |
| A3 | 2 | 两个实际 delete invoke、native来源链、UI脚本fixture | [A3.md](parallel/A3.md)、[验证](parallel/A3-verification.json) |

五个 PASS 全文文件均来自 D1：config/embeddings.rs、web.rs、limits.rs、models.rs、permissions.rs，未因审查而修改。M3 state_tests 只是 PASS caller 切片，不增加全文数。

## 实际验证

- Windows `cargo test --workspace -- --quiet`：**2336 passed / 0 failed / 5 原有 ignored，16 suites**。该组合包含之前已验证 M2/D0 和本轮四包。
- workspace all-targets check、Clippy `-D warnings`、Cargo fmt、diff 检查全部 exit0。
- IPC：前端184个 invoke 全部在201个已注册命令内；后端/前端36个事件集合相等。
- UI：90模块、4014次 mock invoke、0运行错误；完整浏览器链、QA61、文件18、agent-team38、输入资源24、对话70及两主题合同通过。A3 12个项目/身份行为回归在主树再次全部通过。
- 原生构建成功：kzapp.exe 82754048 bytes，SHA256 `23cf09e9c37f977f20d4a425a3050f1ef7b0e1154dc25109b8a75ede9370baf0`；本次只是验证用debug构建。
- D1三个等价旧逻辑控制、C3两个拒绝实链、M3九组控制（10条失败断言）、A3固定旧源码12条行为断言均证明原错误。编译错误、正常通过或测试fixture失败没有冒作负例。
- M3初版内联转换、键装饰覆盖以及遗漏测试导入的失败日志保留；修正后定向、负例恢复后的整仓检查及 UI 全部重新通过。
- Cargo 单一持有者，切换树前刷新全部 Rust 源码 mtime；应用/home/AppData隔离，工具链与现有 browser cache复用。没有新增下载或第三方依赖。

证据在 `output/parallel-M3`（各日志与JSON/SHA256），子包证据复制到 `output/parallel-D1/C3/A3`，逐包机器记录在上表。没有执行 Linux、真实桌面窗口或物理手机/LAN E2E；browser mock结果不替代这些边界。

## 依赖地图进度

| 层 | 全文完成 | 剩余全文 | 状态 |
|---|---:|---:|---|
| 1 base primitive | 9/9 | 0 | 全量完成 |
| 2 harness / 配置基础 | 19/32 | 13 | 部分完成 |
| 3 llm 通用传输 | 15/16 | 1 | A家专项按用户指示暂缓 |
| 4 core state / runtime | 22/65 | 43 | store15/26、runtime7/39 |
| 5 memory | 0/28 | 28 | 待进入 |
| 6 tools / agent | 3/126 | 123 | 通用tools多处已查切片，不冒报全文 |
| 7 API / entry / UI | 27/236 | 209 | entry23/145、UI4/91 |

目前仍只有一层全量完成；依赖安全前置满足的上层文件可以并行审查，未宣称整层全部稳定。原有 `问题.MD` 保持未跟踪、未读取、未修改、未提交。

## 新的真实发现与下一轮

- **C4 活跃 session owner**：desktop/CLI 同会话第二入口会把仍在运行的第一 turn 当崩溃恢复，C3能阻断拒绝后的执行，但不能保证合法第一轮完成。正实施恢复前的跨进程执行 owner，覆盖终态与输入收尾；另修 CLI 队首input ID与新argv prompt错配。
- **D2 team owner / B3原生证据**：在同 exe、两个受支持 KANZEI_HOME 的真实 runtime 中，B list 把 A 的 running rev0改为interrupted rev1，A首个模型socket仍挂起；B resume 又准入第二POST。独立核对同项目、同job、B follow-up和只读SQLite，按一条 P1 计，**尚未修复**。仅证明错误恢复与并发模型请求准入，不宣称观察到并发工具写入。见 [B3-native.md](parallel/B3-native.md) 和 [观察JSON](parallel/B3-native-observation.json)。
- **A4 UI基础**：03-shell与01-core依owner顺序，已在子树修主题hydrate覆盖、偏好缓存/写顺序、pane stream归属；正跑完整UI，未合入本轮95覆盖。
- **M4前端调用方/服务入口**：09-sessions、main.rs等上述owner稳定后继续，不能先在上层补多份状态逻辑。

仍是四棵工作树：主 dev、A树D2、B树A4、C树C4。Cargo 当前归C4，D2依C4原语后验证；root不并行运行第二份Cargo。

# Module Summary

## 已修复
- P0：0。
- P1：7条去重根因，上文逐条对应源码/真实caller/失败回归。
- P2：1条规则查询失败语义。

## PASS 文件
- D1五个全文配置域；必要caller切片单独记录，不重复全文。

## 仍需人工判断
- 无。当前修复与下一轮已确认问题均是运行合同错误，无产品决策需求。

## 依赖影响
- 配置全部协作专用writer共用lower事务；删除IPC必填expectedRule，权限event新增真实projectDir。
- message committed事件新增同步receipt，typed提交返回实际bool；三个durable caller全部适配。
- TOML/SQLite schema、状态持久字段、默认业务分工均未迁移。

## 剩余风险
- C4/D2当前已确认的活跃运行者归属问题尚在修复，不能把本轮测试通过当作全系统无bug。
- 非协作外部编辑器绕过内部锁的既有边界；未验证Linux/真实窗口/物理手机。
- 全仓全文审查剩417个，其他模块不能依据本轮check结果被标PASS。

四个C2负例临时目录仍未清理：自动审批拒绝删除，原因为 `blocked by policy`。只含测试数据库，不影响代码和上述验证；未重试或绕过拒绝。
