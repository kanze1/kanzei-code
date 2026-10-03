# 通用系统并行审查与修复计划

## 当前执行状态（第五轮整合）

全文审查 **132/514**，余 **382**；base、harness完成，其余五层继续。16包已合入，17项当前Windows统一验证通过。四工作树复用，三个Sol子代理，Cargo单运行者；下一队列B5 → A7 → C7，M7先真实探针。只本地提交/sync main-dev，不push/发版。完整结果见[第五轮整合记录](parallel-round-5.md)、[证书](parallel-round-5-verification.json)、[公共队列](parallel-queue.json)。下方起点与第一轮分工保留为历史，当前进度以本段/coverage为准。

## 初始起点与目标（历史）

代码基线：`0d0875ab`。main/dev 均已同步此提交；当前只有一个工作树。验证基线：Rust 2231 passed / 0 failed / 5 ignored，Clippy、格式、UI 运行时与浏览器回归通过。

目标仍是按依赖地图逐文件修复真实 correctness、状态一致性、并发及失败语义问题。用并行审查提高速度，不降低证据标准。A 家专属能力暂不处理。只本地提交、整合 main/dev，不 push、不发版。保留原有未跟踪 `问题.MD`，不修改、不提交。

已完成全文审查 30 个文件；词法索引总计 511 个。剩余 481 个详见 `parallel-queue.json`；其中已经检查过切片的文件也算未完成全文审查。索引只覆盖仓库自有 Rust/JS/MJS，不能据此宣称 Cargo 配置、脚本、HTML/CSS 和全部仓库资产审查完成；实际调用链涉及这些文件时按需加入清单。

先读 `dependency-map.md`、`coverage.json` 和最新 `state-ui.md`；遇到历史契约再读对应设计文档/报告，不要求每个子代理重复读全部历史资料。

## 分工和工作树

- 主代理负责调度、确认可达问题、跨模块契约、整合、最终测试、公共报告和进度。
- 同时最多 3 个子代理。用户已确认使用 Sol；在当前聊天下执行子任务，不创建独立聊天。
- 每个活跃子代理使用独立工作树、唯一分支 `kanzei/audit-<包名>`。以主代理给出的已验证提交为起点，记录实际路径。使用应用工作树工具创建/回收时遵守其规则。
- 每个文件同时只有一个写入者。任务派发前写明确切文件清单，不能只写“负责 core”。所有 caller 都可以读；清单外文件只报告根因和所需适配，交主代理处理或明确转交。
- 不把整个目录一次塞给子代理。普通任务包约 4–8 个文件；复杂事务/调度大文件可单文件成包。按实际复杂度拆，不为凑数量截断契约。
- 子代理不能编辑公共 coverage、inventory、dependency-map，不能切换或提交主目录分支。各自写独立报告 `parallel/<包名>.md`。

## 第一轮：立即可派发的三个任务包

这些是确切初始写入范围。并行阅读互不阻塞；发现需要改公共 API/锁语义/持久化格式时先报告给主代理，受影响的上层实现等待底层契约落定，继续审查无关文件。等待的是内部协调，不需要再问用户是否继续。

### A0：剩余基础原语拆分文件

写入范围：

- `crates/kanzei-base/src/atomic_file/lock.rs`
- `crates/kanzei-base/src/atomic_file/tests.rs`
- `crates/kanzei-base/src/atomic_file/lock_tests.rs`
- `crates/kanzei-base/src/write_log/codec.rs`
- `crates/kanzei-base/src/write_log/tests.rs`

重点：系统锁与进程内槽的状态对应、重入/升级/释放/超时、读写互斥、日志 codec 三态及损坏处理。先读现有 base 报告，避免重复重写已整合实现。测试文件也需判断是否真正证明契约。

已审 `atomic_file.rs`、`write_log.rs`、`path_form.rs`、`lib.rs` 为只读基线；确有根因位于这些文件时交主代理解锁范围，不在 caller 打补丁。

验证：`cargo test -p kanzei-base`；若涉及锁，运行定向线程/进程测试并说明覆盖平台。不能把历史 Linux 结果当成本轮结果。

### B0：Harness 调度和回调边界

写入范围：

- `crates/kanzei-harness/src/orchestration.rs`
- `crates/kanzei-harness/src/managed_fence.rs`
- `crates/kanzei-harness/src/async_mailbox.rs`
- `crates/kanzei-harness/src/pending_question.rs`

重点：租约/读槽释放、关闭与回调竞争、取消后的消息投递、一次性回答状态。先列公共 API 和 core/tools/app caller；操作级文件锁与运行级租约不能互相替代。

验证：`cargo test -p kanzei-harness`；确认 race 用 barrier/channel 等确定性时序证明，不能只用 sleep 碰运气。

### C0：会话数据库基础与事件事务

写入范围：

- `crates/kanzei-core/src/store/mod.rs`
- `crates/kanzei-core/src/store/session.rs`
- `crates/kanzei-core/src/store/session/artifact_refs.rs`
- `crates/kanzei-core/src/store/schema.rs`
- `crates/kanzei-core/src/store/events.rs`
- `crates/kanzei-core/src/store/path_migration.rs`

重点：事务模式、迁移失败、事件序号、备份与工件引用、路径身份归一化；先保护 0d0875ab 的输入/事件原子性。迁移必须有旧库升级与失败路径证据，不能只验证新库。

验证：`cargo test -p kanzei-core`，必要时小型临时数据库回归；不操作用户真实 state.db。

## 后续队列与依赖门槛

队列按以下组分类，组内继续拆小包。阶段表示依赖优先级，不表示必须等整层结束才能阅读上层；上层的契约相关修改和最终验证必须基于已整合的底层。

| 组 | 范围 | 推进顺序与重点 |
| --- | --- | --- |
| base | base 未审拆分文件 | A0 收口后，公共原语稳定；真实新证据才重开已审文件 |
| harness | harness 其余未审文件 | tool/permission → pipeline/context → auto_run/config；注册表已审文件只做 caller 复核 |
| llm | llm 未审测试/辅助文件 | 通用协议与已修行为的回归；A 家专属文件保留为 deferred，不删除也不扩展 |
| core-store | store 其余未审文件 | 基础事务 → processes/workspace → typed projection/work/task → decisions/deliveries/notifications → rewind/checkpoints 等 |
| core-runtime | core 非 store 未审文件 | 先映射 runner 模块依赖；运行状态 → 取消/停止 → 工具串并行 → 回调/子代理 → replay/入口 |
| memory | memory 未审文件 | docstore model/parse/render → repository/archive → memory ledger/store/index → admission/retrieval/lifecycle |
| tools | tools 未审文件 | 低层工具/文档写者 → managed/cross_tree → background/work/worktree → agent 编排；按实际 imports 排序 |
| entry-service | app Rust 与 CLI 未审文件 | SessionRuntime/输入 → schedule/stop → async callback → run persistence/events → command/API → 其余服务 |
| ui | UI/移动页面未审文件 | IPC/event 契约稳定后，模型归约 → 会话/活动/后台投影 → 用户动作/页面入口；不做视觉重设计 |

某组不足三个独立任务时允许空一个槽，不能为并行而拆散同一状态所有者。空闲子代理可只读审查后续组和整理 caller 地图，不能基于尚未落定的契约抢先实现。

## 每个任务的执行规则

1. 记录任务基线、写入清单、依赖与所有 caller、状态所有者、读写/跨进程边界。既有 inventory 只是定位工具，必须搜索真实调用点。
2. 按依赖顺序全文阅读；测试、条件编译及失败分支也在范围内。只读切片不能标全文 PASS。
3. 只记录 P0/P1/P2 的真实问题：可达触发条件 → 错误状态/结果 → 根因。缺测试、风格、理论碰撞不算 bug。
4. 根因明确直接做最小完整修复。public API、序列化、schema、路径/锁/hash 语义改动必须先把 caller 清单发主代理，协调下游适配。
5. 有并发/持久化修复优先提供确定性回归或故障注入；证明 bug 本身，不仅断言新实现的分支。
6. 文件无实际问题就 PASS，不修改。报告按下方固定模板写。
7. 跑受影响包的 check/test 和定向验证，记录命令、退出码、基线/提交及日志路径。验证不了的真实边界明确写出。
8. 提交只包含自己的改动；向主代理交付提交号、报告、问题与验证清单。完成后主代理继续派下一个包，不等用户逐轮回复。

## 编译与验证资源

- 子代理不并行占用同一个 Cargo target 目录；优先各自独立 target，若机器磁盘/内存不足则主代理串行发放 Cargo 验证时段。
- 每包定向验证；主代理整合一轮后统一做全工作区 check/test/Clippy/fmt，避免每读一个文件就跑整个仓库。
- `cargo fmt --all` 可能修改非自己范围，子代理仅格式化所属文件并核对 diff；不要把格式变化混入其他模块。
- UI 全套浏览器冒烟由主代理串行执行，避免端口/进程/测试工件冲突；子代理可运行自己的纯逻辑回归。
- Windows 测试通过不等同于 Linux 验证；历史验证不等同于当前提交验证。

## 整合规则

主代理按底层 → 上层顺序收包：检查真实 bug 和 diff → 整合精确提交 → 修复 caller → 定向验证 → 同轮全量验证 → 更新地图及 coverage → 本地提交并同步 main/dev。

- 共享 `Cargo.toml`、`Cargo.lock`、公共 mod/re-export、IPC 注册、全局 UI 冒烟、coverage/inventory 等由主代理唯一维护。初始 C0 对 store/mod.rs 的所有权仅在该包存续时有效。
- 不整分支盲合。确认子代理基线与提交范围，避免把别的任务夹带进来；遇到重叠先比对语义，不能简单选择 ours/theirs。
- 底层契约变化后，受影响子代理更新到已整合基线并复验，旧基线绿灯不能替代整合验证。
- 工作树回收前确认提交已经整合、报告可查、没有未保存工件；用可恢复的归档机制回收。
- A 家相关既有提交保持当前状态，不在本计划里回滚或扩大支持。

## 报告格式

每个文件必须记录：

```markdown
## <file>
### 职责
一句话；标明全文或切片。
### 判断
PASS / P0 / P1 / P2
### 确切问题
- 可达路径与实际影响；PASS 写“未发现有实际影响的问题”。
### 修改
- 改动及原因；无修改写“无”。
### 影响范围
- 直接 caller、状态/文件/API/UI、兼容性。
### 验证
- 命令及结果、被验证的 invariant、失败路径。
```

包末附 Module Summary：已修复分级、PASS 文件、真正需人工判断的问题、cross-module contract、当前无法验证的真实风险。相同根因跨多个文件适配时只统计一次问题，不虚增 bug 数。

## 进度与完成判据

- 主代理每轮只报：完成哪些依赖链、本轮全文文件数/累计数、真实修复、验证、剩余与下一包。不要把索引计数当已审数量。
- 511 是这次索引基数；新增文件加入清单，删除/拆分文件记录替代关系，不能靠删列表降低待办数。
- 已审文件只有在对应内容与记录一致时复用结论；后续改动使旧 SHA 不匹配时，检查改动范围并更新审查证据。
- 每个阶段完成要求：本范围队列逐项有全文记录，直接 caller 适配完成，变更通过验证，未验证边界明确；不能只以测试通过宣称阶段完成。
- 最终报告分清：已审、仅切片、尚未审、deferred；整层未完成就不能写整层完成。报告必要的真实剩余风险。

## 给下一位主代理的启动指令

```text
在 C:\Users\kanzei\Documents\kanzei code 继续通用系统文件级审查与修复。
先读 docs/reviews/file-audit-2026-10-03/parallel-plan.md、parallel-queue.json、dependency-map.md 和 coverage.json。
已验证代码基线为 0d0875ab；实际执行前核对当前 main/dev、工作树和未提交状态，保留原有 问题.MD。
我授权你使用子代理并行，最多三个子代理加一个主代理。每个子代理独立工作树、明确文件所有权，按计划先派 A0/B0/C0。
已审 30 个文件不要无依据重审；剩余按队列、底层到上层推进。真实系统 bug 直接修，不需要逐批向我确认。
公共接口先查所有 caller，跨范围修改由主代理协调；只做有实际影响的修复，不做风格清理。
主代理负责整合、全量验证、更新逐文件报告和覆盖地图，继续派发下一包直到约定通用范围完成或出现确实无法自行解决的阻塞。
只本地提交并同步 main/dev，不 push、不发版。A 家专属支持先不管。
按计划报告真实进度，不把切片/索引/测试通过算成全文审查完成。
```

## 第六轮发布检查点

B5/A7 已合入并通过当前源的 18 项组合验证。全文累计 135/514，剩余 379；base/harness 两层完成，其余五层继续。C7 的 3 个 P1 已复现但尚未修复，M7/B6/A9 仍为草稿或只读候选，均排除本次发布。详情见 [第六轮记录](parallel-round-6.md) 和 [发布解读](../../reports/2026-10-03-file-audit-release.md)。最新用户已明确授权先发布本版，覆盖此前仅本地不 push/不发布的阶段限制；保留其他工作树和原有 问题.MD。
