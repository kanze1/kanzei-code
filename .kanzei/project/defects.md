# Defects

## D-504 鞭挞配置双真源与 autoRounds 双计数器,四副本靠手工互拷同步 [fixing] (medium)
- 复现: crates/kanzei-app/ui/08-compose.js:1088-1097 lineAutoConfig 活动线读 DOM 复选框、其他线读 processAutoState Map;同状态另存 localStorage(kz-process-auto-state) 与后端 ui_prefs/auto_state_update(:1014-1021,:1057);autoRounds 全局(:4)与 state.auto_rounds(:337,:380) 靠 07-events.js:439/449/465 手工互拷,:1078 切线再读回
- 影响: 四副本两条同步路径,漏一处即显示 0/10 实际下一轮撞上限;历史已翻车两次
- 来源: 2026-08-18 全库勘察(主会话);D-290/D-353 历史翻车点
- 标签: 前端
- 验收: 收敛单一真源(Map/state),DOM 只做投影;切线/后台线/重启回归用例;冒烟覆盖
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-504
- 进展: 实现提交 `8f490d92` 与自动化证据已完成。已确认真实安装位 `C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe` 存在且当前进程正在运行；当前窗口显示用户正在使用该应用，按发布规则不得强杀或擅自关闭。因此最后一项“已安装桌面应用退出→重启→读取持久化状态”暂记外部阻塞，待用户关闭窗口后执行真实重启链路；其余验收保持已通过。
- observed_head: 8f490d92856e1e0208efee838b55b18254d6c883
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787008359348
- 阻塞: 
- 对账: 2026-08-20 对账:用户已关闭 kzapp 窗口,阻塞解除;剩余动作=重新启动安装位 kzapp 回读持久化 auto state 完成真实重启验收(桌面窗口空闲期执行,CLI 循环亦可承接);其余验收已通过(8f490d92)
- 停车: 等待用户 kzapp 窗口空闲:剩余验收=重启安装位回读持久化 auto state,当前 PID 51368 为用户使用中进程不可接管;上一窗口期(2026-08-20 03:5x)已用于 R-101 停止 E2。解除人:用户空闲窗口+agent 执行

## D-566 cross-tree 隔离快照无回收且构建产物误报,121 目录 143MB 纯堆积 [open] (medium)
- refs: D-395 D-397 R-306
- 复杂度: 中
- 复现: .kanzei/quarantine/ 现存 121 个目录共 143MB(shell-with-log 82、cross-tree 32、bg 7),最早 2026-08-16,无任何回收路径。抽查 cross-tree-1787021081327:内容是 crates/kanzei-app/gen/schemas/desktop-schema.json 与 windows-schema.json——Tauri 构建产物,任一线跑构建即重生成,被越界检测当跨树写入取证;cross-tree-1787060772922 存的是 p16 线自己 R-299 B1 提交(7188ba76)的前置内容,合法自身工作被判越界。08-16 单日 30 次 cross-tree 隔离(两线并行互撞日)
- 影响: 误报占绝大多数,真越界信号被淹没;143MB 取证快照只进不出;构建产物类路径每次并行构建都会再触发
- 标签: 流程
- 验收: ①构建产物路径(gen/schemas 等)进入越界检测豁免清单或按内容指纹放行,有定向测试;②quarantine 提供按日期/类型的清理入口(dry-run+实际释放量),真越界证据可显式保留;③清理后 121 个存量目录处置留痕;④真实并行双线构建实测不再产生 schema 误报
- 优先级: P2
- 进展: 2026-08-20 存量处置完成:121 目录 143MB 经用户放行后清空,清单存证 scratchpad/quarantine-manifest-20260820.txt;误报最大来源已随 R-306 B1 修复(collect_tree_metadata 补 D-407 排除清单)。剩余验收:豁免/清理入口代码化(①②④)与真实并行双线实测
- observed_head: 080db353cc33509398d0746987dccf2b703fe0b1
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787174556151

## D-567 记忆 inbox 消化 10 进 0 出:manager run made no inbox progress,96 条积压 [open] (high)
- refs: D-409
- 复杂度: 中
- 复现: .kanzei/memory/inbox.checkpoint.json(updated_at_ms=1787169243689,2026-08-20 03:54):batch_id=inbox-1787169204456,status=failed,input_notes=10,success_notes=0,failure_reason=manager run made no inbox progress,pending_after=96。inbox.md 现存 96 个 note 块(106KB,08-18~08-19 产生,79% 为 bash 指纹)
- 影响: inbox 完全不消化,积压只增不减;D-409 修的是整箱塞爆(分批已生效,本次确实只喂 10 条),这次是 manager 消化端零产出,属新故障模式;记忆写入管道断裂,新知识无法晋升
- 标签: 后端
- 验收: ①定位 manager run 零进展根因(模型调用失败/discard 销账失败/门禁拒绝)并修复,失败原因可观测不再只有一句 no progress;②真实重跑一批消化,success_notes>0 且 pending 下降;③96 条积压清空或按同指纹聚类批量处置留痕;④连败告警:连续 N 批 status=failed 主动上报而非静默重试
- 优先级: P1

## D-568 记忆 INDEX 描述串号污染:M-014/M-015 描述抄错条目,毒化 FTS 检索 [open] (medium)
- 复杂度: 小
- 复现: .kanzei/memory/INDEX.md:M-014 标题「HTML 静态文案必须登记进资源表」但描述整段是 M-009 的「edit 报 old_string not found 时必读…」;M-015 标题「SSE 流内 context overflow」描述却是 M-029 的「处理 bash git 拦截…结构化工具显式 stage」。index.db 的 memory_fts 索引 description 字段,错配描述使这两条在错误查询下被召回
- 影响: FTS 检索被毒化:错误主题命中错误记忆;INDEX 是每会话注入的真源,串号直接影响召回质量
- 标签: 后端
- 验收: ①M-014/M-015 描述修正与源文件 description 一致;②全量 INDEX 行与对应 M-*.md 的 description 做一次机械一致性核对,输出不一致清单并修复;③重建 index.db FTS 后检索抽查不再串号;④INDEX 生成/更新路径补一致性断言防复发
- 优先级: P2

## D-569 tracker 完整性退化复发(D-331 同形态):归档标题双状态标记与非法 severity 再现 [open] (high)
- refs: D-331 D-553 D-554 D-555
- 复杂度: 中
- 复现: defects-archive.md:6822 的 D-553 标题行为「... [open] (small) [fixed]」——同行两个状态标记且 (small) 非法(合法 severity 为 high/medium/low);D-554 同样「[done] (small) [fixed]」;defects-archive.md:6829 引擎在取活依据字段写入「[tracker integrity degraded] D-555: invalid defect lifecycle [done]」,该告警全库出现 4 次。D-331(已 fixed,high)当时修的正是「归档终态无法安全修正且非法状态污染缺陷标题」,b140322 加了跨 DocKind 状态标记校验+fix_terminal,08-20 同形态再现说明校验有漏洞或写入方绕过
- 影响: 归档数据被污染且按 D-331 教训会扩散:完整性门禁降级告警混入取活依据字段,畸形条目无法被 list/get 正确解析;M-012 类机制恶化可能拒绝所有 tracker 写操作
- 标签: 核心
- 验收: ①定位本次畸形写入的具体路径(哪个写入方绕过了 b140322 校验)并封堵;②用 fix_terminal/repair 修正 D-553/D-554/D-555 存量畸形行,integrity 告警清零;③D-331 的回归测试补上本次形态(双状态标记+非法 severity+污染取活依据);④复发计数落档:同形态第 2 次,若再现第 3 次升级为门禁硬拒
- 优先级: P1
- 对账: 2026-08-20 勘察补充:同类完整性脏数据另见 requirements-archive.md 的 R-221 条目——标题 [done] 但残留字段「- 状态: todo」,修复时一并纳入存量清理与回归形态

## D-570 research 上下文注入读 flat 路径而写入走 topic 路径,B2 后新增 S-/F- 对 agent 不可见 [open] (high)
- refs: R-221 R-248 R-277
- 复杂度: 小
- 复现: crates/kanzei-tools/src/profiles/research.rs:199-246 的 index_of 用 DocStore::open(&ctx.project_root, kind) 读死 .kanzei/research/sources.md 平铺路径;而 tracker.rs:316-329(R-221 B2)强制 source/finding 写入 .kanzei/research/<topic>/。结果 research agent 每轮 <research-docs> 索引只看到 2026-08-16 那批 flat 遗留 19 S/11 F,B2 之后 topic 目录里新写的条目永不出现
- 影响: 「引用在收集时绑定」被釜底抽薪:研究员看不见自己刚收集的来源;因 08-17 后 research 零使用一直未被撞见,R-248 prior-art 上线后必触发
- 标签: 后端
- 验收: ①index_of 按当前 topic 聚合注入(topic+flat 遗留两段均可见);②新写 S-/F- 在同会话下一轮注入中可见的定向测试;③r221-chain 等存量 topic 目录条目回读验证
- 优先级: P2

## D-571 websearch 端点本机不可达且无降级提示,轮次预算对直调不设防 [open] (medium)
- refs: R-277 R-248
- 复杂度: 中
- 复现: ①findings.md F-011(V3 本地实测):DuckDuckGo HTML 端点直连与本地代理均不可达,arXiv API 可用;websearch.rs 无可达性探测、无 arXiv/webfetch 降级提示,首次调用撞 30s 超时。②websearch.rs/webfetch.rs 对 research_loop 零引用:R-277 验收④「原始输出不进主上下文/轮次上限」只约束走 begin_search 的路径,模型绕开 loop 直调 websearch 无机械拦截
- 影响: research 检索主力工具在本机是坏的且失败模式是静默超时;预算旋钮是登记式约束非机械闸
- 标签: 后端
- 验收: ①端点不可达时给明确诊断并指引 webfetch+arXiv 通道(F-011 结论进代码);②websearch/webfetch 纳入 loop 预算或对绕行直调设轮次闸;③真实网络环境下降级路径实测
- 优先级: P3

## D-575 导航类工具失手只回裸错误:不存在路径/越界范围/漏参数无自愈信息 [open] (medium)
- 复杂度: 中
- 复现: 真实运行轨迹一轮内多次导航失手(2026-08-20 用户提供的外部评估引用):symbols 查不存在的 coordinator.rs、read 不存在的 invariant.rs、read lib.rs 范围越界、insert 漏 path 参数、读错 memory 路径——工具只回「不存在/失败」,不给最近邻候选、合法范围或必填参数点名,agent 靠再猜恢复
- 影响: 每次失手消耗一轮工具调用与上下文预算,认知预算耗在操作 harness 而非解决软件问题;弱模型(自举档)恢复能力更差,失手被放大;外部评估把仓库导航效率(7.8)定为与 Claude Code/Codex 的最大差距
- 来源: 2026-08-20 用户提供外部工程评估引用的真实运行轨迹失手清单;设计文档 docs/design/weakness_register_20260820.md
- 标签: 后端
- 验收: ①read/symbols/edit/insert 目标路径不存在时返回同目录最近邻文件候选;②读取范围越界时返回文件实际行数与合法范围;③必填参数缺失时点名参数并给一行示例;④memory 路径错误时提示正确根路径;⑤各失手形态有定向测试
- 优先级: P2

## D-576 删除本线临时 worktree 被 cross-tree detector 误报 [open] (low)
- 复现: 主线创建临时 worktree 后执行 git worktree remove --force，cross-tree detector 将被删除树报告为另一条线改动并生成整树 quarantine 清单。
- 影响: 正常的临时 worktree 清理产生大规模隔离噪声，误导为跨线越界并污染研究/隔离目录；本次未回滚仓库内容。
- 期望: 识别本 run 创建并删除的 worktree，清理动作不应被归因成另一条活跃线改动，也不应整树 quarantine。
- 来源: self-found during R-306 B4 merge-preview cleanup
- 标签: 核心
- refs: R-306
- 优先级: P3

## D-577 raw_lines 把空行判成游离段落且 raw_delete 报成功后游离行仍在,后置条件不成立 [open] (medium)
- 复杂度: 中
- 复现: 两处独立复现。①文章获取器测试项目(2026-08-20):R-002 raw_lines 报 1 条「(空行)」游离行,轨迹显示 raw_delete 返回「已删除第 1 条游离行」后再查仍在;D-001 据此登记并带着未复核的后置条件(进展自写「复核应确认 raw_lines 为空」)归档 fixed,本会话复查游离行依旧在。②kanzei 主库当场复现:R-310/R-311 均为本日 kz CLI req add 正常登记(多 --field 路径),raw_lines 各报 1 条「(空行)」;同日同路径登记的 R-313 却没有——正常登记/更新路径自身就会产生该「游离段落」,与「历史多行写法/手改残留」的工具自述不符,基本可定性检测把序列化产物空行误判为不可寻址内容
- 影响: 工具返回语义误导 agent:报成功但后置条件不成立,弱模型陷入 raw_delete 循环并把未验证的 fixed 写进归档;纯空行本不该被判为不可寻址游离段落;产生元数据治理执行噪音,消耗轮次
- 来源: 2026-08-20 需求发现实测(文章获取器项目)+外部评估点名;本会话已在该项目现场复现
- 标签: 核心
- 验收: ①定性空行游离判定是否误报,若误报则空行不再计为游离段落;②raw_delete 返回前复查后置条件,删不掉如实报错而非报成功;③文章获取器 R-002 现场复核游离行清零;④回归测试覆盖「删除报成功后仍存在」形态
- 优先级: P2

## D-578 memory manager 把该判 NOOP 的 inbox note 编造成无关根因 fact 落盘 active [open] (medium)
- refs: D-567 R-308
- 复杂度: 中
- 复现: 文章获取器测试项目 .kanzei/memory/M-001(2026-08-19):标题「完成 D-001(fixed)的根因:知乎/大需求拆解流程失效」,正文根因为编造话术(「collaboration_status 环节缺失有效任务分解信号,导致后续 bash→defect→work→files→glob 流程无法正确分支…不可跳过 decompose 步骤」),与 D-001 实际内容(tracker 元数据游离行清理)毫无关系;subject=安装通道 同样无关;inbox note 模板明确写「若是本条目的具体 bug 且无外推价值,判 NOOP 不要产出」,正确动作是 NOOP
- 影响: 记忆被无中生有的「事实」污染且 status=active 直接进常驻注入索引;NOOP 纪律只有提示词在守,弱模型跑 manager 时编造倾向更强;与 D-567(消化零产出)相反方向——消化端出毒比不出货更糟
- 来源: 2026-08-20 需求发现实测(文章获取器项目)复核发现
- 标签: 后端
- 验收: ①manager 产出 fact 必须带可核验出处,与 refs 条目明显无关的产出(如正文与条目零词汇关联)被机械拒绝或降级 candidate 不进 active;②NOOP/产出/驳回有遥测计数;③文章获取器 M-001 形态成回归用例;④与 R-308 晋升门槛机械化对齐不重复实现
- 优先级: P2

## D-582 循环宿主执行 verify.ps1 报 AuthorizationManager check failed,脚本零秒失败 [open] (medium)
- refs: R-306
- 复现: 循环内 bash 工具执行 & .\scripts\verify.ps1 于 0.0s 失败,PowerShell 返回 AuthorizationManager check failed,脚本第 1 行未执行,证据 T-1786922726507;主会话 Claude Code 同机同脚本解析正常(Process=Bypass,LocalMachine=RemoteSigned)
- 影响: verify 十三步门禁在循环内不可执行,复杂度大条目的关闭验收与发版前置证据只能移交外部会话,循环自闭环断链
- 来源: 2026-08-20 R-306 B4 现场,tests-archive T-1786922726507
- 标签: 核心
- 验收: ①定位根因(宿主 AuthorizationManager/执行策略继承/扩展路径前缀之一);②循环内可跑通 verify.ps1 或提供明确替代通道并写入约定;③循环环境 .ps1 可执行性有冒烟回归
- 优先级: P1

## D-583 鞭挞机制缺连续零产出熔断,R-306 空转 10 轮无停机上报 [open] (medium)
- refs: R-306 R-307 D-504
- 复现: 2026-08-20 R-306 现场:鞭挞计数 7→10,轮次产出 steps 32→10→7→2,最后两轮零文件改动零提交,仅重复背诵同一份证据清单;会话累计 313 条,无熔断无上报,直到用户人工发现
- 影响: 当剩余缺口全是循环无法自解的外部阻塞(权限环境、需用户决策、真实合并冲突)时,鞭挞持续烧 token 空转,活锁无上限
- 来源: 2026-08-20 用户现场发现 R-306 空转,主会话诊断确认活锁三根因(祖先链验收不可自满足/verify 环境挡死/进展提交被混入卡住)
- 标签: 核心
- 验收: ①连续 N 轮(建议 2~3)无文件改动、无提交、无 tracker 实质字段变化即熔断停鞭,输出零产出诊断并点名阻塞清单;②现场案例成回归:模拟连续零产出轮次触发熔断;③熔断事件留痕可审计
- 优先级: P1

## D-585 在线记忆召回只记录 ACTION_CHANGED，OUTCOME_IMPROVED 永久无生产证据 [open] (medium)
- 复现: FailureRecallPolicy::record_outcomes 仅写 memory_eval.arm=action_changed；funnel_counts 对 outcome_improved 只能查到 0 行并标 unavailable。
- 影响: 控制面 F(m)/漏斗无法展示真实最终结果改善，生产数据不能触发 outcome_improved 相关判断。
- 来源: self-found：复核 R-293 代码与 b085499c 后确认
- 标签: 后端
- 验收: 运行期完成结局且存在真实召回注入时写入独立 outcome_improved 证据；暂停/失败结局不误报；回归测试覆盖。
- refs: R-293
- 优先级: P1

## D-586 RecallRunOutcome 未从 kanzei-core 根导出导致 memory crate 编译失败 [open] (medium)
- 复现: kanzei-memory 的 FailureRecallPolicy 实现引用 kanzei_core::RecallRunOutcome；类型仅在 kanzei_core::runner 导出，crate 根 lib.rs 未 re-export，cargo test -p kanzei-memory 编译失败。
- 影响: R-293 批次1 的生产 outcome 写入实现无法编译，memory crate 消费者不能使用运行结局契约。
- 来源: self-found：R-293 批次1 定向测试 T-1786922726510 后发现
- 标签: 核心
- 验收: kanzei-memory 可通过 cargo test -p kanzei-memory 编译；RecallRunOutcome 从 kanzei_core 根可用。
- refs: R-293 T-1786922726510
- 优先级: P1

## D-587 子代理面板缺停止运行中任务能力,process-subagents 开关只挡下一轮不打断在跑请求 [open] (medium)
- refs: R-281
- 复现: 2026-08-20 用户想腾 GPU 显存,点顶栏子代理开关(process-subagents)预期能停掉正在跑的子代理,实际该开关只控制下一轮工具面是否含 task(lifecycle.rs:409-412 即时生效但只影响未来),对已建立的推理请求无效;06-agent-panel.js 全文搜索确认面板只有 agent-clear(清已完成条目)与 agent-close(关闭面板视图)两个按钮,没有 stop/cancel 单个运行中子代理的操作;后端 task_cancel_parallel.rs 证明取消能力存在但从未接到前端;实测 kzapp(PID)与 ollama 有 Established 连接、ollama stop 卡 Stopping 十余秒不释放,唯一手段是 taskkill 强杀 llama-server 整个进程,粒度过粗
- 影响: 用户想因资源紧张(显存/token)临时打断某个子代理,除了杀整个本地推理进程外无其他手段;子代理开关名不副实,容易造成误解
- 来源: 2026-08-20 用户腾 GPU 显存给本地模型让路,主会话诊断
- 标签: 前端
- 验收: ①子代理面板每个运行中条目有停止/取消按钮,调用已有的取消能力;②process-subagents 开关文案或行为二选一对齐:要么明确标注仅影响下一轮,要么增加连带打断在跑任务的选项;③真实取消一个运行中子代理有回归测试
- 优先级: P2
