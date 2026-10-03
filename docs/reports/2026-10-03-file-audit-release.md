# 2026-10-03 文件级系统审查：发布解读

本版集中交付已经审查、修复、提交并组合验证的工作，包含底层锁/写入/路径/日志、会话和子任务状态、后台进程恢复、文档持久化，以及前端项目/会话/运行控件一致性。没有为了风格重写稳定模块，没有扩展 A 家专属支持。**整个仓库审查尚未完成：136/514 文件全文完成，剩余 378；七层中 base 和 harness 两层完成。**

## 发布范围和保留状态

上一发布为 `build-abb596f0`，其目标为 `abb596f0eb59ab32d13c32f0952aa6771a9b3e21`。本轮组合验证源码为 `5d01fb3fc0bdb3d60b3d10143244508a1b87cb22`，从上一发布到该源提交共 **50 个已核对提交**；随后加入本报告、覆盖地图与组合证书的文档提交。最终安装包必须绑定最后提交，按实际区间条数通过 package.ps1 的 Ack 检查。

root/dev 已整合第五轮和 B5/A7。main 只允许从已验证 dev 快进；发布前重新核对本地及 origin 的实际身份。保留另三个审查工作树：C7 输入队列、B6 后台日志与登记、A9 记忆 API/UI。M7 的未执行探针草稿也已保存，移出 Cargo 测试目录。原有 `问题.MD` 不读取、不修改、不提交。发布不自动证明安装完成，当前正在运行的用户应用保持运行。

## 依赖地图与真源

| 层 | 状态 owner / contract | 上层依赖与实际边界 |
|---|---|---|
| base primitive | 标准库文件锁、规范路径身份、统一字节指纹、原子替换、写日志编码 | memory/tools 共用；业务决定写什么，base 负责怎么写与报告失败 |
| storage / state | SessionStore SQLite 事件和输入；DocStore Markdown 文本；app.json 偏好 | 迁移事务、执行 owner、版本/CAS、损坏读取禁止按空状态覆盖 |
| domain | 文档状态/标题往返、权限、工具目录和上下文规则 | 保留状态词表和业务规则；工具目录不能覆盖实际资源权限 |
| orchestration | Runner、TypedSessionWriter、scheduler、team worker 生命周期 | 终态回执必须来自真实提交；锁/owner 保持到旧任务和清理结束 |
| tools / agent | 后台进程登记、reader、final guard、恢复凭据 | Stop 结束收尾才成功，恢复失败保留真实失败与重试状态 |
| API / service | app/CLI 准入、IPC、偏好原锁合并、启动/调度回执 | caller 必须消费实际原项目/原 session/当前 writer 和提交 ACK |
| UI / entry | 项目/会话/线路身份、导航代次、字段增量、迟到响应 | UI 回显不能成为新写入；关闭/退休不能被旧 ACK 复活 |

完整定位地图见 [dependency-map](../reviews/file-audit-2026-10-03/dependency-map.md)，逐文件职责、判断、修改、影响和验证见各轮/各包报告。索引为词法定位，不是经过类型解析的完整调用图。

## 本版主要修复和用户可见变化

### 1. 底层文件设施

- 锁使用系统共享/独占文件锁，只保留空闲、共享、独占状态，移除 30 秒过期抢锁和不可达 acquiring 特例；释放句柄即释放 OS 锁。
- CAS 在同一锁内完成检查与替换；rename 失败明确返回，保留完整临时文件，取消盲目重试。
- 相对路径、点段、链接等普通别名归一锁身份；修复 Windows 259/260 UTF-16 单元的 MAX_PATH 判断。
- 写日志明确区分合法空正文、只有指纹和删除；严格编号决定先后，同毫秒/时钟回退不会覆盖记录；保留路径最新恢复事实，500 条成为保留目标。
- 日志损坏/缺正文时明确报错，不从旧内容偷取恢复；源码凭据按实际工作树记录，避免同名路径串树；正文指纹和元数据换行也有真实回归。
- 内容指纹收敛到同一 primitive，已有建议稿按旧格式兼容，不额外创造另一套身份算法。

设计取舍及 caller 详见 [底层报告](2026-10-03-base-refactor.md) 和 [整合报告](2026-10-03-integration-and-runtime-fixes.md)。内部 CAS 串行化遵守锁协议的 writer；外部编辑器绕过锁写文件仍不受该互斥合同约束。

### 2. 会话、持久化和跨进程任务

- 会话恢复、准入和 CLI 消费先取得 canonical 数据库/session owner；实际队首 input、终态提交和 ACK 统一处理。
- typed run 终态、input、session stage 在同一事务提交。停止回执只发布停止结果，跳过成功通知/副作用；压缩提交后才发布结果。
- team worker/attempt 跟随原 owner 生命周期，另一 profile 的只读检查不能恢复并重新执行活动子任务。
- scheduler 由真实 service/embedded owner 启动，定义与 revision 同读/CAS；启动准入拒绝和调度终态拒绝有真实 Failed 收尾，不执行额外 provider 请求。
- 多 Prompt 共用一个 turn 时，后续以 steering 准入并检查 ACK；启动错误操作真实 input 与当前 writer，避免误恢复到旧版本。
- store 旧输入恢复进入迁移事务，旧 promoted 回填只对真实旧版本执行；事件/投影、通知游标及撤销边界在各包回归中核对。

主要证据见 [第五轮](../reviews/file-audit-2026-10-03/parallel-round-5.md)、[第一轮](../reviews/file-audit-2026-10-03/parallel-round-1.md)、[第二轮](../reviews/file-audit-2026-10-03/parallel-round-2.md)、[第三轮](../reviews/file-audit-2026-10-03/parallel-round-3.md)、[第四轮](../reviews/file-audit-2026-10-03/parallel-round-4.md)及其逐文件报告。

### 3. 后台进程和文件恢复

- Stop 等待实际进程、reader、final guard 和恢复收尾完成；失败传播真实错误并保留可重试状态。
- 成功收据阻止后来旧 baseline 重复回放，避免已经合法修改的文件被再次覆盖。
- B5 修复真实 `bg<ms>-<seq>` ID 自然结束并从内存回收后，Stop 误判非法的问题；合法已回收 ID 可幂等停止，非法格式仍拒绝。

详见 [B4](../reviews/file-audit-2026-10-03/parallel/B4.md) 与 [B5](../reviews/file-audit-2026-10-03/parallel/B5.md)。B5 的测试确实启动后台任务、读取真实 ID、等待结束、仅回收自身记录、调用真实 ProcessTool，没有用猜测 ID 替代。

### 4. 文档、权限与用户偏好

- DocStore 只有 NotFound 才能作为空来源；读取/解析失败禁止分配 ID、恢复或覆盖。已写入后的检查失败保留真实提交事实。
- 文档 status 词表、标题括号和 severity 在解析/渲染后保真；没有终态迁移时也能落盘重复字段清理。
- 工具目录保留资源级 Allow/Ask 例外，被禁工具提示使用实际可用通道。
- 偏好读失败禁止默认值回写；多个窗口在原锁内只合并触及字段，保留其他项目和线路。导出目录独占取得，空导出只清自己的目录；camel 与历史 snake 参数均可用。

### 5. 前端运行状态

- 初始化、工作区、会话、文件补全、SOP 和 hydrate 使用原项目/原会话/操作代次；迟到旧结果不能覆盖新操作或串项目。
- 布局、priority、线路偏好使用字段增量，避免跨窗口整张状态覆盖。明确 null 表示退休线路，空增量不删除其他状态。
- A7 每条线路的请求串行，失败后队列仍继续；关闭/暂停立即取消该会话本地续跑计时器，ACK 合并到当前状态，后台真实请求读取原项目已持久化 priority。

详见 [A6](../reviews/file-audit-2026-10-03/parallel/A6.md)、[M4](../reviews/file-audit-2026-10-03/parallel/M4.md)与 [A7](../reviews/file-audit-2026-10-03/parallel/A7.md)。界面结构没有大改，主要变化是状态更一致，失败更准确。

## 当前统一验证

本轮在当前组合源实际完成 **18 项检查全部通过**：Rust **2431 passed / 0 failed / 7 ignored**，16 个结果套件，无过滤。check/Clippy 均包含 all-targets；fmt、diff 检查通过。忽略项包含 5 个历史项及 2 个由父测试实际调用的子进程入口。

新编译 app/CLI 后，真实 native IPC 的 team 14 项、偏好/导出 11 项、启动准入 7 个场景通过；两套隔离 profile 使用同一个 app 二进制，所有自建 runtime 正常退出。UI 状态 15、会话 22、调度 8、surface 12、权限 12、运行控件 29 项、IPC、完整 runtime/实际浏览器通过。源码、二进制、原始日志均记录哈希。

B5 叶子 process 7 项、tools 799 项；A7 叶子 prefs 26 项、app 600 项、运行控件 29 项通过。旧生产行为的目标断言真实失败（A7 旧 UI 24 条、旧 prefs setter 3 条），正常控制保留。初始缺浏览器依赖/错误夹具的记录保存并区分，不能把它们记成产品 bug。

组合证书见 [第六轮验证](../reviews/file-audit-2026-10-03/parallel-round-6-verification.json)。最终发布还必须执行当前规范 `scripts/verify.ps1 -Full`，绑定最终文档提交、全项通过且无跳过，再打包 CLI+app、发布指定 SHA、独立重新下载核对大小/SHA256 和 HTTP Range 206。实际最终发布结果记于 `dist/release-receipt.json` 及版本页。

native 使用当前新编译调试程序，是实际后端 IPC 测试；不能称为用户安装后桌面 WebView2/手机 E2E。旧底层树 Linux 34 项仅为当时基础层证据，本版没有最新整仓 Linux 验证。用户程序正在运行，本次不自动替换安装位。

## 审查完成度、兼容变化与剩余问题

| 范围 | 全文完成 / 索引 | 剩余 |
|---|---:|---:|
| base | 9 / 9 | 0 |
| harness | 32 / 32 | 0 |
| llm | 15 / 16 | 1 |
| core | 24 / 66 | 42 |
| memory | 7 / 28 | 21 |
| tools | 5 / 126 | 121 |
| CLI / app 服务 | 31 / 146 | 115 |
| UI | 13 / 91 | 78 |

**136/514，剩余 378；2/7 层完成。**必要切片、编译通过的 caller 和未执行草稿不算全文；重开文件只记一次。[coverage](../reviews/file-audit-2026-10-03/coverage.json)和[队列](../reviews/file-audit-2026-10-03/parallel-queue.json)是当前完成范围，逐包报告保留历史时点，不应把旧“未发布”说明当成最新状态。第五轮本轮根因口径为 P0=4/P1=32/P2=4；本轮 B5/A7 新修 P1=4。没有将不同历史轮数简单相加作为全版本独立根因数量。

兼容变化：base 最低 Rust **1.89**，新旧程序须统一升级才能使用同一 OS 锁协议。没有新生产第三方库；app 测试使用已有 rusqlite。写日志引入 `data:`/`omitted`/`deleted` 状态及 `v3:` 换行元数据编码，读端保留旧六行格式兼容；旧空正文/删除歧义明确拒绝自动恢复。没有新增 SQLite schema 版本或新表布局；A7 不更换 IPC 字段，只改变增量合并语义。运行时数据、用户凭据和原始问题文件没有加入发布提交。

**当前真实未修的 C7 三个 P1**：

1. 插话批次中途失败，持久化事实/输入状态与实际 UI/provider 消费不一致。
2. 历史错误已恢复，但 sticky 诊断仍把当前合法插话当失败拒绝。
3. Stop 清空 runtime halt 槽后，旧轮可能消费新输入并发第二次 provider 请求。

C7 已用实际 Runner/HTTP/SQLite 复现：1 个正常控制通过，4 个负例断言失败；**已修 0、覆盖 +0**；这些错误路径仍是本版的已知剩余问题，尚未完成的 C7 修复代码不纳入发布。B6/A9/M7 仅有地图或草稿，未核实的候选不称 bug。它们保留原工作树和证据，下一阶段先回 C7 owner/原子交付，再推进 M7 记忆底层、B6 后台、A9 API/UI。没有需要产品决策才能修的已确认通用系统 bug。


## 发布门禁发现并补修

首轮Full在最终文档源880c7135发现两项真实失败，停止后保留日志。CLI把12个独立tests.rs误算生产，已修P1并通过9项真实文件收集回归；旧/新独立CLI对422文件比较证实其他生产指标不变。线路静态检查要求async而拒绝返回Promise的入口，已修验证P2，缺入口负控制仍拒绝，29项功能回归仍通过。旧度量基线按已审交付有意识重采集，100行/1巨石阈值保持原值。详情见 [补修逐文件记录](../reviews/file-audit-2026-10-03/parallel/Release-checks.md)和[证据](../reviews/file-audit-2026-10-03/parallel/Release-checks-verification.json)。新增CLI全文1文件，累计136/514、剩余378；初始18项组合证书仍对应原5d01fb3f的2431结果，最终Full将重新验证当前源码，不能冒充所有门禁第一次都成功。
