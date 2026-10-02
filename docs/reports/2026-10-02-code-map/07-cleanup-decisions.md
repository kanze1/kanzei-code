# 代码库与决策的整理依据

本页保留清理前的静态审计依据。原优先问题已按用户反馈归档；本轮已经校正文档现状、清理本地工作树和旧分支。现行文档口径见 [current-state.md](../../current-state.md)，清理结果见 [清理记录](../../cleanup-2026-10-02.md)。

原表保留审计时点判断。正式 A 文档现已补入 ESM、dense/RRF、FileLock、手动 CI、worker 与 AUTO 边界，ID 和接受状态未变；下表中的“应改写/需重议”不是新增待办。tracker 状态未改。

## 清理前审计的已归档问题

| 问题 | 源码事实 | 影响和建议 |
|---|---|---|
| 活动进程停止接口缺失 | [06-activity.js](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/06-activity.js:413>) 调用 run_tool_process_stop；166 个 command 声明及 main 注册均没有它 | 该按钮无法经当前 IPC 路径停止进程；接入已有 process 停止能力并验证 owner/project 匹配 |
| ESM 与正式决策冲突 | [index.html](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/index.html:1550>) 已用 module；[A-008](<C:/Users/kanzei/Documents/kanzei code/.kanzei/project/decisions.md:52>) 禁止 ESM | 新建替代决定，保留无打包器的实际边界，删除已经无效的 classic 全局语义承诺 |
| 向量实现与决策状态脱节 | [index.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/index.rs:340>) 余弦扫描、431 行 RRF；A-001 accepted 禁向量，A-011 仍 draft | 保留文件真源，明确可选 dense、完整 lexical 降级和验证条件；确认新决定后局部替代旧子句 |
| 不加锁的旧备注失效 | [MemoryStore::lock](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/store.rs:186>) 已使用 FileLock；[决策偏好](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/decisions.rs:231>) 另有锁 | 更新 A-005 并发约束，写清文档锁、写槽和 worktree 各自保护的范围 |
| 自动 CI 承诺失效 | [ci.yml](<C:/Users/kanzei/Documents/kanzei code/.github/workflows/ci.yml:3>) 只有 workflow_dispatch；A-009/A-010 和设计文档仍按每次 push 兜底 | 明确选择恢复自动 CI 或承认手动策略；在此之前不能靠“之后 CI 会查”省略必要本地检查 |
| CI 与本地 ESM smoke 命令不同 | [ui-runtime-smoke](<C:/Users/kanzei/Documents/kanzei code/scripts/ui-runtime-smoke.mjs:2645>) 使用 vm.SourceTextModule；[verify.ps1](<C:/Users/kanzei/Documents/kanzei code/scripts/verify.ps1:156>) 带 experimental-vm-modules，CI 没有 | 同步执行参数，并实际跑目标 CI；本报告没有取得当前远端 CI 日志 |
| README 仍宣传 Claude 订阅登录 | [README](<C:/Users/kanzei/Documents/kanzei code/README.md:23>) 列 Claude Code 令牌；[build_route](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/assemble.rs:31>) 缺 API Key 时明确停用 | 修改中英文能力说明；保留实际 Anthropic API Key 通道 |
| 全局偏好保存与生效路径不一致 | [decisions.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/decisions.rs:221>) 支持 Global 保存；[dev.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-tools/src/profiles/dev.rs:275>) 常驻偏好只读 project | 单独决定 Global 的适用范围，贯通保存、注入、检索与图谱；当前不能保证保存后下一轮所有入口采用 |
| 调度存在活跃副本 | [memory/scheduling.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/scheduling.rs:1>) 明示复制；[memory/mod.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/mod.rs:1270>) 仍调用；桌面改走统一 work selection | 将已裁决 work context 传给 memory；让记忆负责检索，不重新决定下一项工作 |
| 工作区与 HEAD 不是同一功能快照 | 文件索引区分 modified/untracked/HEAD，后台验证和复核等存在未提交实现 | 整理前先按归属建立可验证批次；不可把当前工作区能力直接写成已发布能力 |

这里的参考图谱是从 refs 等事实生成的只读展示，不等于外部知识图谱推理引擎。A-001 的图谱条款需要澄清定义，不应仅因界面存在图就判断整条禁止已被推翻。

## 全部现存 A 决策逐项对照

当前 decisions.md 有 A-001 至 A-019 共 19 条。下表保留原状态，判断基于当前源码；accepted/draft 是记录状态，不是本报告作出的批准。

| 决策 | 原状态 | 当前代码与建议 |
|---|---|---|
| A-001 文件真源与本地检索 | accepted | Markdown 是记忆内容真源；可重建 index.db 与不可从 MD 重建的 state.db 分开。已校正可选 dense/RRF 与可视化图的边界。 |
| A-002 可用且自动验证后关闭 | accepted | tracker 验收、test_record、发布证据、用户复核已有独立层。保留原则，明确哪个检查证明哪个条件；自动验证通过不能冒充用户验收 |
| A-003 一轮一个完整条目 | accepted | 常规需求粒度约束仍有意义；work_units_v1 显式支持长任务分阶段执行。补充执行模型适用范围，避免禁止合理 checkpoint |
| A-004 验证匹配改动面 | accepted | verify-policy、结构化 git 和 targeted 模式落实此方向。“提交前全量一次”已被 A-010 明文修订，无需把它再列成未解决冲突 |
| A-005 记忆分级、文件锁与作用范围 | accepted | 已校正 FileLock/原子替换/worktree/进程内写槽的职责；global 偏好保存不自动代表普通召回或 Dev 常驻已采用。 |
| A-006 设计文档与 A 决策双层记录 | draft | 仓库实际已有大量设计文档及 refs，但原条目仍待接受。应把实施状态和决策接受状态分开；建立双向引用，不自动把 draft 改 accepted |
| A-007 复刻基线、创新投核心 | accepted | 配置、压缩、认证、中文交互及记忆方向可对照；这属于产品取舍，静态代码不能证明全部能力达到外部产品基线。保留约束并将比较对象和例外记录到具体设计 |
| A-008 文件级模块化与静态 ESM | accepted | 原 classic 方案保留为历史；当前 type=module、显式 import/export 与 ESM smoke 已接线，仍无应用打包器或前端框架。 |
| A-009 发布绑定 commit，CI 手动复核 | accepted | HEAD 绑定、全量、无跳步的发布门禁保留；CI 文档与决定已改为 workflow_dispatch 手动事实，不承诺每次 push 兜底。 |
| A-010 批内定向、关闭与发版全量 | accepted | 验证匹配改动面与 cadence 保留；CI 需要显式触发并核对结果，旧 conventions 章节与自动兜底引用已校正。 |
| A-011 dense 为第二通道 | draft | 实现已提供 embeddings、SQLite 向量表、Rust 余弦扫描、RRF 和 lexical 降级。sqlite-vec 标为初期设想；draft 与回放启用条件保持独立。 |
| A-012 runtime SQLite、治理 Markdown | draft | 会话 typed 事件、投影、Work Unit 与恢复读取已有实现。draft 是审核状态；typed/legacy 回退仍有兼容职责。 |
| A-013 完整独立裁决体系暂不建设 | draft | 已限定决定范围：冻结快照和独立 worker 已接线；完整四角色独立审查仍不由此成立。 |
| A-014 @@kanzei terminal callback | accepted | core/research_runner 与 tools/research_runner 解析结构化回调、保留普通输出和真实终态。继续保持高频事件与稳定 Markdown 事实分离 |
| A-015 探索 Markdown 为路线真源 | accepted | 名称按 A-019 统一为探索，结果挂在探索之下；只读路线投影原则保留。 |
| A-016 本机和 SSH、登记与快照 | accepted | runner local/ssh、environment snapshot 和专用通道已实现。compute 自动准备是后续扩展，不能泛化为远端代码/数据自动同步或集群调度 |
| A-017 Research 业务独立、设施复用 | accepted | topic、exploration、result、research_runs 与开发 Work Unit 分开；共用会话、事件、存储和工具基础设施。保留此边界，避免为共用 auto_run 引擎而合并业务状态 |
| A-018 按环境执行 policy | accepted | relaxed/managed/approval/strict、租约、时长和清理合同存在。把 policy 合同和实际执行终态都作为记录；已有结构不证明任意远程环境都已验收 |
| A-019 探索→结果，通用参数原样记录 | accepted | 两层业务模型保留。通用 runner 不解析 params；AUTO 完整实验另有 JSON 与矩阵/共享种子合同，专用边界已补入决定。 |

## 建议收拢的业务域

这是归属方案，可以先通过目录、接口和责任表完成，不要求立即新增 crate。

| 域 | 应承担的判断 | 主要代码 | 应避免的重复职责 |
|---|---|---|---|
| 运行 | 请求装配、模型循环、取消、消息/事件与上下文 | core/runner、app/run、llm、harness | 前端再实现运行事实状态机 |
| 工作 | tracker、取活、认领、单元、验证合同、交付 | tools/work、tracker、core/store/work、app/verification_monitor | memory 自己排工作，UI 自己认定 unit done |
| 决策与偏好 | 提问、决定、复核、纠正、长期作用范围 | core/decision、tools/question、app/decisions、memory/preference | 将 A 架构决策和每轮自主决定当成同一种记录 |
| 记忆 | 经验准入、检索、生命周期、回放 | memory crate、runner/recall/item_context | 决定研究阶段或需求优先级 |
| 研究 | topic、计划、来源、探索、运行结果、AUTO、论文 | core/research*、tools/research*、app/research* | 借用开发需求关闭语义冒充实验结论 |
| 项目与协作 | 主根、线根、worktree、占用、合并 | project_root、app/processes/worktree/collaboration | 根据前端选中目录重推所有身份 |
| 工具执行 | 文件/命令/Git/联网/浏览器/编译及结果合同 | tools、harness/tool_pipeline、core/runner/drive | 每个工具重造权限、取消、结果包装和观测 |
| 桌面服务 | IPC、窗口、预览、文件编辑、mobile、voice、更新 | app 服务模块 | 将模型策略写入窗口服务和动画控制器 |
| 表现与资源 | 对话渲染、导航、动画、品牌和原型 | app/ui、scripts/oc-*、docs/prototypes | 动画事件成为任务进度的唯一真源 |

base 放原子文件、路径和写日志；harness 放能力与权限契约；llm 放协议与网络。它们是共享设施，整理时以依赖方向为准，不按界面名字拆分。

docstore 目前在 memory crate，但被 tracker、工作和研究使用。先明确它是共享文档存储，再评估下沉；在没有整理调用面之前直接移 crate 会产生大范围改动。

## 推荐处理顺序与完成条件

| 顺序 | 整理工作 | 到什么状态算完成 |
|---|---|---|
| 1 | 修复缺失停止接线，统一本地/CI smoke 参数 | 真实入口能调用对应后端；定向验证证明停止归属正确；目标 CI 有结果 |
| 2 | 统一 A、README、conventions 与当前实现 | 向量、ESM、全局偏好、CI、验证者和 AUTO 边界各有明确决定及双向来源；清除 conventions 中 main/dev 冲突段 |
| 3 | 固定各对象的状态真源和身份 | 每个 UI/模型/CLI 读取入口标出 project_root、cwd、session、process、unit、topic、run/result；不由“当前页面”隐式替代 |
| 4 | 收拢取活与工作上下文 | memory 不再持有队列裁决副本，桌面和 CLI 消费同一裁决合同；不同 WIP/停车/依赖条件保持一致 |
| 5 | 收拢决定、纠正和偏好作用范围 | once/project/global 各有真实保存和下一轮生效证据，失败可见、重复提交幂等、旧复核不覆盖新决定 |
| 6 | 收拢工具执行的公共包装 | 逐族列出迁移状态，权限/取消/guard/输出分类/观测一致；各工具业务 body 保持独立 |
| 7 | 按业务责任拆大文件和 UI 循环依赖 | 先画调用面，再减少跨域依赖和可变全局桥；每批保持原有行为合同可验证 |
| 8 | 归档历史设计、原型和一次性脚本 | 明确对应当前实现、后续方案或历史证据；被保留的辅助脚本仍能定位，历史证据不因过期被删掉 |

## 保留、合并、归档与删除的判断

保留承担独立业务真源、状态机或外部合同的模块。合并相同规则的重复实现及仅转发而没有稳定边界的包装。归档已经被新方案替代的设计与一次性审计脚本，保留原结论、日期和证据链。

删除必须先查生产注册、动态 dispatch、CLI、脚本、测试和文档调用。一次 grep 没找到直接引用不能证明代码无用。宏生成、字符串工具名、Tauri 注册和 hidden worker 都会绕过普通函数引用搜索。

报告已给出全部文件/声明导航和主要逻辑，但没有给所有函数强行打“可删”标签。后续整理应以功能边界和调用证据为依据，避免把测试夹具、原型和生产实现混着计算复杂度。

## 清理前审计的验证边界

最初审计只核对源码，未执行全量编译、测试和发布。本轮整理后的编译、测试与交付结果见下方整理验收；远端 CI，以及这台机器的 voice/embedding/SSH 服务仍未实时验收。Softwire 总体引擎改造需逐项对照设计。

具体功能、调用链与状态规则分别见 01 至 06；接口接线和逐函数位置见 08 至 10；现有设计文档和资源见 11。

## 本轮九项整理已落实

网页 C01–C09 已完成实现；表中历史建议和原问题保留为审计记录。具体改动与实际验证见 [整理验收](../2026-10-02-web-cleanup.md)。共享子代理执行并不改变可写角色的授权边界；复杂视觉仍是表现层。

| 项目 | 实际改动 | 验证 |
| --- | --- | --- |
| C01 | AUTO 作为研究主控制面；plan/loop 复用其阶段、暂停与预算，旧课题保留兼容入口。 | AUTO 暂停和阶段拦截、旧预算不能覆盖主预算的回归通过。 |
| C02 | 分节写作与 AUTO 使用同一论文工件和编译服务；修复稿件必须重新核验。 | 同一路径写作、数值核验不能绕过、修复后回到核验、旧 PDF 不能冒充新编译的回归通过。 |
| C03 | task 与勘察复核共用子代理执行、超时和取消处理；固定角色与屏障保留为策略。 | 共享超时会取消未完成任务；子代理和流水线回归通过。 |
| C04 | 移除只保存 JSON 清单的容器注册、升级、回滚入口；实际 Agent 配置与快照继续保留。 | 无剩余生产引用；Rust 编译、IPC 契约和 UI 检查通过。 |
| C05 | Vega-Lite 为默认绘图入口；PGFPlots 和 matplotlib 抽到显式选择的适配器。 | 未知引擎和未选择适配器的输入被明确拒绝；绘图回归通过。 |
| C06 | 记忆专用对话与图谱通过共享加载器首次打开时加载；核心编辑和检索继续常驻。 | 真实浏览器确认默认未加载、首次对话和图谱入口可用。 |
| C07 | 移动端 UI 从主设置模块拆出，按需加载；KDE Connect 通知单独启用并检查依赖。 | 真实浏览器确认默认不调用手机接口、展开设置才加载；通知默认关闭。 |
| C08 | 语音界面与运行时首次使用才加载；关闭后停止会话归属轮询。 | 真实浏览器确认默认没有语音加载和设置调用，首次检查入口可用。 |
| C09 | 保留轻量状态反馈；复杂视觉统一开关，默认关闭；素材制作脚本移到 extras。 | 真实浏览器确认默认不加载渲染器，开关与关闭销毁通过；既有视觉回归通过。 |
