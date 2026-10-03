# 第五轮并行审查、修复与整合

本轮沿执行 owner、文档真源、持久化收尾，再到会话和工作区 UI 推进。A4/C4/D2/M4/A5/C5/D3/M5/D4/A6/M6/D5/D6/B4/A8/C6 已按依赖方向合入；整仓第一次测试暴露的后台停止问题由 B4 回到底层修复。全文覆盖、必要切片和测试结果分开记录。A 家专属按用户要求暂缓。

统一验证以源提交 `0e0b24f86ea3960314dbf884a5ce6a21dd8ba27c` 开始，**17项全部通过**。Rust **2427通过、0失败、7忽略**；新的app/CLI构建、真实原生owner/偏好/启动回归及完整UI/浏览器已通过。全文覆盖以本次源证书与叶子审查核对后更新，不把正在准备的包计为完成。

## 实际修复与依赖关系

| 包 | 真正改变的行为 | 新增全文数 | 逐文件记录 / 地图 / 证据 |
|---|---|---:|---|
| C4 | 会话恢复/准入先取得数据库与 session 执行 owner；CLI 消费真实队首 input，拒绝终态提交返回失败 | 2 | [记录](parallel/C4.md)、[地图](parallel/C4-map.md)、[验证](parallel/C4-verification.json) |
| D2 | team 的 worker/attempt 在同一 owner 生命周期内；另一 profile 的只读检查不能恢复或重新执行活动子任务 | 0，重开 B2 切片 | [记录](parallel/D2.md)、[地图](parallel/D2-map.md)、[验证](parallel/D2-verification.json) |
| A4 | 主题初始化、偏好写回和 stream pane 绑定真实 owner，异步旧结果不能覆盖新状态 | 2 | [记录](parallel/A4.md)、[验证](parallel/A4-verification.json) |
| M4 | 会话操作绑定原项目/原会话/导航代次，失败恢复与刷新不吞掉结果；main 的 scheduler 同根问题由 A5 修 | 2 | [记录](parallel/M4.md)、[验证](parallel/M4-verification.json) |
| A5 | scheduler 只由 service/embedded 启动；定义与 revision 同读/CAS；终态拒绝有真实 Failed 收尾，UI 消费原项目结果 | 1，24-schedules 已在旧轮全文审查 | [记录](parallel/A5.md)、[地图](parallel/A5-map.md)、[验证](parallel/A5-verification.json) |
| C5 | run 终态/input/session stage 同一事务提交；Stopped 回执只发停止通知并跳过成功副作用；压缩提交后才发布 | 1 | [记录](parallel/C5.md)、[地图](parallel/C5-map.md)、[停止通知补充](parallel/C5-stop-notification.md) |
| D3 | 全局 Deny 的目录筛选保留实际资源级 Allow/Ask 例外，不移除合法工具通道 | 5 | [记录](parallel/D3.md)、[验证](parallel/D3-verification.json) |
| M5 | status 词表与实际 memory 状态一致；标题括号只消费一次，不丢原 status/severity 或标题文本 | 4 | [记录](parallel/M5.md)、[地图](parallel/M5-map.md)、[验证](parallel/M5-verification.json) |
| D4 | conventions、handoff、repair 全文审查 PASS，源码原样 | 3 | [记录](parallel/D4.md)、[地图](parallel/D4-map.md)、[验证](parallel/D4-verification.json) |
| A6 | 布局/工作区迟到初始化不覆盖新操作；各窗口按触及字段在原锁内合并，保留其他项目和课题 | 5 | [记录](parallel/A6.md)、[地图](parallel/A6-map.md)、[验证](parallel/A6-verification.json) |
| M6 | DocStore 只有 NotFound 可以当空状态；原文读取失败禁止 ID 分配/恢复/写入；提交后的检查错误保留真实写入事实；无终态时也落盘重复字段清理 | 3 | [记录](parallel/M6.md)、[地图](parallel/M6-map.md)、[验证](parallel/M6-verification.json) |
| D5 | 被禁用工具的拒绝提示与实际目录一致，避免指引模型调用不存在的通道 | 3 | [记录](parallel/D5.md)、[地图](parallel/D5-map.md)、[验证](parallel/D5-verification.json) |
| D6 | areas、auto_run、tool_pipeline 全文 PASS，保留现有职责和行为 | 3 | [记录](parallel/D6.md)、[地图](parallel/D6-map.md)、[验证](parallel/D6-verification.json) |
| B4 | stop只有实际进程/reader/final guard/恢复收尾结束才返回成功；恢复失败传播并保留重试状态，成功收据阻止旧baseline再次覆盖合法文件 | 1，lifecycle.rs；另4个必要切片不计全文 | [记录](parallel/B4.md)、[地图](parallel/B4-map.md)、[验证](parallel/B4-verification.json) |
| A8 | 偏好读取失败禁止默认状态覆盖；导出目录每次独占取得，空包只清自己目录；真实 camel 输入与历史 snake 输入均可用 | 1，prefs 重开不重复计数 | [记录](parallel/A8.md)、[地图](parallel/A8-map.md)、[验证](parallel/A8-verification.json) |
| C6 | 启动错误收尾实际 input 与当前 writer；初始 user 准入 ACK 拒绝后不执行 provider；多 Prompt 共用 turn 时后续写 steering 并检查 ACK | 1 | [记录](parallel/C6.md)、[地图](parallel/C6-map.md)、[验证](parallel/C6-verification.json) |

同一个 root cause 不因跨文件、多个 caller 或补充验证重复计数。M4 main 与 A5 timer owner 同根；C5-stop-notification 收尾 C5 停止事实，不另算一个新的运行根因；D2 重开已有全文文件，不重复增加覆盖。

## 文件级审查和 PASS

各包报告对每个实际审查文件使用“职责 / 判断 / 确切问题 / 修改 / 影响范围 / 验证”六项。公共 [coverage](coverage.json) 和 [任务队列](parallel-queue.json) 只在叶子证据及统一验证核对后更新。索引是 Rust/JS/MJS 的词法定位，不是经过类型解析的完整调用图，也不包含全部 Cargo 配置、HTML/CSS、脚本和资产。

本轮新增全文 PASS 文件：C4 的 session_execution_owner 测试文件；D3 lib/context/read_ledger/tool；M5 memory lib/render；D4 conventions/handoff/repair；A6 surface/frame；D5 tool_search/tool_catalog；D6 areas/auto_run/tool_pipeline。必要 caller 切片只按切片记录。

| 范围 | 全文完成 / 索引 | 尚未全文 |
|---|---:|---:|
| base | 9 / 9 | 0 |
| harness | 32 / 32 | 0 |
| llm | 15 / 16 | 1 |
| core | 24 / 66 | 42 |
| memory | 7 / 28 | 21 |
| tools | 4 / 126 | 122 |
| CLI / app 服务 | 30 / 146 | 116 |
| UI | 11 / 91 | 80 |

累计 **132/514**，本轮新增 **37**；剩余 **382**。入口/界面合算一层的七层口径，base 和 harness 两层全文完成，另五层仍有未审支线。llm 的一项 A 家专属按用户要求暂缓，仍保留在分母。

## 确切状态 owner 与跨模块合同

- `StdFileLock` → canonical database/session owner → CLI/app 准入和 team worker；锁保持到所有旧 worker、回调和清理结束。它没有替换数据库事件事务。
- `TypedSessionWriter` 原子终态/input/session stage → `PersistedRoundOutcome` → app/CLI/scheduler 真实终态回执与 UI。提交失败保持具体错误，不退回旧 turn 或发成功通知。
- DocStore 原文件/既有文件锁 → checked read 与解析 → 专用写端 → 原子替换 → 写日志记录；动作已经写入和动作完全没有提交分别返回真实语义。
- app.json 原锁 → workspace delta 合并 → 偏好缓存/导航代次 → UI 布局与 workspace。`None`、明确 `null` 和缺失字段沿实际字段合同区分。
- harness 原权限模型 → materialize 工具目录 → dedicated tool denial hint；目录优化不覆盖资源级权限门禁。
- 后台Arc/共享completion → 实际reader和final guard结束 → baseline锁内恢复+成功收据 → ProcessTool stop/重试/历史prune；无registry锁跨await，不能回放成功清理后的旧baseline。
- prefs 原始文件 → checked write-load → 十个实际 writer；只读默认回落不授权写入。ExportOptions 两个 UI nested 参数与旧 snake alias 一起适配，导出包归实际成功 create_dir 的调用方。
- typed 当前 append 返回值 → app/CLI/scheduler 初始用户准入 → provider；startup 持有实际 input/当前 writer，原子结果拒绝的 marker 保护上层 fallback，旧错误列表只作为诊断。

没有引入 framework、第三方依赖、SQLite schema 迁移、持久格式重写或新的默认业务政策。公共 API/字段的变化及 caller 清单在各包地图和验证记录里列明。

## 原错误、失败路径与恢复证据

| 实际验证 | 结果 | 原始证据 |
|---|---|---|
| 当前 Windows Rust 全工作区 | 2427 passed / 0 failed / 7 ignored，0 filtered | `output/parallel-M5/workspace-test.log`、最终证书 |
| check / Clippy / fmt / diff | 全工作区/all-targets、-D warnings、格式与差异均 exit0 | 17项 checks 的对应日志/SHA |
| 实际双 profile 子任务 owner | 14 项检查通过，两个自有进程均退出、errors为空 | `native-root-confirmation.json`、实际native JSON |
| 新 app 偏好/导出 | 11 PASS / 0 FAIL / 0 errors；源和隔离副本 binary SHA 相同，服务正常关闭 | `native-prefs/native-result.json` |
| 新 CLI 启动/准入 | 7场景通过；拒绝 user 后0新增POST，多Prompt正确持久steering和step | `native-startup.json` |
| UI状态/会话/调度/surface/权限 | 真实ES模块定向回归及IPC检查均exit0 | state-ui/sessions-ui/schedules-ui/surface-ui/permission-ui/ipc-event日志 |
| 完整UI运行时与浏览器 | 同一命令exit0；具体视图、模块和实际浏览器输出原文保留 | `ui-runtime.log` |

Rust 的 7 项包括5项原有忽略及D2新增的两个子进程fixture入口。这两个入口由实际父测试带 `--ignored` 调用，不能说成整包新增两项未执行测试。

统一差异检查第一次exit2只因C6报告末尾多空行。旧5项检查JSON、源快照、失败日志已独立保留；仅修该文档EOF字节后，核对所有其余tracked源/fixture/helper逐字不变，保留前4项真实成功结果，再从diff起完成13项。续跑driver前后SHA与两个快照关联在 `document-only-resume-provenance.json`，没有为文档重跑2427项源码测试。

负对照只认可“旧生产逻辑编译成功后，真实状态/文件/回执断言失败”；编译错误、fixture 错误和正常通过不算旧 bug 证据。所有临时替换使用原字节快照与 finally 恢复，日志和 source SHA256 均保留。

- 第一次统一 workspace test exit101：后台进程测试删除临时目录遇到 Windows sharing violation。原日志/JSON 在 `output/parallel-M5/integration-first-failure-*`，校验在 `integration-first-failure-manifest.json`。旧测试单跑 exit0 只是诊断，没有把它当作停止合同正确的证明。
- B4 旧生产实际证明：停止已返回后释放旧 final guard，后续合法写入真的被回滚；另一个 quarantine 故障控制证明恢复失败仍返回 `Ok(true)`。没有只用 Future 是否完成判断问题。
- M6 的初次目录/git fixture 和 MiKTeX profile 初始化错误均保留；把 TEMP 放在自己的非 Git 目录、复用已初始化 cache 后重跑，没有修改 memory 或 LaTeX 产品逻辑来迁就 fixture。
- A6 完整 workspace 浏览器断言对旧/新源码都因固定英文状态词失败。独立 [UI 验证修复](parallel/UI-workspace-check.md) 改为当前语言的精确 canonical 状态词；真实 workspace 浏览器链重新通过。这一项按验证工具 P2 单列，不计产品 root cause 或生产全文数。
- B4 首次挂起是测试同步 hook 阻塞 Tokio worker 的夹具错误，原日志保留；按实际多线程 runtime 用 `block_in_place` 保留同一 final reconcile 屏障，不把它算作新的生产 deadlock。
- B4 合入前还用真实文件抓到补丁的重复清理回归：成功 stop 后合法写入，再 stop/reaper 会回放旧 baseline。按既有 completion 成功收据收口，失败补偿在原 baseline 临界区复核、恢复和发布收据；此修复迭代不另计一个历史 root cause。
- A8 首扩展 native 使用了 desktop-only 命令，被无界面 service 按设计拒绝。四条 fixture 合同断言单独保留，没有当产品失败；新原生回归只使用正式支持的入口，工具打开保存等 writer 用真实 Rust 调用验证。

`output/parallel-M5/root-integration-proof.json` 记录最终源指纹、统一命令日志与 SHA256；`native-root-confirmation.json` 核对新源 debug binary、真实两个 profile 的状态与自有 PID 收尾。D2/M6/D6 等子树证据先原字节复制回主目录，再复用工作树；逐次复制 manifest 独立保留。

这次统一验证在开始时固定全部 tracked 输入、五个执行 helper 和实际 native-team 断言脚本，每步确认源未变化；新 app/CLI 二进制分别绑定该输入快照。最终证书只使用本次构建和本次结果。隔离探针已实际证明输入变化会拒绝认证，恢复后才接受；更新覆盖前再次核对全部生产源指纹。不能把旧日志或旧二进制贴上当前 HEAD 当成通过。

## 提交、工作树和后续队列

本轮源/独立验证共有 **19条本地整合提交**，完整哈希在 [本轮证书](parallel-round-5-verification.json)。最终构建源为 `0e0b24f86ea3960314dbf884a5ce6a21dd8ba27c`；报告、coverage、地图提交后用compare-and-swap将main同步dev，实际同步哈希见 `output/parallel-M5/main-dev-sync.json`。没有push或发布。四个工作树持续复用：主目录、audit-a0、audit-b0、audit-c0；A7/B5/C7尚在子树的准备改动保留。

下一轮已经继续：A7在五个已分配源内修原项目/原process字段delta、迟到UI结果和后台priority参数一致性；B5准备验证真实带连字符的进程ID在历史prune后的幂等stop；C7只准备实际插话SQL/runner探针；M7真实memory file/index/retrieval探针尚未执行。它们均未纳入本轮已修或全文完成。Cargo/共享target始终只有一个运行者，source/Node读取验证可并行；验证队列root整合 → B5 → A7 → C7。

# Module Summary

## 已修复

- P0：4个产品根因。M6文档坏来源写入擦除；B4停止后旧guard回滚合法文件；A8偏好坏来源覆盖与导出目录复用覆盖/误删。
- P1：32个产品根因，逐包去重清单见证书与对应文件记录。
- P2：4个产品根因。M4两个失败恢复问题、M6重复字段清理漏落盘、D5拒绝补救routing错误。
- UI验证脚本的语言假失败另计验证工具P2=1，未混入产品40根因。

## PASS 文件

- 本轮新增全文 PASS 清单见上文及各包六项记录；仅必要切片通过或编译通过的文件不冒报全文。

## 仍需人工判断

- 无。本轮已修和下一条已确认运行合同问题不需要产品决策。

## 依赖影响

- 见上文真实 owner/提交合同及各包 map。按底层到上层适配 caller，旧公开 bool/usize 保留的边界单独注明。

## 剩余风险

- 382 个词法源码文件尚未完成全文审查；活跃包未验证的源码不能计为已修。
- 当前验证为 Windows。没有运行 Linux、真实 Tauri 多窗口或物理手机/LAN E2E；浏览器/Node 结果和实际无界面 native profile 结果各自只证明所覆盖合同。
- 外部编辑器可绕过内部文件锁的既有边界保持；没有声称阻止非协作写者。
- D2 cleanup-failed 后 crash/restart 丢失 baseline 的路径无法凭空证明恢复，继续 fail-closed 并保留错误与证据。

此前自动审批拒绝删除四个 C2 测试目录及一个 C4 临时目录，理由为 `blocked by policy`；保留这些旧测试工件，没有绕过拒绝。
