# Decisions

## A-001 记忆系统:文件优先,不用向量/图谱/外部框架 [accepted]
- 决定: 记忆真源是 markdown 文件(一条一文件+frontmatter),SQLite 只存可重建派生物;检索用 FTS5+结构化过滤,不引向量库、知识图谱与 Mem0 类框架;写读分离,写路径由 memory-manager 子代理专管。
- 依据: 用户为记忆研究方向,实测 Mem0 一般;向量记忆杂而不精准;图谱慢且不一定准;非参数化外部记忆的核心优势是可编辑与透明。
- 日期: 2026-08-08
- refs: R-103
- 备注: 完整决策记录见 docs/design/memory_system.md §0/§9;重议须新开设计文档说明理由。

## A-002 关闭边界:可用即关闭 [accepted]
- 决定: 缺陷/需求以「功能可用+有自动化验证」为关闭界;E2 夹具、UI harness、压测等验证增强项转专门条目(R-101),不阻塞推进;功能性验收未实现、数据丢失/安全风险、零验证仍不得关闭。
- 依据: 测试矩阵完备性阻塞需求推进;质量欠账应显式记账而不是滞留 fixing。
- 日期: 2026-08-07
- refs: R-101
- 备注: 详见 conventions.md §1.2。

## A-003 工作粒度:一轮一个完整条目 [accepted]
- 决定: 自举轮次以做完一条缺陷/需求为目标;同构批量改动(i18n/重命名/迁移)一轮吃完整类别,禁止微切片;超单轮容量按验收子项分轮并写明批次边界。
- 依据: 微切片实测:D-108 每轮 2~3 处文案拖 34 步,轮次固定开销 15 倍浪费。
- 日期: 2026-08-07
- refs: D-114
- 备注: 详见 conventions.md §1.3;鞭挞默认提示词规则 2 同步。

## A-004 验证选择匹配改动面 [accepted]
- 决定: 纯 ui/ 改动跑 node 检查与冒烟脚本;动 crates/ 才跑 cargo test(先定向,提交前全量一次);与改动无关的套件不跑。
- 依据: 纯前端轮每轮跑全量 Rust 测试是零信息量开销,实测清零后单轮终端调用 30→10。
- 日期: 2026-08-07
- refs: D-114
- 备注: 详见 conventions.md §1.3。

## A-005 记忆分级与安全模型 [accepted]
- 决定: scope=global(U-,偏好/习惯)×project(M-,事实/SOP);episode 是日志进 state.db 不落文件;安全模型=可视化+source 溯源+墓碑可逆,不做重安全规则与写入确认门。
- 依据: 个人开发者场景,重规则损害易用与简易(Claude Code 的痛点);真有问题开发者可自行溯源回滚。
- 日期: 2026-08-08
- refs: R-103 R-104
- 备注: 并发不加文件锁:原子替换+可重建索引+完整性门禁,冲突留给 agent 事后解决。

## A-007 方向基线:可替代区复刻 Claude Code,创新只投护城河 [accepted]
- 决定: 凡 Claude Code 已解决的能力(上下文压缩、限流恢复、凭证生命周期、thinking 回放等),先复刻其行为契约再谈改进——设计前必须先写「CC 基线行为」,偏离基线须记录一行理由,无理由偏差按缺陷处理。压缩判据(不满足即丢弃):单人自用/桌面+CLI/中文优先/服务自举。护城河(引擎强制追踪状态机、鞭挞+验收打假闭环、桌面排队与中文 UX、记忆系统)不复刻,创新预算只花在此。英文 i18n 保留但换架构:chrome/content 分离(对话内容永不触碰)、翻译发生在渲染点 t(key)、禁止事后 DOM 重写,模型输出语言走 prompt 而非显示层转换。
- 依据: 2026-08-08 宏观体检结论"约两万行在以更低可靠性重造 CC 已解决的问题"(context overflow 六次逃逸、限流无策略、凭证无续期均为 CC 已解决项),用户当日认可并定调;i18n 词典+MutationObserver 机制已产出 8 条缺陷家族并会篡改模型输出显示,用户明确英文必须保留、机制必须换。
- 日期: 2026-08-08
- refs: G-001 G-003 R-086 R-101
- 备注: 完整方案与复刻清单见 docs/design/direction_taste.md;重议须新开设计文档说明理由。

## A-006 AI 设计讨论与技术决策采用双层记录 [draft]
- 依据: 只保留 decisions.md 会丢失方案演进，只保留设计文档又难以注入和检索稳定约束；双层结构同时保留可追溯过程与可复用结论。
- 决定: 设计文档记录问题背景、讨论摘要、候选方案、技术选型、取舍、变更记录与验证证据；稳定的产品或技术约束另建 A-* 决策条目。设计文档与决策条目必须双向引用，方案变更新建决策并将旧决策标为 superseded。
- 备注: 规范入口：docs/design/readme.md；R-108 示例：docs/design/r108_ai_design_decision_records.md。待用户接受后再将本条从 draft 转为 accepted。
- 日期: 2026-08-08
- refs: R-108

## A-008 巨石拆解:文件级模块化,前端用有序 classic script,不引入 ES modules [accepted]
- 决定: 四个巨石(app/main.rs、ui/main.js、core/runner.rs、core/store.rs)只做文件级拆分与可见性收窄——零行为变更、外部 API 面零变更;前端拆为 index.html 按序加载的多个 classic script(顶层 let/const 走共享全局词法环境,与单文件语义一致),不引入 ES modules、打包器或框架;四个冒烟脚本改为从 index.html 解析脚本清单,runtime 冒烟逐文件 vm.runInContext(与浏览器多 script 语义一致,含 TDZ)。
- 依据: 2026-08-09 仓库工程评审定调拆解。评审原文建议 ES modules,勘察后偏离:①runtime 冒烟是 vm 单串执行,ES modules 须重写整套 harness(experimental vm modules 或全局注入+原生 import);②模块不建全局绑定,数百处跨文件引用须显式化,零行为变更承诺无法机械保证;③classic 多脚本方案冒烟机制逐文件等价可验证,弱模型可按行号地图机械执行。文件级模块化的收益(agent 检索粒度/patch locality)两方案相同。
- 日期: 2026-08-09
- refs: R-153 R-154 R-155
- 备注: 完整方案 docs/design/monolith_decomposition.md。与评审建议(ES modules)有方案偏差,已向用户说明偏离理由,2026-08-09 用户认可转 accepted;日后若要 ES modules 化,须新开设计文档并重写冒烟 harness,本条转 superseded。

## A-009 发布证据链:无绑定 commit 的验证证据不得打包发布 [accepted]
- 决定: 公开发布(package.ps1)前必须存在 dist/verification.json:其 commit 与 HEAD 全 SHA 一致且全部检查通过,否则中止;证据由 scripts/verify.ps1 在干净工作树上产出;GitHub Actions 对每次 push 在独立环境复跑同套门禁。"Agent 说 done"不作为发布依据,只认独立、绑定 commit 的证据。
- 依据: 2026-08-09 用户定调(评审 P0:release gate 机械化——package.ps1 有 -Ack 范围核对却不验证"该 commit 跑过测试",正确性依赖约定而非机制,与"规则进代码"哲学矛盾)。与 D-183(防夹带)、D-198(防假安装)同族。
- 日期: 2026-08-09
- refs: R-152 A-007
- 备注: 方案 docs/design/ci_release_evidence_chain.md;通道语义(stable/nightly)、多平台、签名/SBOM、CI 直发显式不在本决策范围。
