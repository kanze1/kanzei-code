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

## D-566 cross-tree 隔离快照无回收且构建产物误报,121 目录 143MB 纯堆积 [fixing] (medium)
- refs: D-395 D-397 R-306
- 复杂度: 中
- 复现: .kanzei/quarantine/ 现存 121 个目录共 143MB(shell-with-log 82、cross-tree 32、bg 7),最早 2026-08-16,无任何回收路径。抽查 cross-tree-1787021081327:内容是 crates/kanzei-app/gen/schemas/desktop-schema.json 与 windows-schema.json——Tauri 构建产物,任一线跑构建即重生成,被越界检测当跨树写入取证;cross-tree-1787060772922 存的是 p16 线自己 R-299 B1 提交(7188ba76)的前置内容,合法自身工作被判越界。08-16 单日 30 次 cross-tree 隔离(两线并行互撞日)
- 影响: 误报占绝大多数,真越界信号被淹没;143MB 取证快照只进不出;构建产物类路径每次并行构建都会再触发
- 标签: 流程
- 验收: ①构建产物路径(gen/schemas 等)进入越界检测豁免清单或按内容指纹放行,有定向测试;②quarantine 提供按日期/类型的清理入口(dry-run+实际释放量),真越界证据可显式保留;③清理后 121 个存量目录处置留痕;④真实并行双线构建实测不再产生 schema 误报
- 优先级: P2
- 进展: B1 已提交 38497890：cross_tree.rs:48-82 新增 gen/schemas 完整路径级豁免，:214-218/:298-300 两条扫描路径共用 is_excluded_path；新增 tools/quarantine.rs:1-202，已知类型/毫秒时间戳扫描、dry-run eligible_bytes、apply freed_bytes、未知命名目录 preserved_paths。B2 已提交 e4ecfcf1：cli/quarantine.rs:1-121 提供真实 `kz quarantine` 调用方，默认 dry-run，apply 必须带 type/date 筛选；cli/mod.rs:20-56 接入分发，:86-90 接入帮助。B2 审计补强已提交 3adcfd66：tools/quarantine.rs:21-51 新增 CleanupAudit，:96-126 append_audit 每次成功 dry-run/apply 追加 `.kanzei/quarantine/cleanup-log.jsonl`，记录 eligible_paths 与 preserved_paths、筛选条件、字节和模式；:213-217 单测断言候选与保留目录均进 JSONL。T-1786922726528：393 passed、0 failed、1 ignored；T-1786922726529：真实 CLI dry-run 生成审计记录。新增真实并行验证：T-1786922726530，主树与同 HEAD 临时 worktree 并行执行 cargo tauri build --config {bundle active=false} 均 exit=0，两边各生成4个 gen/schemas 文件，主树 quarantine 目录数保持1、临时树为0。验收对账：①已完成，路径级豁免与回归在 cross_tree.rs:48-82、:806-828，T-1786922726524；②已完成，真实 CLI 为 cli/mod.rs:52、cli/quarantine.rs:58-91，dry-run/apply证据 T-1786922726527，未知证据保留由 tools/quarantine.rs:82-93、:128-156 保证；③新增未来每次清理的逐目录审计留痕：tools/quarantine.rs:96-126 的 cleanup-log.jsonl，T-1786922726528/6529 已断言候选/保留路径落盘并由真实 CLI 触发；历史基线仍为121目录/143MB，当前盘点为1目录/1204224 bytes，历史120目录无独立逐目录 manifest，故历史存量部分仍显式降级；④已完成，真实并行双线 Tauri 构建 T-1786922726530：两 worktree 同 HEAD、两个真实构建均成功，gen/schemas 各4个且无新增 quarantine schema 误报。下一步：提交本次并行验证记录；D-566 仍 fixing，仅③历史逐目录证据缺口未关闭。
- observed_head: 3adcfd663ccd5736581d107aa09ffa36a3926fd6
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787192820656
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-566(unblocks=0)
- 批次: 2/2
- 停车: 历史121目录逐目录manifest无法从当前文件系统重建；代码与真实并行构建已完成，暂让位下一条可执行缺陷；恢复人:agent，恢复条件:找到历史清单或重新产生可逐目录审计的存量窗口

## D-568 记忆 INDEX 描述串号污染:M-014/M-015 描述抄错条目,毒化 FTS 检索 [fixing] (medium)
- 复杂度: 小
- 复现: .kanzei/memory/INDEX.md:M-014 标题「HTML 静态文案必须登记进资源表」但描述整段是 M-009 的「edit 报 old_string not found 时必读…」;M-015 标题「SSE 流内 context overflow」描述却是 M-029 的「处理 bash git 拦截…结构化工具显式 stage」。index.db 的 memory_fts 索引 description 字段,错配描述使这两条在错误查询下被召回
- 影响: FTS 检索被毒化:错误主题命中错误记忆;INDEX 是每会话注入的真源,串号直接影响召回质量
- 标签: 后端
- 验收: ①M-014/M-015 描述修正与源文件 description 一致;②全量 INDEX 行与对应 M-*.md 的 description 做一次机械一致性核对,输出不一致清单并修复;③重建 index.db FTS 后检索抽查不再串号;④INDEX 生成/更新路径补一致性断言防复发
- 优先级: P2
- 取活依据: engine:唯一可执行 WIP 是 D-568，必须先恢复它
- 进展: 对账 2026-08-20(resume reconcile):④已落地——7c238573(D-590)在 store.rs assert_index_matches_entries 接入 refresh_derived 写入路径+守护测试 index_description_guard_rejects_mismatched_source,验收④视为既有能力核销。①②③未落地,且发现比登记更深:不止 INDEX 串号,M-014/M-015 源文件本身 description+正文整段串号(当前 M-014 正文是 M-009 的 edit SOP、M-015 正文是 M-029 的 git 拦截 SOP),真源=git 1476098e 建条原始版(已从历史取出全文)。修正路径被 managed fence 挡死:.kanzei/memory/*.md 仅 memory 写工具白名单可写,edit 被拒(R-316 来源实录);同步修正通道 R-316 仍 todo。下一步:探查既有 memory 工具族是否已有改现有条目文本的能力,无则按 R-316 最小实现(memory 文本修正工具+fence 白名单+审计留痕),落地后修 M-014/M-015 源文件→refresh_derived 重建 INDEX+FTS→②全量机械核对→③FTS 抽查。
- observed_head: 11b60ae32647a5ff999329120316e8ffebad7fd8
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787203506741
- 停车: 停车: 当前主 agent 无同步 memory_update 工具,修源文件必须先交付 R-316 同步通道;恢复人:agent;恢复条件:R-316 提供真实调用路径后继续修 M-014/M-015、重建 INDEX/FTS。

## D-569 tracker 完整性退化复发(D-331 同形态):归档标题双状态标记与非法 severity 再现 [fixing] (high)
- refs: D-331 D-553 D-554 D-555
- 复杂度: 中
- 复现: defects-archive.md:6822 的 D-553 标题行为「... [open] (small) [fixed]」——同行两个状态标记且 (small) 非法(合法 severity 为 high/medium/low);D-554 同样「[done] (small) [fixed]」;defects-archive.md:6829 引擎在取活依据字段写入「[tracker integrity degraded] D-555: invalid defect lifecycle [done]」,该告警全库出现 4 次。D-331(已 fixed,high)当时修的正是「归档终态无法安全修正且非法状态污染缺陷标题」,b140322 加了跨 DocKind 状态标记校验+fix_terminal,08-20 同形态再现说明校验有漏洞或写入方绕过
- 影响: 归档数据被污染且按 D-331 教训会扩散:完整性门禁降级告警混入取活依据字段,畸形条目无法被 list/get 正确解析;M-012 类机制恶化可能拒绝所有 tracker 写操作
- 标签: 核心
- 验收: ①定位本次畸形写入的具体路径(哪个写入方绕过了 b140322 校验)并封堵;②用 fix_terminal/repair 修正 D-553/D-554/D-555 存量畸形行,integrity 告警清零;③D-331 的回归测试补上本次形态(双状态标记+非法 severity+污染取活依据);④复发计数落档:同形态第 2 次,若再现第 3 次升级为门禁硬拒
- 优先级: P1
- 对账: 2026-08-20 勘察补充:同类完整性脏数据另见 requirements-archive.md 的 R-221 条目——标题 [done] 但残留字段「- 状态: todo」,修复时一并纳入存量清理与回归形态
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-569(unblocks=0)
- 复发计数: 2（D-331 历史形态→本次 D-553/D-554/D-555 再现）
- 进展: D-569 收口对账：①具体绕过路径=历史直写提交 3c123bd5(D-553)与36ccb253(D-554/D-555)未经过当前 add/update 的 check_title/check_severity；当前写入封堵位于 crates/kanzei-tools/src/tracker.rs:644-660，归档修复写日志补线位于 tracker.rs:483-496，fix_terminal 入口位于 crates/kanzei/src/cli/tracker.rs:146-156。②存量修正=通过真实 kz defect fix_terminal 持久化清理 D-553/D-554/D-555(defects-archive.md:6822/6836/6851)，并按对账清理 R-221(requirements-archive.md:3645)、D-172、D-283；kz defect list 与 kz req list 均无 tracker integrity 告警。③回归=crates/kanzei-memory/src/docstore.rs:489-544 覆盖双状态+非法 severity+状态字段污染与修复后清零，validation.rs:158-183 统一检测，tracker.rs:380-404 普通写硬拒；T-1786922726538 通过(kanzei-memory 156、kanzei-tools 394)。④复发计数已落档为2；第三次及以后由 tracker.rs:380-404 直接硬拒普通写，修复动作仍经 FIX_TERMINAL_ACTION/normalize 显式收口。
- 门禁: 同形态第 3 次及以后沿用 tracker.rs:380-404 完整性硬拒；仅 fix_terminal/normalize 修复通道可用，普通 add/update/close/archive 等写操作拒绝
- 验收核验: ①已完成：3c123bd5/36ccb253历史直写绕过点已定位，tracker.rs:644-660与483-496封堵。②已完成：D-553/D-554/D-555及对账R-221/D-172/D-283经fix_terminal修正，双库list无integrity告警。③已完成：docstore.rs:489-544、validation.rs:158-183、tracker.rs:380-404与T-1786922726538。④已完成：复发计数=2已落档；第三次及以后普通写沿tracker.rs:380-404硬拒。
- observed_head: 11b60ae32647a5ff999329120316e8ffebad7fd8
- observed_worktree_hash: fnv1a64:72ca3632fdae6c18
- recorded_at: 1787207020185

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

## D-587 子代理面板缺停止运行中任务能力,process-subagents 开关只挡下一轮不打断在跑请求 [open] (medium)
- refs: R-281
- 复现: 2026-08-20 用户想腾 GPU 显存,点顶栏子代理开关(process-subagents)预期能停掉正在跑的子代理,实际该开关只控制下一轮工具面是否含 task(lifecycle.rs:409-412 即时生效但只影响未来),对已建立的推理请求无效;06-agent-panel.js 全文搜索确认面板只有 agent-clear(清已完成条目)与 agent-close(关闭面板视图)两个按钮,没有 stop/cancel 单个运行中子代理的操作;后端 task_cancel_parallel.rs 证明取消能力存在但从未接到前端;实测 kzapp(PID)与 ollama 有 Established 连接、ollama stop 卡 Stopping 十余秒不释放,唯一手段是 taskkill 强杀 llama-server 整个进程,粒度过粗
- 影响: 用户想因资源紧张(显存/token)临时打断某个子代理,除了杀整个本地推理进程外无其他手段;子代理开关名不副实,容易造成误解
- 来源: 2026-08-20 用户腾 GPU 显存给本地模型让路,主会话诊断
- 标签: 前端
- 验收: ①子代理面板每个运行中条目有停止/取消按钮,调用已有的取消能力;②process-subagents 开关文案或行为二选一对齐:要么明确标注仅影响下一轮,要么增加连带打断在跑任务的选项;③真实取消一个运行中子代理有回归测试
- 优先级: P2

## D-591 设置页 Provider 表格只读全局配置,项目级 kanzei.toml 新增的 provider 保存时报 unknown provider [open] (medium)
- refs: D-288
- 复现: settings.rs:501 settings_get 读 crate::global_config_path()(即 kanzei_home()/kanzei.toml,~/.kanzei/kanzei.toml),不合并项目级 .kanzei/kanzei.toml 的 [providers.*] 段;validate_model_roles(settings.rs:167-198)校验只用前端 payload.providers(即这份只含全局 provider 的表格数据)。用户在项目级配置里加了 [providers.llama-local],CLI(kz run)能正常解析使用(resolve_model 走的是合并后的项目+全局配置),但桌面端设置页 primary 填 llama-local:xxx 保存时报 unknown provider,重启 kzapp 无效——因为读的压根不是同一份文件
- 影响: 项目级自定义 provider 无法通过桌面端 UI 选用,用户表现为反复重启无效、误以为是配置写错或缓存问题,实际是两处数据源天生不同步;唯一解法是把 provider 定义也复制一份到全局配置,体验上完全不透明
- 来源: 2026-08-20 用户接入本地 llama-server(llama-local provider)现场,重启后仍报错,主会话定位
- 标签: 前端
- 验收: ①settings_get 合并展示项目级+全局的 provider(标注来源,与 R-184 全局/项目分层显示一致);②validate_model_roles 校验时同样按合并后的完整 provider 集合走,不能只认前端表格里显示的那部分;③项目级新增 provider 后设置页无需手动同步一份到全局即可选用,有回归测试
- 优先级: P2

## D-592 上下文预算检查信 bytes/4 估算不锚定真实 usage,本地小窗口模型压缩零触发直至撞 400 [open] (high)
- refs: D-203 D-206 R-219 R-236
- 复现: 2026-08-20 现场:llama-local(qwen3.8-27b,llama-server n_ctx=65536)鞭挞 D-568 任务,真实请求 69889 tokens 撞 provider 400(exceed_context_size_error),全程主动压缩零触发。判定链 context_budget.rs:51 用 bytes/4 估算(context.rs:130)×校准因子与触发线比大小,三重系统性偏低叠加:①bytes/4 对中文(UTF-8 3字节/字实际≈1~1.5 token/字)、代码、llama.cpp jinja 模板渲染的工具 schema 膨胀,合计偏低>2.1×(69889 真实 vs 触发线 32768 未达);②校准单步比值 clamp [0.5,2.0](context.rs:165),系统性偏差≥2× 时数学上限封死追不上,EMA 0.7/0.3 收敛慢且每 run 重置 1.0(assembly.rs:195),恢复大历史的新对话首步最脆;③compaction_budget=limit−max(max_tokens,buffer)(context.rs:92),全局 max_tokens=32768 吃掉 65536 窗口一半。usage 回读链路本身是通的(openai.rs:110 include_usage,drive.rs:609 拿真实 prompt_tokens),但只喂校准 EMA,预算比大小不直接用——真实值在手边,决策看估算
- 影响: 本地小窗口 provider 跑长任务必然在压缩触发前撞墙 400,自主推进直接致命中断;窗口越小、内容越偏中文/代码,撞墙越早;98304 窗口同样防不住(偏差>1.5× 即穿)
- 来源: 2026-08-20 用户实测反馈『快摸到上限了还是没压缩』,主会话诊断
- 标签: 核心
- 边界: 历史侧 Part::Reasoning 剪枝(openai 协议 build_body openai.rs:83-91 从不回传,drive.rs:582 却存进历史虚增估算)只能与本条①同批落地——单独剪会让估算更小、压缩触发更晚,加重症状;Qwen 官方口径『多轮剥离 thinking 但多步工具调用期间保留』的质量权衡(llama-server --reasoning-preserve)不在本条,另行评估
- 验收: ①预算检查锚定上一步真实 prompt_tokens(last_input_tokens 已在手)+本步新增内容估算增量,bytes/4 全量估算只做冷启动兜底;②校准按 provider 持久化或冷启动用保守初值,消除每 run 重置 1.0 的首步裸奔;③compaction_budget 对小窗口自适应,max_tokens 不得吃掉固定一半窗口;④回归:模拟估算偏低 2 倍场景压缩在撞墙前触发;⑤llama-local 真实长任务(多步工具循环读大文件)实测不再 400
- 优先级: P1

## D-593 上下文占用显示轮末才刷新且与预算引擎两套口径,长 prefill 期间滞后一整步 [open] (medium)
- refs: D-592
- 复现: UI 占用 ctxTokens=input+cacheRead 只在轮末 kz:step 事件更新(07-events.js:286),本地模型一步 prefill 数分钟,期间显示冻结在上一轮旧值;显示走 provider 真实 usage,压缩决策走 bytes/4 估算(D-592),两套口径——用户看到的数字与引擎行为对不上。2026-08-20 用户实测反馈『渲染显示的上下文不准确』
- 影响: 长轮运行中用户无法判断真实占用,临限无感知;引擎该压不压时显示也给不出预警
- 来源: 2026-08-20 用户实测反馈,主会话诊断
- 标签: 前端
- 验收: ①运行中占用显示与预算引擎同口径(D-592 改锚定真实值后两边天然收敛);②轮内长 prefill 期间显示不冻结——至少标注滞后/计算中,或用引擎估算实时刷新并标注口径;③对 context_limit 的占比展示与撞线告警准确
- 优先级: P2
